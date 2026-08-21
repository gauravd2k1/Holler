package inventory

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/postgres"
)

// pgUniqueViolation mirrors backend/internal/kitchen and backend/internal/
// tables's convention: the PostgreSQL SQLSTATE for unique_violation.
const pgUniqueViolation = "23505"

func isUniqueViolation(err error) bool {
	var pgErr *pgconn.PgError
	return errors.As(err, &pgErr) && pgErr.Code == pgUniqueViolation
}

// Repository is the persistence boundary for the inventory context. Service
// depends on this interface, never on pgx directly (CLAUDE.md §Coding
// rules).
type Repository interface {
	WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error

	// BumpOutletConfigVersion increments outlet.config_version by exactly
	// one, mirroring backend/internal/kitchen and backend/internal/
	// compliance.
	BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error)

	// OutletBelongsToTenant reports whether outletID belongs (via brand) to
	// tenantID — every inventory write is scoped through this.
	OutletBelongsToTenant(ctx context.Context, tenantID, outletID string) (bool, error)

	// --- inventory_item + item_unit_conversion ----------------------------

	// UpsertInventoryItem creates or updates an inventory_item and replaces
	// its item_unit_conversion children wholesale — the station_printer
	// PUT-not-merge precedent, applied to a create-or-update route.
	UpsertInventoryItem(ctx context.Context, tx pgx.Tx, item InventoryItem, conversions []ItemUnitConversion) error
	InventoryItemDimension(ctx context.Context, itemID string) (Dimension, bool, error)
	InventoryItemsSince(ctx context.Context, outletID string, sinceVersion int) ([]InventoryItem, error)
	ItemUnitConversionsSince(ctx context.Context, outletID string, sinceVersion int) ([]ItemUnitConversion, error)

	// --- recipe + recipe_ingredient ---------------------------------------

	// UpsertRecipe creates or updates a recipe and replaces its
	// recipe_ingredient children wholesale. recipe_version increments on
	// every edit that already exists; a fresh recipe starts at 1.
	UpsertRecipe(ctx context.Context, tx pgx.Tx, recipe Recipe, ingredients []RecipeIngredient) error
	RecipeOutputDimension(ctx context.Context, recipeID string) (Dimension, bool, error)
	// RecipeExistsForVariant reports the recipe_version to use for an
	// upsert: 0 (meaning "start at 1") for a new recipe, or the current
	// stored version + 1 for an edit — recipe_version increments on EVERY
	// edit (ADR-018 §6).
	RecipeVersionForVariant(ctx context.Context, menuItemVariantID string) (recipeID string, nextVersion int, exists bool, err error)
	// ReachableSubRecipes runs the recursive-CTE reachability check pinned
	// in packages/contracts/sqlite/0015_m4_inventory_config.sql's header:
	// every recipe_id reachable from proposedSubRecipeID by following
	// sub_recipe_id edges, each paired with its depth from the proposed
	// root (1-based). UNION (not UNION ALL) dedups, so this terminates even
	// over a graph that already contains a cycle elsewhere.
	ReachableSubRecipes(ctx context.Context, proposedSubRecipeID string) (map[string]int, error)
	RecipesSince(ctx context.Context, outletID string, sinceVersion int) ([]Recipe, error)
	RecipeIngredientsSince(ctx context.Context, outletID string, sinceVersion int) ([]RecipeIngredient, error)
	// MenuItemVariantOutlet resolves the outlet a menu_item_variant belongs
	// to, so a recipe write (which has no outlet_id of its own) can still be
	// checked against the caller's tenant — the same pattern
	// backend/internal/kitchen.MenuItemOutlet uses for station routing.
	MenuItemVariantOutlet(ctx context.Context, variantID string) (string, bool, error)

	// --- modifier_ingredient_delta (read-only here; no write route of its
	// own — it rides inside the MenuItem config bundle, backend/internal/
	// menu's territory) ------------------------------------------------
	ModifierIngredientDeltasSince(ctx context.Context, outletID string, sinceVersion int) ([]ModifierIngredientDelta, error)

	// --- stock_ledger_entry / stock_deduction_gap --------------------------

	// LastEntrySeq returns the highest entry_seq recorded for outletID, or 0
	// if none exist yet — the cursor the contiguity check compares an
	// incoming entry_seq against (ADR-018 replay addendum).
	LastEntrySeq(ctx context.Context, outletID string) (int64, error)
	// GetLedgerEntryBySeq looks up an existing row by (outlet_id, entry_seq)
	// so a replayed envelope for an already-ingested entry_seq can be told
	// apart from a genuine conflicting write.
	GetLedgerEntryBySeq(ctx context.Context, outletID string, entrySeq int64) (StockLedgerEntry, bool, error)
	GetLedgerEntryByID(ctx context.Context, id string) (StockLedgerEntry, bool, error)
	InsertLedgerEntry(ctx context.Context, entry StockLedgerEntry) error
	GetDeductionGapByID(ctx context.Context, id string) (StockDeductionGap, bool, error)
	InsertDeductionGap(ctx context.Context, gap StockDeductionGap) error

	// --- stock_count ---------------------------------------------------

	GetStockCountByID(ctx context.Context, id string) (StockCount, bool, error)
	// InsertStockCount stores a stock_count and its stock_count_line
	// children in one transaction. Idempotent on id: a duplicate replay of
	// the same count id is a no-op.
	InsertStockCount(ctx context.Context, tx pgx.Tx, count StockCount, lines []StockCountLine) error
	GetStockCountLines(ctx context.Context, countID string) ([]StockCountLine, error)
}

type pgRepository struct {
	pool postgres.Pool
}

// NewRepository returns a Repository backed by a live PostgreSQL pool.
func NewRepository(pool postgres.Pool) Repository {
	return &pgRepository{pool: pool}
}

func (r *pgRepository) WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("inventory: begin tx: %w", err)
	}
	if err := fn(tx); err != nil {
		_ = tx.Rollback(ctx)
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("inventory: commit tx: %w", err)
	}
	return nil
}

func (r *pgRepository) BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error) {
	var newVersion int
	err := tx.QueryRow(ctx,
		`UPDATE outlet SET config_version = config_version + 1, updated_at = now()
		 WHERE id = $1 RETURNING config_version`,
		outletID,
	).Scan(&newVersion)
	if errors.Is(err, pgx.ErrNoRows) {
		return 0, fmt.Errorf("%w: outlet %s", httpx.ErrNotFound, outletID)
	}
	if err != nil {
		return 0, fmt.Errorf("inventory: bumping outlet config_version: %w", err)
	}
	return newVersion, nil
}

func (r *pgRepository) OutletBelongsToTenant(ctx context.Context, tenantID, outletID string) (bool, error) {
	var exists bool
	err := r.pool.QueryRow(ctx,
		`SELECT EXISTS(
			SELECT 1 FROM outlet o JOIN brand b ON b.id = o.brand_id
			WHERE o.id = $1 AND b.tenant_id = $2
		 )`,
		outletID, tenantID,
	).Scan(&exists)
	if err != nil {
		return false, fmt.Errorf("inventory: checking outlet tenancy: %w", err)
	}
	return exists, nil
}

// --- inventory_item ----------------------------------------------------

func (r *pgRepository) UpsertInventoryItem(ctx context.Context, tx pgx.Tx, item InventoryItem, conversions []ItemUnitConversion) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO inventory_item (id, outlet_id, sku, name, category, dimension, reorder_level_micro, par_level_micro, storage_location, is_active, yield_factor_ppm, config_version)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
		 ON CONFLICT (id) DO UPDATE SET
		   sku = EXCLUDED.sku, name = EXCLUDED.name, category = EXCLUDED.category,
		   reorder_level_micro = EXCLUDED.reorder_level_micro, par_level_micro = EXCLUDED.par_level_micro,
		   storage_location = EXCLUDED.storage_location, is_active = EXCLUDED.is_active,
		   config_version = EXCLUDED.config_version`,
		item.ID, item.OutletID, item.SKU, item.Name, item.Category, string(item.Dimension),
		item.ReorderLevelMicro, item.ParLevelMicro, item.StorageLocation, item.IsActive,
		item.YieldFactorPPM, item.ConfigVersion,
	)
	if err != nil {
		if isUniqueViolation(err) {
			return fmt.Errorf("%w: sku %q already exists in this outlet", httpx.ErrConflict, item.SKU)
		}
		return fmt.Errorf("inventory: upserting inventory item: %w", err)
	}
	if _, err := tx.Exec(ctx, `DELETE FROM item_unit_conversion WHERE inventory_item_id = $1`, item.ID); err != nil {
		return fmt.Errorf("inventory: clearing item unit conversions: %w", err)
	}
	for _, c := range conversions {
		if _, err := tx.Exec(ctx,
			`INSERT INTO item_unit_conversion (id, inventory_item_id, pack_unit_label, source_dimension, numerator, denominator, config_version)
			 VALUES ($1, $2, $3, $4, $5, $6, $7)`,
			c.ID, item.ID, c.PackUnitLabel, string(c.SourceDimension), c.Numerator, c.Denominator, item.ConfigVersion,
		); err != nil {
			if isUniqueViolation(err) {
				return fmt.Errorf("%w: pack_unit_label %q already exists for this item", httpx.ErrConflict, c.PackUnitLabel)
			}
			return fmt.Errorf("inventory: inserting item unit conversion: %w", err)
		}
	}
	return nil
}

func (r *pgRepository) InventoryItemDimension(ctx context.Context, itemID string) (Dimension, bool, error) {
	var dim string
	err := r.pool.QueryRow(ctx, `SELECT dimension FROM inventory_item WHERE id = $1`, itemID).Scan(&dim)
	if errors.Is(err, pgx.ErrNoRows) {
		return "", false, nil
	}
	if err != nil {
		return "", false, fmt.Errorf("inventory: getting inventory item dimension: %w", err)
	}
	return Dimension(dim), true, nil
}

func (r *pgRepository) InventoryItemsSince(ctx context.Context, outletID string, sinceVersion int) ([]InventoryItem, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, outlet_id, sku, name, category, dimension, reorder_level_micro, par_level_micro, storage_location, is_active, yield_factor_ppm, config_version
		 FROM inventory_item WHERE outlet_id = $1 AND config_version > $2 ORDER BY config_version`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("inventory: listing inventory items since %d: %w", sinceVersion, err)
	}
	defer rows.Close()
	var out []InventoryItem
	for rows.Next() {
		var it InventoryItem
		var dim string
		if err := rows.Scan(&it.ID, &it.OutletID, &it.SKU, &it.Name, &it.Category, &dim,
			&it.ReorderLevelMicro, &it.ParLevelMicro, &it.StorageLocation, &it.IsActive,
			&it.YieldFactorPPM, &it.ConfigVersion); err != nil {
			return nil, fmt.Errorf("inventory: scanning inventory item: %w", err)
		}
		it.Dimension = Dimension(dim)
		it.SchemaVersion = 1
		out = append(out, it)
	}
	return out, rows.Err()
}

func (r *pgRepository) ItemUnitConversionsSince(ctx context.Context, outletID string, sinceVersion int) ([]ItemUnitConversion, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT c.id, c.inventory_item_id, c.pack_unit_label, c.source_dimension, c.numerator, c.denominator, c.config_version
		 FROM item_unit_conversion c
		 JOIN inventory_item i ON i.id = c.inventory_item_id
		 WHERE i.outlet_id = $1 AND c.config_version > $2 ORDER BY c.config_version`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("inventory: listing item unit conversions since %d: %w", sinceVersion, err)
	}
	defer rows.Close()
	var out []ItemUnitConversion
	for rows.Next() {
		var c ItemUnitConversion
		var dim string
		if err := rows.Scan(&c.ID, &c.InventoryItemID, &c.PackUnitLabel, &dim, &c.Numerator, &c.Denominator, &c.ConfigVersion); err != nil {
			return nil, fmt.Errorf("inventory: scanning item unit conversion: %w", err)
		}
		c.SourceDimension = Dimension(dim)
		c.SchemaVersion = 1
		out = append(out, c)
	}
	return out, rows.Err()
}

// --- recipe --------------------------------------------------------------

func (r *pgRepository) RecipeVersionForVariant(ctx context.Context, menuItemVariantID string) (string, int, bool, error) {
	var id string
	var version int
	err := r.pool.QueryRow(ctx,
		`SELECT id, recipe_version FROM recipe WHERE menu_item_variant_id = $1`,
		menuItemVariantID,
	).Scan(&id, &version)
	if errors.Is(err, pgx.ErrNoRows) {
		return "", 1, false, nil
	}
	if err != nil {
		return "", 0, false, fmt.Errorf("inventory: getting recipe version for variant: %w", err)
	}
	return id, version + 1, true, nil
}

func (r *pgRepository) UpsertRecipe(ctx context.Context, tx pgx.Tx, recipe Recipe, ingredients []RecipeIngredient) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO recipe (id, menu_item_variant_id, name, recipe_version, output_dimension, output_quantity_micro, config_version)
		 VALUES ($1, $2, $3, $4, $5, $6, $7)
		 ON CONFLICT (id) DO UPDATE SET
		   name = EXCLUDED.name, recipe_version = EXCLUDED.recipe_version,
		   output_dimension = EXCLUDED.output_dimension, output_quantity_micro = EXCLUDED.output_quantity_micro,
		   config_version = EXCLUDED.config_version`,
		recipe.ID, recipe.MenuItemVariantID, recipe.Name, recipe.RecipeVersion,
		string(recipe.OutputDimension), recipe.OutputQuantityMicro, recipe.ConfigVersion,
	)
	if err != nil {
		if isUniqueViolation(err) {
			return fmt.Errorf("%w: a recipe already exists for menu_item_variant_id %q", httpx.ErrConflict, recipe.MenuItemVariantID)
		}
		return fmt.Errorf("inventory: upserting recipe: %w", err)
	}
	if _, err := tx.Exec(ctx, `DELETE FROM recipe_ingredient WHERE recipe_id = $1`, recipe.ID); err != nil {
		return fmt.Errorf("inventory: clearing recipe ingredients: %w", err)
	}
	for _, ing := range ingredients {
		if _, err := tx.Exec(ctx,
			`INSERT INTO recipe_ingredient (id, recipe_id, component_kind, inventory_item_id, sub_recipe_id, quantity_micro, quantity_dimension, yield_factor_ppm, sort_order, config_version)
			 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)`,
			ing.ID, recipe.ID, string(ing.ComponentKind), ing.InventoryItemID, ing.SubRecipeID,
			ing.QuantityMicro, string(ing.QuantityDimension), ing.YieldFactorPPM, ing.SortOrder, recipe.ConfigVersion,
		); err != nil {
			return fmt.Errorf("inventory: inserting recipe ingredient: %w", err)
		}
	}
	return nil
}

func (r *pgRepository) RecipeOutputDimension(ctx context.Context, recipeID string) (Dimension, bool, error) {
	var dim string
	err := r.pool.QueryRow(ctx, `SELECT output_dimension FROM recipe WHERE id = $1`, recipeID).Scan(&dim)
	if errors.Is(err, pgx.ErrNoRows) {
		return "", false, nil
	}
	if err != nil {
		return "", false, fmt.Errorf("inventory: getting recipe output dimension: %w", err)
	}
	return Dimension(dim), true, nil
}

// ReachableSubRecipes is the recursive-CTE reachability check pinned in
// packages/contracts/sqlite/0015_m4_inventory_config.sql's header, run
// against PostgreSQL. UNION (not UNION ALL) dedups so a graph already
// containing a cycle elsewhere still terminates.
func (r *pgRepository) ReachableSubRecipes(ctx context.Context, proposedSubRecipeID string) (map[string]int, error) {
	rows, err := r.pool.Query(ctx,
		`WITH RECURSIVE reach(recipe_id, depth) AS (
		     SELECT $1::uuid, 1
		     UNION
		     SELECT ri.sub_recipe_id, r.depth + 1
		       FROM recipe_ingredient ri
		       JOIN reach r ON ri.recipe_id = r.recipe_id
		      WHERE ri.sub_recipe_id IS NOT NULL
		 )
		 SELECT recipe_id, MIN(depth) FROM reach GROUP BY recipe_id`,
		proposedSubRecipeID,
	)
	if err != nil {
		return nil, fmt.Errorf("inventory: checking recipe reachability: %w", err)
	}
	defer rows.Close()
	out := map[string]int{}
	for rows.Next() {
		var id string
		var depth int
		if err := rows.Scan(&id, &depth); err != nil {
			return nil, fmt.Errorf("inventory: scanning reachable recipe: %w", err)
		}
		out[id] = depth
	}
	return out, rows.Err()
}

func (r *pgRepository) RecipesSince(ctx context.Context, outletID string, sinceVersion int) ([]Recipe, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT r.id, r.menu_item_variant_id, r.name, r.recipe_version, r.output_dimension, r.output_quantity_micro, r.config_version
		 FROM recipe r
		 JOIN menu_item_variant v ON v.id = r.menu_item_variant_id
		 JOIN menu_item mi ON mi.id = v.menu_item_id
		 WHERE mi.outlet_id = $1 AND r.config_version > $2
		 ORDER BY r.config_version`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("inventory: listing recipes since %d: %w", sinceVersion, err)
	}
	defer rows.Close()
	var out []Recipe
	for rows.Next() {
		var rc Recipe
		var dim string
		if err := rows.Scan(&rc.ID, &rc.MenuItemVariantID, &rc.Name, &rc.RecipeVersion, &dim, &rc.OutputQuantityMicro, &rc.ConfigVersion); err != nil {
			return nil, fmt.Errorf("inventory: scanning recipe: %w", err)
		}
		rc.OutputDimension = Dimension(dim)
		rc.SchemaVersion = 1
		out = append(out, rc)
	}
	return out, rows.Err()
}

func (r *pgRepository) RecipeIngredientsSince(ctx context.Context, outletID string, sinceVersion int) ([]RecipeIngredient, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT ri.id, ri.recipe_id, ri.component_kind, ri.inventory_item_id, ri.sub_recipe_id,
		        ri.quantity_micro, ri.quantity_dimension, ri.yield_factor_ppm, ri.sort_order, ri.config_version
		 FROM recipe_ingredient ri
		 JOIN recipe r ON r.id = ri.recipe_id
		 JOIN menu_item_variant v ON v.id = r.menu_item_variant_id
		 JOIN menu_item mi ON mi.id = v.menu_item_id
		 WHERE mi.outlet_id = $1 AND ri.config_version > $2
		 ORDER BY ri.config_version`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("inventory: listing recipe ingredients since %d: %w", sinceVersion, err)
	}
	defer rows.Close()
	var out []RecipeIngredient
	for rows.Next() {
		var ri RecipeIngredient
		var kind, dim string
		if err := rows.Scan(&ri.ID, &ri.RecipeID, &kind, &ri.InventoryItemID, &ri.SubRecipeID,
			&ri.QuantityMicro, &dim, &ri.YieldFactorPPM, &ri.SortOrder, &ri.ConfigVersion); err != nil {
			return nil, fmt.Errorf("inventory: scanning recipe ingredient: %w", err)
		}
		ri.ComponentKind = RecipeComponentKind(kind)
		ri.QuantityDimension = Dimension(dim)
		ri.SchemaVersion = 1
		out = append(out, ri)
	}
	return out, rows.Err()
}

func (r *pgRepository) MenuItemVariantOutlet(ctx context.Context, variantID string) (string, bool, error) {
	var outletID string
	err := r.pool.QueryRow(ctx,
		`SELECT mi.outlet_id FROM menu_item_variant v JOIN menu_item mi ON mi.id = v.menu_item_id WHERE v.id = $1`,
		variantID,
	).Scan(&outletID)
	if errors.Is(err, pgx.ErrNoRows) {
		return "", false, nil
	}
	if err != nil {
		return "", false, fmt.Errorf("inventory: resolving menu item variant outlet: %w", err)
	}
	return outletID, true, nil
}

// --- modifier_ingredient_delta (read-only) --------------------------------

func (r *pgRepository) ModifierIngredientDeltasSince(ctx context.Context, outletID string, sinceVersion int) ([]ModifierIngredientDelta, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT d.id, d.menu_item_modifier_id, d.inventory_item_id, d.quantity_micro, d.config_version
		 FROM modifier_ingredient_delta d
		 JOIN menu_item_modifier m ON m.id = d.menu_item_modifier_id
		 JOIN menu_item mi ON mi.id = m.menu_item_id
		 WHERE mi.outlet_id = $1 AND d.config_version > $2
		 ORDER BY d.config_version`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("inventory: listing modifier ingredient deltas since %d: %w", sinceVersion, err)
	}
	defer rows.Close()
	var out []ModifierIngredientDelta
	for rows.Next() {
		var d ModifierIngredientDelta
		if err := rows.Scan(&d.ID, &d.MenuItemModifierID, &d.InventoryItemID, &d.QuantityMicro, &d.ConfigVersion); err != nil {
			return nil, fmt.Errorf("inventory: scanning modifier ingredient delta: %w", err)
		}
		d.SchemaVersion = 1
		out = append(out, d)
	}
	return out, rows.Err()
}

// --- stock_ledger_entry / stock_deduction_gap -----------------------------

func (r *pgRepository) LastEntrySeq(ctx context.Context, outletID string) (int64, error) {
	var seq int64
	err := r.pool.QueryRow(ctx,
		`SELECT COALESCE(MAX(entry_seq), 0) FROM stock_ledger_entry WHERE outlet_id = $1`,
		outletID,
	).Scan(&seq)
	if err != nil {
		return 0, fmt.Errorf("inventory: getting last entry_seq: %w", err)
	}
	return seq, nil
}

func (r *pgRepository) GetLedgerEntryBySeq(ctx context.Context, outletID string, entrySeq int64) (StockLedgerEntry, bool, error) {
	entry, ok, err := scanLedgerEntryRow(r.pool.QueryRow(ctx, ledgerEntrySelect+` WHERE outlet_id = $1 AND entry_seq = $2`, outletID, entrySeq))
	return entry, ok, err
}

func (r *pgRepository) GetLedgerEntryByID(ctx context.Context, id string) (StockLedgerEntry, bool, error) {
	return scanLedgerEntryRow(r.pool.QueryRow(ctx, ledgerEntrySelect+` WHERE id = $1`, id))
}

const ledgerEntrySelect = `SELECT id, outlet_id, entry_seq, inventory_item_id, inventory_item_name, dimension,
	       entry_type, origin, quantity_applied_micro, recipe_id, recipe_version, recipe_name,
	       modifier_delta_id, modifier_name, modifier_delta_version, source_order_id, source_order_item_id,
	       reason_code, note, occurred_at, business_date, created_by_user_id, unit_cost_paise
	FROM stock_ledger_entry`

type rowScanner interface {
	Scan(dest ...interface{}) error
}

// businessDateLayout mirrors backend/internal/payments/repository.go's
// constant of the same name: how a Postgres DATE column round-trips through
// this milestone's string-typed BusinessDate fields.
const businessDateLayout = "2006-01-02"

func scanLedgerEntryRow(row rowScanner) (StockLedgerEntry, bool, error) {
	var e StockLedgerEntry
	var dim, entryType, origin string
	var occurredAt, businessDate time.Time
	err := row.Scan(&e.ID, &e.OutletID, &e.EntrySeq, &e.InventoryItemID, &e.InventoryItemName, &dim,
		&entryType, &origin, &e.QuantityAppliedMicro, &e.RecipeID, &e.RecipeVersion, &e.RecipeName,
		&e.ModifierDeltaID, &e.ModifierName, &e.ModifierDeltaVersion, &e.SourceOrderID, &e.SourceOrderItemID,
		&e.ReasonCode, &e.Note, &occurredAt, &businessDate, &e.CreatedByUserID, &e.UnitCostPaise)
	if errors.Is(err, pgx.ErrNoRows) {
		return StockLedgerEntry{}, false, nil
	}
	if err != nil {
		return StockLedgerEntry{}, false, fmt.Errorf("inventory: scanning ledger entry: %w", err)
	}
	e.Dimension = Dimension(dim)
	e.EntryType = StockEntryType(entryType)
	e.Origin = StockEntryOrigin(origin)
	e.OccurredAt = occurredAt.UTC().Format(time.RFC3339)
	e.BusinessDate = businessDate.Format(businessDateLayout)
	e.SchemaVersion = 1
	return e, true, nil
}

func (r *pgRepository) InsertLedgerEntry(ctx context.Context, entry StockLedgerEntry) error {
	_, err := r.pool.Exec(ctx,
		`INSERT INTO stock_ledger_entry (
		   id, outlet_id, entry_seq, inventory_item_id, inventory_item_name, dimension,
		   entry_type, origin, quantity_applied_micro, recipe_id, recipe_version, recipe_name,
		   modifier_delta_id, modifier_name, modifier_delta_version, source_order_id, source_order_item_id,
		   reason_code, note, occurred_at, business_date, created_by_user_id, unit_cost_paise
		 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23)`,
		entry.ID, entry.OutletID, entry.EntrySeq, entry.InventoryItemID, entry.InventoryItemName, string(entry.Dimension),
		string(entry.EntryType), string(entry.Origin), entry.QuantityAppliedMicro, entry.RecipeID, entry.RecipeVersion, entry.RecipeName,
		entry.ModifierDeltaID, entry.ModifierName, entry.ModifierDeltaVersion, entry.SourceOrderID, entry.SourceOrderItemID,
		entry.ReasonCode, entry.Note, entry.OccurredAt, entry.BusinessDate, entry.CreatedByUserID, entry.UnitCostPaise,
	)
	if err != nil {
		if isUniqueViolation(err) {
			return fmt.Errorf("%w: ledger entry id or (outlet_id, entry_seq) already exists", httpx.ErrConflict)
		}
		return fmt.Errorf("inventory: inserting ledger entry: %w", err)
	}
	return nil
}

func (r *pgRepository) GetDeductionGapByID(ctx context.Context, id string) (StockDeductionGap, bool, error) {
	var g StockDeductionGap
	var reason string
	var occurredAt, businessDate time.Time
	err := r.pool.QueryRow(ctx,
		`SELECT id, outlet_id, order_id, order_item_id, menu_item_id, menu_item_variant_id, menu_item_name,
		        quantity, reason, occurred_at, business_date
		 FROM stock_deduction_gap WHERE id = $1`,
		id,
	).Scan(&g.ID, &g.OutletID, &g.OrderID, &g.OrderItemID, &g.MenuItemID, &g.MenuItemVariantID, &g.MenuItemName,
		&g.Quantity, &reason, &occurredAt, &businessDate)
	if errors.Is(err, pgx.ErrNoRows) {
		return StockDeductionGap{}, false, nil
	}
	if err != nil {
		return StockDeductionGap{}, false, fmt.Errorf("inventory: getting deduction gap: %w", err)
	}
	g.Reason = StockDeductionGapReason(reason)
	g.OccurredAt = occurredAt.UTC().Format(time.RFC3339)
	g.BusinessDate = businessDate.Format(businessDateLayout)
	g.SchemaVersion = 1
	return g, true, nil
}

func (r *pgRepository) InsertDeductionGap(ctx context.Context, gap StockDeductionGap) error {
	_, err := r.pool.Exec(ctx,
		`INSERT INTO stock_deduction_gap (id, outlet_id, order_id, order_item_id, menu_item_id, menu_item_variant_id, menu_item_name, quantity, reason, occurred_at, business_date)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)`,
		gap.ID, gap.OutletID, gap.OrderID, gap.OrderItemID, gap.MenuItemID, gap.MenuItemVariantID, gap.MenuItemName,
		gap.Quantity, string(gap.Reason), gap.OccurredAt, gap.BusinessDate,
	)
	if err != nil {
		if isUniqueViolation(err) {
			return fmt.Errorf("%w: deduction gap id already exists", httpx.ErrConflict)
		}
		return fmt.Errorf("inventory: inserting deduction gap: %w", err)
	}
	return nil
}

// --- stock_count -----------------------------------------------------------

func (r *pgRepository) GetStockCountByID(ctx context.Context, id string) (StockCount, bool, error) {
	var c StockCount
	var status string
	var businessDate, startedAt time.Time
	var completedAt *time.Time
	err := r.pool.QueryRow(ctx,
		`SELECT id, outlet_id, business_date, status, started_at, completed_at, counted_by_user_id, note
		 FROM stock_count WHERE id = $1`,
		id,
	).Scan(&c.ID, &c.OutletID, &businessDate, &status, &startedAt, &completedAt, &c.CountedByUserID, &c.Note)
	if errors.Is(err, pgx.ErrNoRows) {
		return StockCount{}, false, nil
	}
	if err != nil {
		return StockCount{}, false, fmt.Errorf("inventory: getting stock count: %w", err)
	}
	c.Status = StockCountStatus(status)
	c.BusinessDate = businessDate.Format(businessDateLayout)
	c.StartedAt = startedAt.UTC().Format(time.RFC3339)
	if completedAt != nil {
		formatted := completedAt.UTC().Format(time.RFC3339)
		c.CompletedAt = &formatted
	}
	c.SchemaVersion = 1
	return c, true, nil
}

func (r *pgRepository) InsertStockCount(ctx context.Context, tx pgx.Tx, count StockCount, lines []StockCountLine) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO stock_count (id, outlet_id, business_date, status, started_at, completed_at, counted_by_user_id, note)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
		 ON CONFLICT (id) DO NOTHING`,
		count.ID, count.OutletID, count.BusinessDate, string(count.Status), count.StartedAt, count.CompletedAt, count.CountedByUserID, count.Note,
	)
	if err != nil {
		return fmt.Errorf("inventory: inserting stock count: %w", err)
	}
	for _, l := range lines {
		if _, err := tx.Exec(ctx,
			`INSERT INTO stock_count_line (id, stock_count_id, inventory_item_id, inventory_item_name, dimension, counted_quantity_micro, expected_quantity_micro, note)
			 VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
			 ON CONFLICT (id) DO NOTHING`,
			l.ID, count.ID, l.InventoryItemID, l.InventoryItemName, string(l.Dimension), l.CountedQuantityMicro, l.ExpectedQuantityMicro, l.Note,
		); err != nil {
			return fmt.Errorf("inventory: inserting stock count line: %w", err)
		}
	}
	return nil
}

func (r *pgRepository) GetStockCountLines(ctx context.Context, countID string) ([]StockCountLine, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, stock_count_id, inventory_item_id, inventory_item_name, dimension, counted_quantity_micro, expected_quantity_micro, note
		 FROM stock_count_line WHERE stock_count_id = $1`,
		countID,
	)
	if err != nil {
		return nil, fmt.Errorf("inventory: listing stock count lines: %w", err)
	}
	defer rows.Close()
	var out []StockCountLine
	for rows.Next() {
		var l StockCountLine
		var dim string
		if err := rows.Scan(&l.ID, &l.StockCountID, &l.InventoryItemID, &l.InventoryItemName, &dim, &l.CountedQuantityMicro, &l.ExpectedQuantityMicro, &l.Note); err != nil {
			return nil, fmt.Errorf("inventory: scanning stock count line: %w", err)
		}
		l.Dimension = Dimension(dim)
		l.SchemaVersion = 1
		out = append(out, l)
	}
	return out, rows.Err()
}
