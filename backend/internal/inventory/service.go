package inventory

import (
	"context"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

// Service implements the inventory business commands: the config write
// routes (inventory_item, recipe — including the recipe cycle guard) and
// the envelope-wrapped edge->cloud replay routes (stock_ledger_entry /
// stock_deduction_gap sharing one route per ADR-018 §10.1, and stock_count).
type Service struct {
	repo Repository
}

func NewService(repo Repository) *Service {
	return &Service{repo: repo}
}

// now is the observation timestamp for replay-gap bookkeeping. A method so
// tests can pin it without threading a clock through every ingest signature;
// the values it stamps are diagnostics, never business time (business time
// always comes from the edge, on the row).
func (s *Service) now() time.Time {
	return time.Now().UTC()
}

func (s *Service) requireOutletInTenant(ctx context.Context, tenantID, outletID string) error {
	if strings.TrimSpace(tenantID) == "" {
		return httpx.ErrUnauthorized
	}
	outletID = strings.TrimSpace(outletID)
	if outletID == "" {
		return fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	ok, err := s.repo.OutletBelongsToTenant(ctx, tenantID, outletID)
	if err != nil {
		return err
	}
	if !ok {
		return httpx.ErrForbidden
	}
	return nil
}

// --- inventory_item ------------------------------------------------------

// CreateInventoryItem creates or updates a raw material (config, cloud→edge).
// Current stock and cost are never accepted here — ADR-018 §1 forbids them
// on this row structurally, and this input type simply has no field for
// either.
func (s *Service) CreateInventoryItem(ctx context.Context, tenantID string, in NewInventoryItemInput) (InventoryItem, []ItemUnitConversion, error) {
	if err := s.requireOutletInTenant(ctx, tenantID, in.OutletID); err != nil {
		return InventoryItem{}, nil, err
	}
	if strings.TrimSpace(in.ID) == "" {
		return InventoryItem{}, nil, fmt.Errorf("%w: id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.SKU) == "" {
		return InventoryItem{}, nil, fmt.Errorf("%w: sku is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.Name) == "" {
		return InventoryItem{}, nil, fmt.Errorf("%w: name is required", httpx.ErrInvalidInput)
	}
	if !validDimension(in.Dimension) {
		return InventoryItem{}, nil, fmt.Errorf("%w: dimension must be one of MASS, VOLUME, COUNT", httpx.ErrInvalidInput)
	}
	conversions := make([]ItemUnitConversion, 0, len(in.Conversions))
	for _, c := range in.Conversions {
		if strings.TrimSpace(c.ID) == "" || strings.TrimSpace(c.PackUnitLabel) == "" {
			return InventoryItem{}, nil, fmt.Errorf("%w: item_unit_conversion id and pack_unit_label are required", httpx.ErrInvalidInput)
		}
		if !validDimension(c.SourceDimension) {
			return InventoryItem{}, nil, fmt.Errorf("%w: item_unit_conversion source_dimension must be one of MASS, VOLUME, COUNT", httpx.ErrInvalidInput)
		}
		if c.Numerator <= 0 || c.Denominator <= 0 {
			return InventoryItem{}, nil, fmt.Errorf("%w: item_unit_conversion numerator/denominator must be positive", httpx.ErrInvalidInput)
		}
		if isTier1Label(c.PackUnitLabel) {
			return InventoryItem{}, nil, fmt.Errorf("%w: pack_unit_label %q collides with a frozen Tier-1 unit", httpx.ErrInvalidInput, c.PackUnitLabel)
		}
		conversions = append(conversions, ItemUnitConversion{
			ID: c.ID, InventoryItemID: in.ID, PackUnitLabel: c.PackUnitLabel,
			SourceDimension: c.SourceDimension, Numerator: c.Numerator, Denominator: c.Denominator,
			SchemaVersion: 1,
		})
	}

	item := InventoryItem{
		ID: in.ID, OutletID: in.OutletID, SKU: in.SKU, Name: in.Name, Category: in.Category,
		Dimension: in.Dimension, ReorderLevelMicro: in.ReorderLevelMicro, ParLevelMicro: in.ParLevelMicro,
		StorageLocation: in.StorageLocation, IsActive: in.IsActive,
		YieldFactorPPM: YieldFactorPPMIdentity, SchemaVersion: 1,
	}
	err := s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, in.OutletID)
		if err != nil {
			return err
		}
		item.ConfigVersion = newVersion
		for i := range conversions {
			conversions[i].ConfigVersion = newVersion
		}
		return s.repo.UpsertInventoryItem(ctx, tx, item, conversions)
	})
	if err != nil {
		return InventoryItem{}, nil, err
	}
	return item, conversions, nil
}

func validDimension(d Dimension) bool {
	switch d {
	case DimensionMass, DimensionVolume, DimensionCount:
		return true
	default:
		return false
	}
}

// isTier1Label mirrors the CHECK in packages/contracts/postgres/0015 and
// packages/contracts/sqlite/0015: a pack label may never collide with a
// Tier-1 dimensional unit, checked case-insensitively.
func isTier1Label(label string) bool {
	_, isFrozen := contracts.DimensionalConversions[strings.ToLower(label)]
	if isFrozen {
		return true
	}
	switch strings.ToLower(label) {
	case "litre", "liter", "pieces", "pc":
		return true
	default:
		return false
	}
}

// --- recipe ----------------------------------------------------------------

// CreateRecipe creates or updates a recipe and its ingredient list in one
// transaction (config, cloud→edge). THIS METHOD IS THE CYCLE GUARD
// (ADR-018 §7, task instruction): before accepting any SUB_RECIPE
// ingredient it runs the recursive-CTE reachability check pinned in
// packages/contracts/sqlite/0015_m4_inventory_config.sql's header and
// rejects a write that would let the parent recipe reach itself, naming the
// offending path; depth is bounded at MaxRecipeDepth in the same pass. It
// also rejects a quantity_dimension that disagrees with its referent's own
// dimension (422) — a recipe is not an inventory item, so nothing converts.
func (s *Service) CreateRecipe(ctx context.Context, tenantID string, in NewRecipeInput) (Recipe, []RecipeIngredient, error) {
	if strings.TrimSpace(in.ID) == "" {
		return Recipe{}, nil, fmt.Errorf("%w: id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.MenuItemVariantID) == "" {
		return Recipe{}, nil, fmt.Errorf("%w: menu_item_variant_id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.Name) == "" {
		return Recipe{}, nil, fmt.Errorf("%w: name is required", httpx.ErrInvalidInput)
	}
	if !validDimension(in.OutputDimension) {
		return Recipe{}, nil, fmt.Errorf("%w: output_dimension must be one of MASS, VOLUME, COUNT", httpx.ErrInvalidInput)
	}
	if in.OutputQuantityMicro <= 0 {
		return Recipe{}, nil, fmt.Errorf("%w: output_quantity_micro must be positive", httpx.ErrInvalidInput)
	}

	outletID, ok, err := s.repo.MenuItemVariantOutlet(ctx, in.MenuItemVariantID)
	if err != nil {
		return Recipe{}, nil, err
	}
	if !ok {
		return Recipe{}, nil, fmt.Errorf("%w: menu_item_variant %s does not exist", httpx.ErrInvalidInput, in.MenuItemVariantID)
	}
	if err := s.requireOutletInTenant(ctx, tenantID, outletID); err != nil {
		return Recipe{}, nil, err
	}

	existingID, nextVersion, exists, err := s.repo.RecipeVersionForVariant(ctx, in.MenuItemVariantID)
	if err != nil {
		return Recipe{}, nil, err
	}
	if exists && existingID != in.ID {
		return Recipe{}, nil, fmt.Errorf("%w: menu_item_variant_id %s is already bound to a different recipe", httpx.ErrConflict, in.MenuItemVariantID)
	}

	ingredients := make([]RecipeIngredient, 0, len(in.Ingredients))
	for _, ingIn := range in.Ingredients {
		if strings.TrimSpace(ingIn.ID) == "" {
			return Recipe{}, nil, fmt.Errorf("%w: recipe_ingredient id is required", httpx.ErrInvalidInput)
		}
		if !validDimension(ingIn.QuantityDimension) {
			return Recipe{}, nil, fmt.Errorf("%w: recipe_ingredient quantity_dimension must be one of MASS, VOLUME, COUNT", httpx.ErrInvalidInput)
		}
		if ingIn.QuantityMicro <= 0 {
			return Recipe{}, nil, fmt.Errorf("%w: recipe_ingredient quantity_micro must be positive", httpx.ErrInvalidInput)
		}

		switch ingIn.ComponentKind {
		case RecipeComponentKindItem:
			if ingIn.InventoryItemID == nil || ingIn.SubRecipeID != nil {
				return Recipe{}, nil, fmt.Errorf("%w: ITEM ingredient must set inventory_item_id and not sub_recipe_id", httpx.ErrInvalidInput)
			}
			dim, found, err := s.repo.InventoryItemDimension(ctx, *ingIn.InventoryItemID)
			if err != nil {
				return Recipe{}, nil, err
			}
			if !found {
				return Recipe{}, nil, fmt.Errorf("%w: recipe_ingredient references inventory_item %s which does not exist", httpx.ErrInvalidInput, *ingIn.InventoryItemID)
			}
			if dim != ingIn.QuantityDimension {
				return Recipe{}, nil, fmt.Errorf("%w: quantity_dimension %q but inventory_item is measured in %q", ErrDimensionMismatch, ingIn.QuantityDimension, dim)
			}

		case RecipeComponentKindSubRecipe:
			if ingIn.SubRecipeID == nil || ingIn.InventoryItemID != nil {
				return Recipe{}, nil, fmt.Errorf("%w: SUB_RECIPE ingredient must set sub_recipe_id and not inventory_item_id", httpx.ErrInvalidInput)
			}
			if *ingIn.SubRecipeID == in.ID {
				return Recipe{}, nil, fmt.Errorf("%w: recipe %s cannot reference itself", ErrRecipeCycle, in.ID)
			}
			outDim, found, err := s.repo.RecipeOutputDimension(ctx, *ingIn.SubRecipeID)
			if err != nil {
				return Recipe{}, nil, err
			}
			if !found {
				return Recipe{}, nil, fmt.Errorf("%w: recipe_ingredient references recipe %s which does not exist", httpx.ErrInvalidInput, *ingIn.SubRecipeID)
			}
			if outDim != ingIn.QuantityDimension {
				return Recipe{}, nil, fmt.Errorf("%w: quantity_dimension %q but sub-recipe yields %q", ErrDimensionMismatch, ingIn.QuantityDimension, outDim)
			}

			// THE CYCLE GUARD. Reachability from the proposed child; a
			// parent that appears in its own reachable set would cycle.
			reachable, err := s.repo.ReachableSubRecipes(ctx, *ingIn.SubRecipeID)
			if err != nil {
				return Recipe{}, nil, err
			}
			if depth, cycles := reachable[in.ID]; cycles {
				return Recipe{}, nil, fmt.Errorf("%w: recipe %s is reachable from sub_recipe %s at depth %d", ErrRecipeCycle, in.ID, *ingIn.SubRecipeID, depth)
			}
			if maxDepth := maxReachableDepth(reachable); maxDepth > MaxRecipeDepth {
				return Recipe{}, nil, fmt.Errorf("%w: sub_recipe %s nests %d levels deep, exceeds %d", ErrRecipeDepthExceeded, *ingIn.SubRecipeID, maxDepth, MaxRecipeDepth)
			}

		default:
			return Recipe{}, nil, fmt.Errorf("%w: component_kind must be ITEM or SUB_RECIPE", httpx.ErrInvalidInput)
		}

		ingredients = append(ingredients, RecipeIngredient{
			ID: ingIn.ID, RecipeID: in.ID, ComponentKind: ingIn.ComponentKind,
			InventoryItemID: ingIn.InventoryItemID, SubRecipeID: ingIn.SubRecipeID,
			QuantityMicro: ingIn.QuantityMicro, QuantityDimension: ingIn.QuantityDimension,
			YieldFactorPPM: YieldFactorPPMIdentity, SortOrder: ingIn.SortOrder, SchemaVersion: 1,
		})
	}

	recipe := Recipe{
		ID: in.ID, MenuItemVariantID: in.MenuItemVariantID, Name: in.Name,
		RecipeVersion: nextVersion, OutputDimension: in.OutputDimension,
		OutputQuantityMicro: in.OutputQuantityMicro, SchemaVersion: 1,
	}
	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, outletID)
		if err != nil {
			return err
		}
		recipe.ConfigVersion = newVersion
		for i := range ingredients {
			ingredients[i].ConfigVersion = newVersion
		}
		return s.repo.UpsertRecipe(ctx, tx, recipe, ingredients)
	})
	if err != nil {
		return Recipe{}, nil, err
	}
	return recipe, ingredients, nil
}

// maxReachableDepth returns the deepest nesting level in reachable, or 0 if
// empty — the depth-bound half of the cycle guard's single recursive-CTE
// pass (ADR-018 §7).
func maxReachableDepth(reachable map[string]int) int {
	max := 0
	for _, d := range reachable {
		if d > max {
			max = d
		}
	}
	return max
}

// --- stock_ledger_entry / stock_deduction_gap -----------------------------

// IngestLedgerEntry replays an edge-recorded stock movement. Append-only:
// the DB trigger is the backstop, but this path never issues an UPDATE.
// entry_seq contiguity is enforced against the outlet's high-water mark
// (ADR-018 replay addendum) — a gap is a loud, structured error, never a
// silent skip.
func (s *Service) IngestLedgerEntry(ctx context.Context, callerTenantID string, env contracts.SyncEnvelope, entry StockLedgerEntry) (StockLedgerEntry, error) {
	if err := requireEnvelope(env, contracts.AggregateTypeStockLedgerEntry); err != nil {
		return StockLedgerEntry{}, err
	}
	if err := requireTenantMatch(callerTenantID, env); err != nil {
		return StockLedgerEntry{}, err
	}
	if strings.TrimSpace(entry.ID) == "" {
		return StockLedgerEntry{}, fmt.Errorf("%w: id is required", httpx.ErrInvalidInput)
	}
	if entry.ID != env.RecordID {
		return StockLedgerEntry{}, fmt.Errorf("%w: payload id must match envelope record_id", httpx.ErrInvalidInput)
	}
	if entry.OutletID != env.OutletID {
		return StockLedgerEntry{}, fmt.Errorf("%w: payload outlet_id must match envelope outlet_id", httpx.ErrInvalidInput)
	}
	if entry.EntrySeq < 1 {
		return StockLedgerEntry{}, fmt.Errorf("%w: entry_seq must be >= 1", httpx.ErrInvalidInput)
	}

	// SILENT AND IDEMPOTENT, and BEFORE the contiguity check. The same id
	// arriving twice is an ordinary retry (a dropped ack, a resumed batch),
	// not a fault: the mark it carries is already stored, so reaching the
	// contiguity check would read it as a reused mark and refuse a request
	// that is merely a repeat — a false alarm on every reconnect.
	if existing, found, err := s.repo.GetLedgerEntryByID(ctx, entry.ID); err != nil {
		return StockLedgerEntry{}, err
	} else if found {
		return existing, nil
	}

	if err := s.checkContiguity(ctx, entry.OutletID, ReplayStreamLedger, entry.EntrySeq); err != nil {
		return StockLedgerEntry{}, err
	}

	if err := s.repo.InsertLedgerEntry(ctx, entry); err != nil {
		return StockLedgerEntry{}, err
	}
	// The entry just stored may be the one that completes an earlier hole.
	// Resolving here rather than on a sweep keeps the table meaning "still
	// missing" at every moment a human might read it.
	if err := s.repo.ResolveCoveredReplayGaps(ctx, entry.OutletID, ReplayStreamLedger, s.now()); err != nil {
		return StockLedgerEntry{}, err
	}
	return entry, nil
}

// checkContiguity compares an arriving mark against the stream's high-water
// mark, and is the shared implementation for both ranged streams.
//
// A HOLE IS RECORDED, NOT REJECTED — the condition this function exists to
// get right. Rejecting an entry whose mark is beyond the cursor turns one
// lost row into a permanent outage: replay halts there, every later entry
// stays at the outlet, and the failure is silent because "no ledger activity"
// and "replay wedged since Tuesday" look identical downstream. Detection is
// the goal; blocking was a side effect nobody wanted. So the hole is written
// to ledger_replay_gap and the arriving entry is accepted.
//
// The one refusal left is a mark at or below the cursor under a different id,
// which would make the mark ambiguous. That is unreachable through the edge's
// own path (contracts 0.5.3 made entry_seq a durable counter for exactly this
// reason) and clearing it is a documented manual operation — see
// ErrLedgerSequenceMarkReused.
func (s *Service) checkContiguity(ctx context.Context, outletID string, stream ReplayStream, entrySeq int64) error {
	var cursor int64
	var err error
	switch stream {
	case ReplayStreamLedger:
		cursor, err = s.repo.LastEntrySeq(ctx, outletID)
	case ReplayStreamDeductionGap:
		cursor, err = s.repo.LastGapEntrySeq(ctx, outletID)
	default:
		return fmt.Errorf("inventory: unknown replay stream %q", stream)
	}
	if err != nil {
		return err
	}

	switch {
	case entrySeq <= cursor:
		return fmt.Errorf("%w: %s entry_seq %d is not greater than outlet %s's high-water mark %d",
			ErrLedgerSequenceMarkReused, stream, entrySeq, outletID, cursor)

	case entrySeq > cursor+1:
		// Loud and durable, and the stream keeps moving.
		return s.repo.RecordReplayGap(ctx, outletID, stream, cursor+1, entrySeq-1, s.now())
	}
	return nil
}

// IngestDeductionGap replays a stock_deduction_gap signal — never a
// correction (ADR-018 §10.1). Sharing the ledger ingest route rather than
// taking one of its own.
func (s *Service) IngestDeductionGap(ctx context.Context, callerTenantID string, env contracts.SyncEnvelope, gap StockDeductionGap) (StockDeductionGap, error) {
	if err := requireEnvelope(env, contracts.AggregateTypeStockDeductionGap); err != nil {
		return StockDeductionGap{}, err
	}
	if err := requireTenantMatch(callerTenantID, env); err != nil {
		return StockDeductionGap{}, err
	}
	if strings.TrimSpace(gap.ID) == "" {
		return StockDeductionGap{}, fmt.Errorf("%w: id is required", httpx.ErrInvalidInput)
	}
	if gap.ID != env.RecordID {
		return StockDeductionGap{}, fmt.Errorf("%w: payload id must match envelope record_id", httpx.ErrInvalidInput)
	}
	if gap.OutletID != env.OutletID {
		return StockDeductionGap{}, fmt.Errorf("%w: payload outlet_id must match envelope outlet_id", httpx.ErrInvalidInput)
	}
	if gap.Quantity <= 0 {
		return StockDeductionGap{}, fmt.Errorf("%w: quantity must be positive", httpx.ErrInvalidInput)
	}
	if gap.EntrySeq < 1 {
		return StockDeductionGap{}, fmt.Errorf("%w: entry_seq must be >= 1", httpx.ErrInvalidInput)
	}

	// SILENT AND IDEMPOTENT, deliberately, and BEFORE the contiguity check.
	// The same id arriving twice is an ordinary retry — a dropped ack, a
	// resumed batch. Treating it as anything louder would make every
	// reconnect produce a false alarm, and the mark it carries is one the
	// cloud has already stored, so the contiguity check below would read it
	// as a reused mark and refuse a request that is simply a repeat.
	if existing, found, err := s.repo.GetDeductionGapByID(ctx, gap.ID); err != nil {
		return StockDeductionGap{}, err
	} else if found {
		return existing, nil
	}

	// The gap stream ranges over its OWN counter and so has its own
	// high-water mark: a gap row is the signal that a sale went unaccounted,
	// and a signal lost in transit must be as visible as a lost movement.
	if err := s.checkContiguity(ctx, gap.OutletID, ReplayStreamDeductionGap, gap.EntrySeq); err != nil {
		return StockDeductionGap{}, err
	}

	if err := s.repo.InsertDeductionGap(ctx, gap); err != nil {
		return StockDeductionGap{}, err
	}
	if err := s.repo.ResolveCoveredReplayGaps(ctx, gap.OutletID, ReplayStreamDeductionGap, s.now()); err != nil {
		return StockDeductionGap{}, err
	}
	return gap, nil
}

// --- stock_count -----------------------------------------------------------

// IngestStockCount replays a completed (or in-progress) physical stock
// count and its lines in one payload (ADR-018 §10, the invoice_line
// precedent). Idempotent on the count's id.
func (s *Service) IngestStockCount(ctx context.Context, callerTenantID string, env contracts.SyncEnvelope, replay StockCountReplay) (StockCountReplay, error) {
	if err := requireEnvelope(env, contracts.AggregateTypeStockCount); err != nil {
		return StockCountReplay{}, err
	}
	if err := requireTenantMatch(callerTenantID, env); err != nil {
		return StockCountReplay{}, err
	}
	count := replay.Count
	if strings.TrimSpace(count.ID) == "" {
		return StockCountReplay{}, fmt.Errorf("%w: id is required", httpx.ErrInvalidInput)
	}
	if count.ID != env.RecordID {
		return StockCountReplay{}, fmt.Errorf("%w: payload id must match envelope record_id", httpx.ErrInvalidInput)
	}
	if count.OutletID != env.OutletID {
		return StockCountReplay{}, fmt.Errorf("%w: payload outlet_id must match envelope outlet_id", httpx.ErrInvalidInput)
	}

	if existing, found, err := s.repo.GetStockCountByID(ctx, count.ID); err != nil {
		return StockCountReplay{}, err
	} else if found {
		lines, err := s.repo.GetStockCountLines(ctx, existing.ID)
		if err != nil {
			return StockCountReplay{}, err
		}
		return StockCountReplay{Count: existing, Lines: emptyIfNilLines(lines)}, nil
	}

	err := s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		return s.repo.InsertStockCount(ctx, tx, count, replay.Lines)
	})
	if err != nil {
		return StockCountReplay{}, err
	}
	return replay, nil
}

func emptyIfNilLines(l []StockCountLine) []StockCountLine {
	if l == nil {
		return []StockCountLine{}
	}
	return l
}

// --- Sync config bundle ------------------------------------------------

// SyncConfigBundle returns the inventory context's contribution to
// GET /sync/config: inventory items, item unit conversions, recipes, recipe
// ingredients and modifier ingredient deltas newer than sinceVersion.
func (s *Service) SyncConfigBundle(ctx context.Context, tenantID, outletID string, sinceVersion int) (ConfigBundle, error) {
	if err := s.requireOutletInTenant(ctx, tenantID, outletID); err != nil {
		return ConfigBundle{}, err
	}

	items, err := s.repo.InventoryItemsSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	conversions, err := s.repo.ItemUnitConversionsSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	recipes, err := s.repo.RecipesSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	recipeIngredients, err := s.repo.RecipeIngredientsSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	deltas, err := s.repo.ModifierIngredientDeltasSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}

	sort.Slice(items, func(i, j int) bool { return items[i].ConfigVersion < items[j].ConfigVersion })

	return ConfigBundle{
		InventoryItems:           emptyIfNilItems(items),
		ItemUnitConversions:      emptyIfNilConversions(conversions),
		Recipes:                  emptyIfNilRecipes(recipes),
		RecipeIngredients:        emptyIfNilIngredients(recipeIngredients),
		ModifierIngredientDeltas: emptyIfNilDeltas(deltas),
	}, nil
}

func emptyIfNilItems(s []InventoryItem) []InventoryItem {
	if s == nil {
		return []InventoryItem{}
	}
	return s
}

func emptyIfNilConversions(s []ItemUnitConversion) []ItemUnitConversion {
	if s == nil {
		return []ItemUnitConversion{}
	}
	return s
}

func emptyIfNilRecipes(s []Recipe) []Recipe {
	if s == nil {
		return []Recipe{}
	}
	return s
}

func emptyIfNilIngredients(s []RecipeIngredient) []RecipeIngredient {
	if s == nil {
		return []RecipeIngredient{}
	}
	return s
}

func emptyIfNilDeltas(s []ModifierIngredientDelta) []ModifierIngredientDelta {
	if s == nil {
		return []ModifierIngredientDelta{}
	}
	return s
}
