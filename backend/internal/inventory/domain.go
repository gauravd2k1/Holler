// Package inventory implements the Milestone 4 inventory and recipe bounded
// context (ADR-018, docs/spec/inventory.md): raw materials, unit
// conversions, recipes and sub-recipes, modifier-driven ingredient deltas,
// the append-only stock ledger, stock counts and the deduction-gap signal.
//
// Per docs/spec/sync.md §50.1 (ADR-009), restated by ADR-018 §1:
//
//	inventory_item, recipe                       CLOUD_TO_EDGE aggregates
//	stock_ledger_entry, stock_count,
//	stock_deduction_gap                          EDGE_TO_CLOUD aggregates
//	item_unit_conversion, recipe_ingredient,
//	modifier_ingredient_delta, stock_count_line  child rows, no direction
//
// This package never touches stock_balance_snapshot: it is edge-local
// (SQLite only, no Postgres mirror, no AggregateType, the invoice_sequence
// precedent) and never crosses the cloud boundary.
package inventory

import (
	contracts "github.com/holler/contracts"
)

// Wire/domain shapes are the contract types, aliased rather than duplicated
// (CLAUDE.md: import contract types, never hand-roll mirrors).
type (
	Dimension               = contracts.Dimension
	InventoryItem           = contracts.InventoryItem
	ItemUnitConversion      = contracts.ItemUnitConversion
	Recipe                  = contracts.Recipe
	RecipeComponentKind     = contracts.RecipeComponentKind
	RecipeIngredient        = contracts.RecipeIngredient
	ModifierIngredientDelta = contracts.ModifierIngredientDelta
	StockLedgerEntry        = contracts.StockLedgerEntry
	StockEntryType          = contracts.StockEntryType
	StockEntryOrigin        = contracts.StockEntryOrigin
	StockCount              = contracts.StockCount
	StockCountStatus        = contracts.StockCountStatus
	StockCountLine          = contracts.StockCountLine
	StockDeductionGap       = contracts.StockDeductionGap
	StockDeductionGapReason = contracts.StockDeductionGapReason
)

const (
	DimensionMass   = contracts.DimensionMass
	DimensionVolume = contracts.DimensionVolume
	DimensionCount  = contracts.DimensionCount

	RecipeComponentKindItem      = contracts.RecipeComponentKindItem
	RecipeComponentKindSubRecipe = contracts.RecipeComponentKindSubRecipe

	StockCountStatusOpen      = contracts.StockCountStatusOpen
	StockCountStatusCompleted = contracts.StockCountStatusCompleted

	// MaxRecipeDepth bounds sub-recipe nesting (ADR-018 §7).
	MaxRecipeDepth = contracts.MaxRecipeDepth
	// YieldFactorPPMIdentity is what M4 writes and nothing reads (ADR-018 §8).
	YieldFactorPPMIdentity = contracts.YieldFactorPPMIdentity
)

// ConfigBundle is the inventory context's contribution to GET /sync/config:
// inventory items, their unit conversions, recipes, recipe ingredients and
// modifier ingredient deltas newer than the caller's since_version. The full
// /sync/config route composes this with every other context's contribution —
// that composition is cross-context wiring owned outside
// backend/internal/inventory (backend/cmd/api/syncconfig.go).
type ConfigBundle struct {
	InventoryItems           []InventoryItem
	ItemUnitConversions      []ItemUnitConversion
	Recipes                  []Recipe
	RecipeIngredients        []RecipeIngredient
	ModifierIngredientDeltas []ModifierIngredientDelta
}

// NewInventoryItemInput is what a caller supplies to POST /inventory/items.
// The id is caller-supplied (app-generated UUIDv7, §74) and the write is a
// create-or-update, per the OpenAPI summary — the same idempotent-replay
// shape as every other config write route already in this codebase.
type NewInventoryItemInput struct {
	ID                string
	OutletID          string
	SKU               string
	Name              string
	Category          *string
	Dimension         Dimension
	ReorderLevelMicro *int64
	ParLevelMicro     *int64
	StorageLocation   *string
	IsActive          bool
	Conversions       []NewItemUnitConversionInput
}

// NewItemUnitConversionInput is a child row inside NewInventoryItemInput's
// bundle — item_unit_conversion has no route of its own (ADR-018 §4).
type NewItemUnitConversionInput struct {
	ID              string
	PackUnitLabel   string
	SourceDimension Dimension
	Numerator       int64
	Denominator     int64
}

// NewRecipeInput is what a caller supplies to POST /inventory/recipes: the
// recipe plus its full ingredient list in one bundle, per the OpenAPI
// requestBody shape (`{recipe, ingredients}`).
type NewRecipeInput struct {
	ID                  string
	MenuItemVariantID   string
	Name                string
	OutputDimension     Dimension
	OutputQuantityMicro int64
	Ingredients         []NewRecipeIngredientInput
}

// NewRecipeIngredientInput mirrors contracts.RecipeIngredient's writable
// fields. QuantityDimension is THE AUTHOR'S UNIT and is never derived from
// the referent (ADR-018 0.5.2 addendum) — the write path compares it against
// the referent's own dimension and rejects a mismatch rather than filling it
// in, or the guard could never fire.
type NewRecipeIngredientInput struct {
	ID                string
	ComponentKind     RecipeComponentKind
	InventoryItemID   *string
	SubRecipeID       *string
	QuantityMicro     int64
	QuantityDimension Dimension
	SortOrder         int
}

// LedgerReplayResult is what IngestLedgerEntries and IngestStockCount
// return: the accepted envelope's aggregate type (so the HTTP layer knows
// which shape to echo) alongside the stored row.
type LedgerReplayResult struct {
	AggregateType contracts.AggregateType
	LedgerEntry   *StockLedgerEntry
	DeductionGap  *StockDeductionGap
}

// StockCountReplay is a stock_count plus its stock_count_line children,
// travelling together as one payload (ADR-018 §10, the invoice_line
// precedent).
type StockCountReplay struct {
	Count StockCount
	Lines []StockCountLine
}
