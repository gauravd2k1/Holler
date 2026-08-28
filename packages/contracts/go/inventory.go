// Inventory and recipe contracts — added at 0.5.0 (ADR-018, Milestone 4).
// Mirrors src/types/inventory.ts.
//
// AUTHORITY (§50.1, unchanged by this milestone):
//
//	InventoryItem, Recipe                          CLOUD_TO_EDGE aggregates
//	StockLedgerEntry, StockCount, StockDeductionGap EDGE_TO_CLOUD aggregates
//	ItemUnitConversion, RecipeIngredient,
//	ModifierIngredientDelta, StockCountLine         child rows, no direction
//
// There is deliberately NO StockBalanceSnapshot struct here. It is edge-local
// (SQLite only, no PostgreSQL mirror, no AggregateType) and never crosses a
// boundary, so a Go struct would imply a transport it must never have — the
// same treatment InvoiceSequence gets: named in a comment, typed nowhere.
//
// Field names match sqlite/0013..0017 and postgres/0013..0016 exactly.
package contracts

// Dimension fixes what a micro-quantity means. An item's dimension never
// changes: changing it would silently reinterpret every historical ledger row
// that referenced it, which is why the ledger snapshots the value rather than
// joining for it.
type Dimension string

const (
	DimensionMass   Dimension = "MASS"
	DimensionVolume Dimension = "VOLUME"
	DimensionCount  Dimension = "COUNT"
)

// Every quantity in this file is an integer count of MICRO-units of its
// dimension's canonical unit: micro-grams, micro-litres, micro-pieces. The
// money-is-paise rule generalised, with one scaling rule rather than a
// per-dimension choice. No float, ever.
//
// int64 is not the binding limit — JavaScript is. The TypeScript mirror
// carries these as `number`, so the real ceiling is 2^53, and the Zod schema
// asserts it. A 50 kg sack is 5e10 micro-grams, comfortably inside both.

// DimensionalConversion is a TIER 1 conversion: a physical constant, frozen in
// code rather than stored as a row. Giving these a config write path would
// only create a way to get them wrong per tenant.
//
// Cross-dimension conversion is NOT here and never will be: density varies per
// ingredient (oil is bought in kg and cooked in ml), so g↔ml is not a physical
// constant. Those are per-item rows in ItemUnitConversion. One global g→ml
// factor would be a wrong number for every ingredient it touched.
type DimensionalConversion struct {
	Dimension Dimension
	// Micro-units of the canonical unit per 1 of the named unit.
	Micro int64
}

// DimensionalConversions is the frozen Tier 1 map. Keep in exact agreement
// with DIMENSIONAL_CONVERSIONS in src/types/inventory.ts.
var DimensionalConversions = map[string]DimensionalConversion{
	"mg":    {Dimension: DimensionMass, Micro: 1_000},
	"g":     {Dimension: DimensionMass, Micro: 1_000_000},
	"kg":    {Dimension: DimensionMass, Micro: 1_000_000_000},
	"ml":    {Dimension: DimensionVolume, Micro: 1_000},
	"l":     {Dimension: DimensionVolume, Micro: 1_000_000},
	"piece": {Dimension: DimensionCount, Micro: 1_000_000},
	"dozen": {Dimension: DimensionCount, Micro: 12_000_000},
}

// MaxRecipeDepth bounds sub-recipe nesting. Enforced at cloud write time with
// a recursive-CTE cycle check, and again defensively in the edge resolver,
// which must terminate on a cyclic graph even if a bad row exists: an
// unbounded walk inside confirm_order's transaction hangs a till mid-service.
const MaxRecipeDepth = 8

// YieldFactorPPMIdentity is what M4 writes and nothing reads (ADR-018 §8).
// Inert, not merely unused: 1_000_000 ppm is the identity.
const YieldFactorPPMIdentity = 1_000_000

// InventoryItem is CONFIG, cloud→edge.
//
// Current stock is NEVER a field here, and neither is cost. A quantity written
// by the edge on a row the cloud owns is the half-config, half-transaction row
// ADR-011 forbids. Stock lives in the ledger and its edge-local snapshot; cost
// lives on the ledger entry, because a weighted average is derived from
// edge-recorded purchases.
type InventoryItem struct {
	ID        string    `json:"id"`
	OutletID  string    `json:"outlet_id"`
	SKU       string    `json:"sku"`
	Name      string    `json:"name"`
	Category  *string   `json:"category"`
	Dimension Dimension `json:"dimension"`
	// Crossing a reorder level is a SIGNAL, never a block (ADR-018 Rule 1).
	ReorderLevelMicro *int64  `json:"reorder_level_micro"`
	ParLevelMicro     *int64  `json:"par_level_micro"`
	StorageLocation   *string `json:"storage_location"`
	IsActive          bool    `json:"is_active"`
	// DEFERRED to M5 and inert until then — see YieldFactorPPMIdentity.
	YieldFactorPPM int `json:"yield_factor_ppm"`
	ConfigVersion  int `json:"config_version"`
	SchemaVersion  int `json:"schema_version"`
}

// ItemUnitConversion is a TIER 2 conversion and a CHILD ROW of InventoryItem:
// "1 packet paneer = 200 g" is a property of that paneer, since two suppliers
// may disagree. Ratios are integer numerator/denominator — a conversion is a
// rational multiplication, never a decimal factor.
//
// PackUnitLabel may never collide with DimensionalConversions above: two
// sources of truth for kg→g would need a silent precedence rule between
// disagreeing numbers, which is how a deduction goes quietly wrong. Enforced by
// a CHECK in both stores and by the Zod schema.
type ItemUnitConversion struct {
	ID              string `json:"id"`
	InventoryItemID string `json:"inventory_item_id"`
	PackUnitLabel   string `json:"pack_unit_label"`
	// The dimension the label is measured IN, which need not be the item's own:
	// this is where cross-dimension (density) conversions live.
	SourceDimension Dimension `json:"source_dimension"`
	Numerator       int64     `json:"numerator"`
	Denominator     int64     `json:"denominator"`
	ConfigVersion   int       `json:"config_version"`
	SchemaVersion   int       `json:"schema_version"`
}

// Recipe is CONFIG, cloud→edge, and there is exactly ONE PER SELLABLE UNIT: a
// recipe binds at the same grain as a price. MenuItemVariantID is non-nullable
// and uniquely keys the recipe — the nullable form was rejected because
// NULL != NULL defeats the unique index in both stores, permitting two
// "applies to all variants" recipes for one item.
type Recipe struct {
	ID                string `json:"id"`
	MenuItemVariantID string `json:"menu_item_variant_id"`
	// Snapshotted into every ledger entry this recipe produces, so a year of
	// ledger stays readable without this table.
	Name string `json:"name"`
	// Incremented cloud-side on EVERY edit. Past entries keep the old number,
	// so an edit can never retro-alter a past deduction.
	RecipeVersion int `json:"recipe_version"`
	// WHAT ONE EXECUTION PRODUCES. Non-nullable on every recipe, not only on
	// those referenced as sub-recipes. It unifies the arithmetic into one code
	// path — multiplier = requested_quantity / OutputQuantityMicro, with no
	// special case for the root — and makes a 2-serving platter expressible.
	//
	// Added at 0.5.1: without it, rescaling a sub-recipe silently multiplied
	// every parent's deductions with no error (see sqlite/0019).
	OutputDimension     Dimension `json:"output_dimension"`
	OutputQuantityMicro int64     `json:"output_quantity_micro"`
	ConfigVersion       int       `json:"config_version"`
	SchemaVersion       int       `json:"schema_version"`
}

type RecipeComponentKind string

const (
	RecipeComponentKindItem      RecipeComponentKind = "ITEM"
	RecipeComponentKindSubRecipe RecipeComponentKind = "SUB_RECIPE"
)

// RecipeIngredient is a CHILD ROW of Recipe. A component is EITHER a raw
// material OR a sub-recipe — never both, never neither, the PrintJob.InvoiceID
// precedent where both-set and neither-set are equally rejected.
type RecipeIngredient struct {
	ID              string              `json:"id"`
	RecipeID        string              `json:"recipe_id"`
	ComponentKind   RecipeComponentKind `json:"component_kind"`
	InventoryItemID *string             `json:"inventory_item_id"`
	SubRecipeID     *string             `json:"sub_recipe_id"`
	// Positive: a recipe consumes. Negative deltas are a modifier concept.
	QuantityMicro int64 `json:"quantity_micro"`
	// THE UNIT THE AUTHOR CHOSE — never derived from the referent. If a write
	// path fills this from the referenced item's dimension the comparison
	// becomes x == x and the guard can never fire, while looking correct in
	// review. Added at 0.5.2: without it, reclassifying chicken from MASS to
	// COUNT silently reinterprets every recipe's 220_000_000 as 220 birds.
	QuantityDimension Dimension `json:"quantity_dimension"`
	YieldFactorPPM    int       `json:"yield_factor_ppm"` // DEFERRED M5, inert
	SortOrder         int       `json:"sort_order"`
	ConfigVersion     int       `json:"config_version"`
	SchemaVersion     int       `json:"schema_version"`
}

// ModifierIngredientDelta is a child of MenuItemModifier, itself a child of
// MenuItem, so it rides in the MenuItem config payload and needs no route.
//
// A MODIFIER WITH NO ROW HERE DEDUCTS NOTHING. Absence is never read as
// consent: the PrinterRole rule (0.4.7) applied to ingredients.
type ModifierIngredientDelta struct {
	ID                 string `json:"id"`
	MenuItemModifierID string `json:"menu_item_modifier_id"`
	InventoryItemID    string `json:"inventory_item_id"`
	// SIGNED: "Extra Paneer" positive, "No Onion" negative. Zero is meaningful
	// and permitted — a costed modifier that consumes nothing is different
	// information from an absent row.
	QuantityMicro int64 `json:"quantity_micro"`
	ConfigVersion int   `json:"config_version"`
	SchemaVersion int   `json:"schema_version"`
}

type StockEntryType string

const (
	StockEntryTypePurchase              StockEntryType = "PURCHASE"
	StockEntryTypeConsumption           StockEntryType = "CONSUMPTION"
	StockEntryTypeWastage               StockEntryType = "WASTAGE"
	StockEntryTypeTransferIn            StockEntryType = "TRANSFER_IN"
	StockEntryTypeTransferOut           StockEntryType = "TRANSFER_OUT"
	StockEntryTypeAdjustment            StockEntryType = "ADJUSTMENT"
	StockEntryTypeReturnToVendor        StockEntryType = "RETURN_TO_VENDOR"
	StockEntryTypeProductionConsumption StockEntryType = "PRODUCTION_CONSUMPTION"
	StockEntryTypeProductionOutput      StockEntryType = "PRODUCTION_OUTPUT"
)

// StockEntryOrigin is different information from StockEntryType: a CONSUMPTION
// posted by a recipe and one posted by a modifier delta share a type and are
// different facts, and variance must tell them apart without re-deriving.
type StockEntryOrigin string

const (
	StockEntryOriginRecipe          StockEntryOrigin = "RECIPE"
	StockEntryOriginModifierDelta   StockEntryOrigin = "MODIFIER_DELTA"
	StockEntryOriginManual          StockEntryOrigin = "MANUAL"
	StockEntryOriginCountAdjustment StockEntryOrigin = "COUNT_ADJUSTMENT"
	StockEntryOriginWastage         StockEntryOrigin = "WASTAGE"
)

// StockLedgerEntry is EDGE-AUTHORITATIVE and append-only, enforced by trigger
// in both stores. It is self-describing: it snapshots its context and holds NO
// foreign keys to config or orders, so a recipe edit never retro-alters a past
// deduction and a year of ledger reads without the config tables.
type StockLedgerEntry struct {
	ID       string `json:"id"`
	OutletID string `json:"outlet_id"`
	// THE HIGH-WATER MARK. Per-outlet monotonic, assigned by the edge in the
	// same transaction as the insert. A stock read selects entries NOT COVERED
	// BY THE MARK, never entries after a date: an entry arriving after its day
	// is sealed while carrying that day's business date is absent from the seal
	// and excluded by a date predicate, and would vanish permanently.
	EntrySeq int64 `json:"entry_seq"`
	// Snapshotted, no FK.
	InventoryItemID   string           `json:"inventory_item_id"`
	InventoryItemName string           `json:"inventory_item_name"`
	Dimension         Dimension        `json:"dimension"`
	EntryType         StockEntryType   `json:"entry_type"`
	Origin            StockEntryOrigin `json:"origin"`
	// THE QUANTITY ACTUALLY APPLIED, authoritative. Signed, and deliberately
	// unbounded below: negative stock is permitted and is a variance signal,
	// not an error (ADR-018 Rule 1).
	QuantityAppliedMicro int64 `json:"quantity_applied_micro"`
	// Provenance, all optional, none an FK. Exactly one group is populated,
	// keyed on Origin — a half-attributed deduction is what this prevents.
	RecipeID             *string `json:"recipe_id"`
	RecipeVersion        *int    `json:"recipe_version"`
	RecipeName           *string `json:"recipe_name"`
	ModifierDeltaID      *string `json:"modifier_delta_id"`
	ModifierName         *string `json:"modifier_name"`
	ModifierDeltaVersion *int    `json:"modifier_delta_version"`
	SourceOrderID        *string `json:"source_order_id"`
	SourceOrderItemID    *string `json:"source_order_item_id"`
	ReasonCode           *string `json:"reason_code"`
	// The count that produced a COUNT_ADJUSTMENT, typed and with no FK like
	// the rest of this group (contracts 0.5.5 in the two schemas, 0.5.9 on
	// the wire). Before it, the link lived in `note` as "stock_count:{id}" —
	// provenance in free text is provenance nothing can check, and the ledger
	// is append-only, so a severed link is permanent.
	SourceStockCountID *string `json:"source_stock_count_id"`
	// Procurement provenance, added at 0.6.0 (ADR-019) WITH the wire, the
	// INSERT and the SELECT in the same version — which is the whole lesson of
	// SourceStockCountID above. That field sat in both schemas from 0.5.5 and
	// the cloud never heard of it until 0.5.9; json.Unmarshal is lenient, so
	// it was silently discarded and the PostgreSQL column was NULL for every
	// row. A column nothing reads is a column that does not exist.
	//
	// Exactly one of these is populated, keyed on EntryType — PURCHASE,
	// RETURN_TO_VENDOR and TRANSFER_OUT respectively. Three of the six
	// previously-dead entry_type CHECK branches finally have a writer.
	//
	// There is NO SourceStockTransferInID: TRANSFER_IN is M8, and a field with
	// no consumer is the defect this comment is about.
	SourceGrnID              *string `json:"source_grn_id"`
	SourcePurchaseReturnID   *string `json:"source_purchase_return_id"`
	SourceStockTransferOutID *string `json:"source_stock_transfer_out_id"`
	Note                     *string `json:"note"`
	OccurredAt               string  `json:"occurred_at"`
	// Outlet-local business day, computed once at write time from
	// outlet.timezone and outlet.day_start_time, never recomputed on read. The
	// cloud replays this value; it does not own the inputs as they were.
	BusinessDate    string  `json:"business_date"`
	CreatedByUserID *string `json:"created_by_user_id"`
	// Cost per BASE unit. NO LONGER DEFERRED as of 0.6.0: GrnLine.UnitCostPaise
	// is what populates it, and weighted average cost is derived from these
	// entries. Its exemption in scripts/check-contract-field-consumers.mjs is
	// removed in the same change — an exemption that outlives its reason is a
	// silenced failure.
	UnitCostPaise *int64 `json:"unit_cost_paise"`
	SchemaVersion   int     `json:"schema_version"`
}

type StockCountStatus string

const (
	StockCountStatusOpen      StockCountStatus = "OPEN"
	StockCountStatusCompleted StockCountStatus = "COMPLETED"
)

// StockCount is EDGE-AUTHORITATIVE. It is the only instrument that can
// FALSIFY the deduction engine: theoretical deduction is arithmetic over data
// we control and will always agree with itself. Mutable while OPEN, immutable
// once COMPLETED, enforced by trigger in both stores.
type StockCount struct {
	ID              string           `json:"id"`
	OutletID        string           `json:"outlet_id"`
	BusinessDate    string           `json:"business_date"`
	Status          StockCountStatus `json:"status"`
	StartedAt       string           `json:"started_at"`
	CompletedAt     *string          `json:"completed_at"`
	CountedByUserID *string          `json:"counted_by_user_id"`
	Note            *string          `json:"note"`
	SchemaVersion   int              `json:"schema_version"`
}

// StockCountLine is a CHILD ROW of StockCount, travelling inside its payload.
type StockCountLine struct {
	ID                   string    `json:"id"`
	StockCountID         string    `json:"stock_count_id"`
	InventoryItemID      string    `json:"inventory_item_id"`
	InventoryItemName    string    `json:"inventory_item_name"`
	Dimension            Dimension `json:"dimension"`
	CountedQuantityMicro int64     `json:"counted_quantity_micro"`
	// The theoretical balance AT THE MOMENT OF COUNTING, snapshotted so
	// variance stays reproducible. Recomputing it later compares today's theory
	// against yesterday's shelf. Signed: theory can be negative.
	ExpectedQuantityMicro int64   `json:"expected_quantity_micro"`
	Note                  *string `json:"note"`
	SchemaVersion         int     `json:"schema_version"`
}

type StockDeductionGapReason string

const (
	StockDeductionGapReasonNoRecipe      StockDeductionGapReason = "NO_RECIPE"
	StockDeductionGapReasonNoVariant     StockDeductionGapReason = "NO_VARIANT"
	StockDeductionGapReasonCycle         StockDeductionGapReason = "CYCLE"
	StockDeductionGapReasonDepthExceeded StockDeductionGapReason = "DEPTH_EXCEEDED"
	StockDeductionGapReasonUnknownUnit   StockDeductionGapReason = "UNKNOWN_UNIT"
	// 0.5.1: a parent asking for 180g of a recipe that yields ml. Nothing to
	// convert through — a recipe is not an inventory item. Rejected at cloud
	// write time; a gap at the edge, never a failed confirm.
	StockDeductionGapReasonDimensionMismatch StockDeductionGapReason = "DIMENSION_MISMATCH"
	// 0.5.3: a delta or ingredient referencing an item that is not there.
	// Skipping it silently, as T2 did, inverts why this table exists — a real
	// failure with an absent signal is an absent feature.
	StockDeductionGapReasonUnresolvableReference StockDeductionGapReason = "UNRESOLVABLE_REFERENCE"
)

// StockDeductionGap is a SIGNAL, NEVER A CORRECTION. Deductions are never
// backfilled when the recipe is later authored — that would retro-alter
// history. In the variance report it appears as a named term ("N sales
// unaccounted"), never folded into shrinkage.
//
// Cloud-visible because the person who can SEE it and the person who can FIX it
// are different people in different places: fixing means authoring a recipe,
// which is cloud config under recipe.manage. It shares the ledger ingest route
// rather than taking one of its own.
type StockDeductionGap struct {
	ID       string `json:"id"`
	OutletID string `json:"outlet_id"`
	// THE REPLAY MARK, 0.5.8. Per-outlet monotonic, minted by the edge from
	// stock_deduction_gap_sequence — SEPARATE from the ledger's counter, so
	// the two ranged streams advance independently. The cloud checks
	// contiguity of the received stream against it, which is why this column
	// lives in both stores while the replay CURSORS stay edge-local.
	EntrySeq          int64   `json:"entry_seq"`
	OrderID           string  `json:"order_id"`
	OrderItemID       string  `json:"order_item_id"`
	MenuItemID        string  `json:"menu_item_id"`
	MenuItemVariantID *string `json:"menu_item_variant_id"` // null is itself a reason
	MenuItemName      string  `json:"menu_item_name"`
	// Sellable units sold unaccounted — a plain count, NOT a micro-quantity:
	// nothing was resolved to an ingredient, which is the point of the row.
	Quantity      int                     `json:"quantity"`
	Reason        StockDeductionGapReason `json:"reason"`
	OccurredAt    string                  `json:"occurred_at"`
	BusinessDate  string                  `json:"business_date"`
	SchemaVersion int                     `json:"schema_version"`
}
