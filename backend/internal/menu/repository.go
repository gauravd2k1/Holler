package menu

import (
	"context"
	"errors"
	"fmt"

	"github.com/jackc/pgx/v5"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/postgres"
)

// Repository is the persistence boundary for the menu context. The service
// layer depends on this interface, not on pgx directly, so pricing and
// versioning logic can be unit tested without a database.
type Repository interface {
	// BumpOutletConfigVersion increments outlet.config_version by exactly one
	// and returns the new value. Callers use the returned value to stamp every
	// row touched by the same logical write.
	BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error)

	WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error

	ListCategories(ctx context.Context, outletID string) ([]Category, error)
	InsertCategory(ctx context.Context, tx pgx.Tx, c Category) error

	ListItems(ctx context.Context, outletID string) ([]Item, error)
	InsertItem(ctx context.Context, tx pgx.Tx, i Item) error
	GetItem(ctx context.Context, outletID, itemID string) (Item, error)
	UpdateItemAvailability(ctx context.Context, tx pgx.Tx, itemID string, isAvailable bool, configVersion int) error

	CategoryExists(ctx context.Context, outletID, categoryID string) (bool, error)

	InsertVariant(ctx context.Context, tx pgx.Tx, v Variant) error
	InsertModifier(ctx context.Context, tx pgx.Tx, m Modifier) error

	// ListVariantsSince and ListModifiersSince are this context's
	// since_version-filtered sync exports, the shape
	// backend/internal/kitchen exports station_printers with
	// (StationPrintersSince): joined through menu_item to outletID,
	// filtered at the DB rather than in the caller (M4 T4 delivery-fix
	// follow-up — variants and modifiers never reached GET /sync/config
	// before this).
	ListVariantsSince(ctx context.Context, outletID string, sinceVersion int) ([]Variant, error)
	ListModifiersSince(ctx context.Context, outletID string, sinceVersion int) ([]Modifier, error)
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
		return fmt.Errorf("menu: begin tx: %w", err)
	}
	if err := fn(tx); err != nil {
		_ = tx.Rollback(ctx)
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("menu: commit tx: %w", err)
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
		return 0, fmt.Errorf("menu: bumping outlet config_version: %w", err)
	}
	return newVersion, nil
}

func (r *pgRepository) ListCategories(ctx context.Context, outletID string) ([]Category, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, outlet_id, name, sort_order, config_version
		 FROM menu_category WHERE outlet_id = $1 ORDER BY sort_order, name`,
		outletID,
	)
	if err != nil {
		return nil, fmt.Errorf("menu: listing categories: %w", err)
	}
	defer rows.Close()

	var out []Category
	for rows.Next() {
		var c Category
		if err := rows.Scan(&c.ID, &c.OutletID, &c.Name, &c.SortOrder, &c.ConfigVersion); err != nil {
			return nil, fmt.Errorf("menu: scanning category: %w", err)
		}
		out = append(out, c)
	}
	return out, rows.Err()
}

func (r *pgRepository) InsertCategory(ctx context.Context, tx pgx.Tx, c Category) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO menu_category (id, outlet_id, name, sort_order, config_version)
		 VALUES ($1, $2, $3, $4, $5)`,
		c.ID, c.OutletID, c.Name, c.SortOrder, c.ConfigVersion,
	)
	if err != nil {
		return fmt.Errorf("menu: inserting category: %w", err)
	}
	return nil
}

func (r *pgRepository) CategoryExists(ctx context.Context, outletID, categoryID string) (bool, error) {
	var exists bool
	err := r.pool.QueryRow(ctx,
		`SELECT EXISTS(SELECT 1 FROM menu_category WHERE id = $1 AND outlet_id = $2)`,
		categoryID, outletID,
	).Scan(&exists)
	if err != nil {
		return false, fmt.Errorf("menu: checking category existence: %w", err)
	}
	return exists, nil
}

func (r *pgRepository) ListItems(ctx context.Context, outletID string) ([]Item, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, outlet_id, category_id, name, base_price_paise, is_available, tax_profile_id, hsn_sac, config_version
		 FROM menu_item WHERE outlet_id = $1 ORDER BY name`,
		outletID,
	)
	if err != nil {
		return nil, fmt.Errorf("menu: listing items: %w", err)
	}
	defer rows.Close()

	var out []Item
	for rows.Next() {
		var i Item
		if err := rows.Scan(&i.ID, &i.OutletID, &i.CategoryID, &i.Name, &i.BasePricePaise, &i.IsAvailable, &i.TaxProfileID, &i.HSNSAC, &i.ConfigVersion); err != nil {
			return nil, fmt.Errorf("menu: scanning item: %w", err)
		}
		out = append(out, i)
	}
	return out, rows.Err()
}

func (r *pgRepository) InsertItem(ctx context.Context, tx pgx.Tx, i Item) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO menu_item (id, outlet_id, category_id, name, base_price_paise, is_available, tax_profile_id, hsn_sac, config_version)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)`,
		i.ID, i.OutletID, i.CategoryID, i.Name, i.BasePricePaise, i.IsAvailable, i.TaxProfileID, i.HSNSAC, i.ConfigVersion,
	)
	if err != nil {
		return fmt.Errorf("menu: inserting item: %w", err)
	}
	return nil
}

func (r *pgRepository) GetItem(ctx context.Context, outletID, itemID string) (Item, error) {
	var i Item
	err := r.pool.QueryRow(ctx,
		`SELECT id, outlet_id, category_id, name, base_price_paise, is_available, tax_profile_id, hsn_sac, config_version
		 FROM menu_item WHERE id = $1 AND outlet_id = $2`,
		itemID, outletID,
	).Scan(&i.ID, &i.OutletID, &i.CategoryID, &i.Name, &i.BasePricePaise, &i.IsAvailable, &i.TaxProfileID, &i.HSNSAC, &i.ConfigVersion)
	if errors.Is(err, pgx.ErrNoRows) {
		return Item{}, fmt.Errorf("%w: menu item %s", httpx.ErrNotFound, itemID)
	}
	if err != nil {
		return Item{}, fmt.Errorf("menu: getting item: %w", err)
	}
	return i, nil
}

func (r *pgRepository) UpdateItemAvailability(ctx context.Context, tx pgx.Tx, itemID string, isAvailable bool, configVersion int) error {
	tag, err := tx.Exec(ctx,
		`UPDATE menu_item SET is_available = $1, config_version = $2 WHERE id = $3`,
		isAvailable, configVersion, itemID,
	)
	if err != nil {
		return fmt.Errorf("menu: updating item availability: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("%w: menu item %s", httpx.ErrNotFound, itemID)
	}
	return nil
}

func (r *pgRepository) InsertVariant(ctx context.Context, tx pgx.Tx, v Variant) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO menu_item_variant (id, menu_item_id, name, price_delta_paise, is_default, config_version)
		 VALUES ($1, $2, $3, $4, $5, $6)`,
		v.ID, v.MenuItemID, v.Name, v.PriceDeltaPaise, v.IsDefault, v.ConfigVersion,
	)
	if err != nil {
		return fmt.Errorf("menu: inserting variant: %w", err)
	}
	return nil
}

func (r *pgRepository) InsertModifier(ctx context.Context, tx pgx.Tx, m Modifier) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO menu_item_modifier
		 (id, menu_item_id, group_name, option_name, price_delta_paise, min_selection, max_selection, config_version)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)`,
		m.ID, m.MenuItemID, m.GroupName, m.OptionName, m.PriceDeltaPaise, m.MinSelection, m.MaxSelection, m.ConfigVersion,
	)
	if err != nil {
		return fmt.Errorf("menu: inserting modifier: %w", err)
	}
	return nil
}

func (r *pgRepository) ListVariantsSince(ctx context.Context, outletID string, sinceVersion int) ([]Variant, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT v.id, v.menu_item_id, v.name, v.price_delta_paise, v.is_default, v.config_version
		 FROM menu_item_variant v
		 JOIN menu_item mi ON mi.id = v.menu_item_id
		 WHERE mi.outlet_id = $1 AND v.config_version > $2
		 ORDER BY v.config_version`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("menu: listing variants since %d: %w", sinceVersion, err)
	}
	defer rows.Close()

	var out []Variant
	for rows.Next() {
		var v Variant
		if err := rows.Scan(&v.ID, &v.MenuItemID, &v.Name, &v.PriceDeltaPaise, &v.IsDefault, &v.ConfigVersion); err != nil {
			return nil, fmt.Errorf("menu: scanning variant: %w", err)
		}
		out = append(out, v)
	}
	return out, rows.Err()
}

func (r *pgRepository) ListModifiersSince(ctx context.Context, outletID string, sinceVersion int) ([]Modifier, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT m.id, m.menu_item_id, m.group_name, m.option_name, m.price_delta_paise, m.min_selection, m.max_selection, m.config_version
		 FROM menu_item_modifier m
		 JOIN menu_item mi ON mi.id = m.menu_item_id
		 WHERE mi.outlet_id = $1 AND m.config_version > $2
		 ORDER BY m.config_version`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("menu: listing modifiers since %d: %w", sinceVersion, err)
	}
	defer rows.Close()

	var out []Modifier
	for rows.Next() {
		var m Modifier
		if err := rows.Scan(&m.ID, &m.MenuItemID, &m.GroupName, &m.OptionName, &m.PriceDeltaPaise, &m.MinSelection, &m.MaxSelection, &m.ConfigVersion); err != nil {
			return nil, fmt.Errorf("menu: scanning modifier: %w", err)
		}
		out = append(out, m)
	}
	return out, rows.Err()
}
