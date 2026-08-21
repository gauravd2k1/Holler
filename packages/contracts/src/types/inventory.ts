// Inventory and recipe contracts — added at 0.5.0 (ADR-018, Milestone 4).
//
// Field names and types match sqlite/0013..0017 and postgres/0013..0016
// exactly. Read the SQLite migrations for the reasoning; this file carries the
// wire shapes and the two invariants TypeScript can actually enforce.
//
// AUTHORITY (§50.1, unchanged by this milestone):
//   inventory_item, recipe                     CLOUD_TO_EDGE aggregates
//   stock_ledger_entry, stock_count,
//   stock_deduction_gap                        EDGE_TO_CLOUD aggregates
//   item_unit_conversion, recipe_ingredient,
//   modifier_ingredient_delta, stock_count_line   child rows, no direction
//
// `stock_balance_snapshot` has NO type here, deliberately — it is edge-local
// (SQLite only, no PostgreSQL mirror, no AggregateType) and never crosses a
// boundary, so a wire type would imply a transport it must never have. Exactly
// how `invoice_sequence` is treated: named in a comment, typed nowhere.

import { z } from "zod";

// ---------------------------------------------------------------------------
// Quantities
// ---------------------------------------------------------------------------

export const DimensionSchema = z.enum(["MASS", "VOLUME", "COUNT"]);
export type Dimension = z.infer<typeof DimensionSchema>;

// Every quantity in this file is an integer count of MICRO-units of its
// dimension's canonical unit: micro-grams, micro-litres, micro-pieces. The
// money-is-paise rule generalised, with one scaling rule rather than a
// per-dimension choice. No float, ever.
//
// THE BINDING LIMIT IS JAVASCRIPT, NOT i64, which is why this refinement
// exists at all: TypeScript and Zod carry these as `number`, so the ceiling is
// Number.MAX_SAFE_INTEGER (2^53), not i64's 9.2e18. A 50 kg sack is 5e10
// micro-grams, five orders of magnitude inside it — but the limit is asserted
// here rather than described in a comment, because a comment asserting a
// property nothing verifies is worse than no comment (docs/retro.md,
// 2026-08-20).
const SAFE_INTEGER_MESSAGE =
  "micro-quantity exceeds Number.MAX_SAFE_INTEGER — beyond this point " +
  "JavaScript arithmetic is silently lossy and the value cannot be trusted " +
  "to round-trip";

const isSafe = (n: number) => Number.isSafeInteger(n);

// Signed: consumption is negative, purchase positive, and a modifier delta may
// be either. Deliberately unbounded below — negative stock is permitted and is
// a variance signal, not an error (ADR-018 Rule 1).
const microQuantity = () => z.number().int().refine(isSafe, { message: SAFE_INTEGER_MESSAGE });

// Strictly positive. Note the ordering: `.positive()` must be applied to the
// ZodNumber, before `.refine()` turns it into a ZodEffects that has no such
// method.
const positiveMicroQuantity = () =>
  z.number().int().positive().refine(isSafe, { message: SAFE_INTEGER_MESSAGE });

// TIER 1 — DIMENSIONAL CONVERSIONS. Physical constants, frozen in code, NOT a
// table: giving them a config write path would only create a way to get them
// wrong per tenant. Values are micro-units of the canonical unit per 1 of the
// named unit.
//
// Cross-dimension conversion is NOT here and never will be: density varies per
// ingredient (oil is bought in kg and cooked in ml), so g↔ml is not a physical
// constant. Those live in item_unit_conversion, per item. A single global g→ml
// factor would be a wrong number for every ingredient it touched.
export const DIMENSIONAL_CONVERSIONS = {
  mg: { dimension: "MASS", micro: 1_000 },
  g: { dimension: "MASS", micro: 1_000_000 },
  kg: { dimension: "MASS", micro: 1_000_000_000 },
  ml: { dimension: "VOLUME", micro: 1_000 },
  l: { dimension: "VOLUME", micro: 1_000_000 },
  piece: { dimension: "COUNT", micro: 1_000_000 },
  dozen: { dimension: "COUNT", micro: 12_000_000 },
} as const satisfies Record<string, { dimension: Dimension; micro: number }>;

// A pack unit label may never collide with the frozen map above — two sources
// of truth for kg→g would need a silent precedence rule between disagreeing
// numbers, which is how a deduction goes quietly wrong. Enforced by a CHECK in
// both stores; mirrored here so the wire shape refuses it too.
const RESERVED_UNIT_LABELS = new Set([
  ...Object.keys(DIMENSIONAL_CONVERSIONS),
  "litre",
  "liter",
  "pieces",
  "pc",
]);

// The sub-recipe depth limit. Enforced at cloud write time with a
// recursive-CTE cycle check, and again defensively in the edge resolver —
// which must terminate on a cyclic graph even if a bad row exists, because an
// unbounded walk inside confirm_order's transaction hangs a till mid-service.
export const MAX_RECIPE_DEPTH = 8;

// M4 writes exactly this and nothing reads it (ADR-018 §8). Inert, not merely
// unused: 1_000_000 ppm is the identity.
export const YIELD_FACTOR_PPM_IDENTITY = 1_000_000;

// ---------------------------------------------------------------------------
// Config aggregates — CLOUD_TO_EDGE
// ---------------------------------------------------------------------------

export const InventoryItemSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  sku: z.string().min(1),
  name: z.string().min(1),
  category: z.string().nullable(),
  // Fixes what every micro-quantity on this item means. It never changes:
  // changing it would silently reinterpret every historical ledger row, which
  // is why the ledger snapshots the value rather than joining for it.
  dimension: DimensionSchema,
  // Crossing a reorder level is a SIGNAL, never a block (ADR-018 Rule 1).
  reorder_level_micro: microQuantity().nullable(),
  par_level_micro: microQuantity().nullable(),
  storage_location: z.string().nullable(),
  is_active: z.boolean(),
  // DEFERRED to M5 and INERT until then — see YIELD_FACTOR_PPM_IDENTITY.
  yield_factor_ppm: z.number().int().positive(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type InventoryItem = z.infer<typeof InventoryItemSchema>;

// TIER 2 — pack conversions, per item. "1 packet paneer = 200 g" is a property
// of that paneer: two suppliers may disagree, and a global packet size would be
// wrong for one of them. Ratios are integer numerator/denominator — a
// conversion is a rational multiplication, never a decimal factor.
export const ItemUnitConversionSchema = z.object({
  id: z.string().uuid(),
  inventory_item_id: z.string().uuid(),
  pack_unit_label: z
    .string()
    .min(1)
    .refine((label) => !RESERVED_UNIT_LABELS.has(label.toLowerCase()), {
      message:
        "pack_unit_label may not be a unit the frozen dimensional map already " +
        "defines — two sources of truth for the same conversion need a silent " +
        "precedence rule, and a silent precedence rule between disagreeing " +
        "numbers is how a deduction becomes quietly wrong",
    }),
  // The dimension the label is measured IN, which need not be the item's own:
  // this is where cross-dimension (density) conversions live.
  source_dimension: DimensionSchema,
  numerator: z.number().int().positive(),
  denominator: z.number().int().positive(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type ItemUnitConversion = z.infer<typeof ItemUnitConversionSchema>;

// ONE RECIPE PER SELLABLE UNIT. A recipe binds at the same grain as a price.
// menu_item_variant_id is NOT NULL and uniquely keys the recipe: nullable was
// rejected because NULL != NULL defeats the unique index in both stores, which
// would permit two "applies to all variants" recipes for one item.
export const RecipeSchema = z.object({
  id: z.string().uuid(),
  menu_item_variant_id: z.string().uuid(),
  // Snapshotted into every ledger entry this recipe produces, so a year of
  // ledger stays readable without this table — which, being config, sync
  // overwrites repeatedly.
  name: z.string().min(1),
  // Incremented cloud-side on EVERY edit. Past entries keep the old number, so
  // an edit can never retro-alter a past deduction.
  recipe_version: z.number().int().positive(),
  // WHAT ONE EXECUTION OF THIS RECIPE PRODUCES. NOT NULL on every recipe, not
  // only on those referenced as sub-recipes: nullable-with-enforcement-at-
  // reference-time is the shape this contract keeps rejecting.
  //
  // It unifies the arithmetic into one code path —
  //   multiplier = requested_quantity / output_quantity_micro
  // with no special case for the root — and makes a 2-serving sharing platter
  // expressible, which the 0.5.0 multiplier reading could not express at all.
  //
  // A dish yields 1 serving (COUNT, 1_000_000); a gravy 300 ml; a spice mix
  // 250 g. Added at 0.5.1 because without it, rescaling a sub-recipe silently
  // multiplied every parent's deductions — see sqlite/0019.
  output_dimension: DimensionSchema,
  output_quantity_micro: positiveMicroQuantity(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type Recipe = z.infer<typeof RecipeSchema>;

export const RecipeComponentKindSchema = z.enum(["ITEM", "SUB_RECIPE"]);
export type RecipeComponentKind = z.infer<typeof RecipeComponentKindSchema>;

// A component is EITHER a raw material OR a sub-recipe — never both, never
// neither. The print_job.invoice_id precedent: both-set and neither-set are
// equally rejected rather than one silently winning.
export const RecipeIngredientSchema = z
  .object({
    id: z.string().uuid(),
    recipe_id: z.string().uuid(),
    component_kind: RecipeComponentKindSchema,
    inventory_item_id: z.string().uuid().nullable(),
    sub_recipe_id: z.string().uuid().nullable(),
    // Positive: a recipe consumes. Negative deltas are a modifier concept.
    quantity_micro: positiveMicroQuantity(),
    // THE UNIT THE AUTHOR CHOSE — never derived from the referent.
    //
    // If a write path or an authoring UI fills this in by looking up the
    // referenced item's dimension, the cloud's comparison becomes x == x and
    // the guard can never fire. It will look correct in review: every row
    // consistent, every test green, the column decoration. The lazy
    // implementation is the tautological one.
    //
    // Added at 0.5.2 because without it quantity_micro was dimensionless in
    // storage: reclassify chicken from MASS to COUNT and every recipe silently
    // reinterprets 220_000_000 as 220 whole birds.
    quantity_dimension: DimensionSchema,
    yield_factor_ppm: z.number().int().positive(), // DEFERRED M5, inert
    sort_order: z.number().int(),
    config_version: z.number().int(),
    schema_version: z.literal(1),
  })
  .refine(
    (r) =>
      r.component_kind === "ITEM"
        ? r.inventory_item_id !== null && r.sub_recipe_id === null
        : r.sub_recipe_id !== null && r.inventory_item_id === null,
    {
      message:
        "exactly one component reference: an ITEM row carries inventory_item_id, " +
        "a SUB_RECIPE row carries sub_recipe_id, and neither may carry both",
    },
  )
  .refine((r) => r.sub_recipe_id === null || r.sub_recipe_id !== r.recipe_id, {
    message:
      "a recipe cannot contain itself. The general cycle case is caught by the " +
      "cloud write path's recursive-CTE reachability check and by the edge " +
      "resolver's depth/visited backstop; this catches the shortest one",
  });
export type RecipeIngredient = z.infer<typeof RecipeIngredientSchema>;

// Child of menu_item_modifier, which is itself a child of menu_item — so it
// rides in the MenuItem config payload and needs no route of its own.
//
// A MODIFIER WITH NO ROW HERE DEDUCTS NOTHING. Absence is never read as
// consent: the printer_role rule (0.4.7) applied to ingredients.
export const ModifierIngredientDeltaSchema = z.object({
  id: z.string().uuid(),
  menu_item_modifier_id: z.string().uuid(),
  inventory_item_id: z.string().uuid(),
  // SIGNED: "Extra Paneer" positive, "No Onion" negative. Zero is meaningful
  // and permitted — a costed modifier that consumes nothing is different
  // information from an absent row.
  quantity_micro: microQuantity(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type ModifierIngredientDelta = z.infer<typeof ModifierIngredientDeltaSchema>;

// ---------------------------------------------------------------------------
// Edge-authoritative aggregates — EDGE_TO_CLOUD
// ---------------------------------------------------------------------------

export const StockEntryTypeSchema = z.enum([
  "PURCHASE",
  "CONSUMPTION",
  "WASTAGE",
  "TRANSFER_IN",
  "TRANSFER_OUT",
  "ADJUSTMENT",
  "RETURN_TO_VENDOR",
  "PRODUCTION_CONSUMPTION",
  "PRODUCTION_OUTPUT",
]);
export type StockEntryType = z.infer<typeof StockEntryTypeSchema>;

// Where the entry came from, which is different information from what it is: a
// CONSUMPTION posted by a recipe and one posted by a modifier delta share an
// entry_type and are different facts, and variance has to tell them apart
// without re-deriving anything.
export const StockEntryOriginSchema = z.enum([
  "RECIPE",
  "MODIFIER_DELTA",
  "MANUAL",
  "COUNT_ADJUSTMENT",
  "WASTAGE",
]);
export type StockEntryOrigin = z.infer<typeof StockEntryOriginSchema>;

export const StockLedgerEntrySchema = z
  .object({
    id: z.string().uuid(),
    outlet_id: z.string().uuid(),
    // THE HIGH-WATER MARK. Per-outlet monotonic, assigned by the edge in the
    // same transaction as the insert. A stock read selects entries NOT COVERED
    // BY THE MARK, never entries after a date — an entry arriving after its day
    // is sealed while carrying that day's business_date is absent from the seal
    // and excluded by a date predicate, and would vanish permanently.
    entry_seq: z.number().int().positive(),
    // Snapshotted, no FK: a recipe edit must never retro-alter a past
    // deduction, and a year of ledger must read without the config tables.
    inventory_item_id: z.string().uuid(),
    inventory_item_name: z.string().min(1),
    dimension: DimensionSchema,
    entry_type: StockEntryTypeSchema,
    origin: StockEntryOriginSchema,
    // THE QUANTITY ACTUALLY APPLIED, authoritative. Signed, and deliberately
    // unbounded below: negative stock is permitted and is a variance signal,
    // not an error (ADR-018 Rule 1).
    quantity_applied_micro: microQuantity(),
    recipe_id: z.string().uuid().nullable(),
    recipe_version: z.number().int().nullable(),
    recipe_name: z.string().nullable(),
    modifier_delta_id: z.string().uuid().nullable(),
    modifier_name: z.string().nullable(),
    modifier_delta_version: z.number().int().nullable(),
    source_order_id: z.string().uuid().nullable(),
    source_order_item_id: z.string().uuid().nullable(),
    reason_code: z.string().nullable(),
    note: z.string().nullable(),
    occurred_at: z.string().datetime(),
    // Outlet-local business day, computed once at write time from
    // outlet.timezone and outlet.day_start_time, never recomputed on read.
    business_date: z.string().regex(/^\d{4}-\d{2}-\d{2}$/),
    created_by_user_id: z.string().uuid().nullable(),
    unit_cost_paise: z.number().int().nullable(), // DEFERRED M5
    schema_version: z.literal(1),
  })
  .refine(
    (e) => {
      switch (e.origin) {
        case "RECIPE":
          return e.recipe_id !== null && e.modifier_delta_id === null;
        case "MODIFIER_DELTA":
          return e.modifier_delta_id !== null && e.recipe_id === null;
        default:
          return e.recipe_id === null && e.modifier_delta_id === null;
      }
    },
    {
      message:
        "exactly one provenance, keyed on origin: a RECIPE row carries a recipe " +
        "and no modifier delta, a MODIFIER_DELTA row the inverse, and a MANUAL / " +
        "COUNT_ADJUSTMENT / WASTAGE row carries neither. A half-attributed " +
        "deduction is what this provenance exists to prevent",
    },
  );
export type StockLedgerEntry = z.infer<typeof StockLedgerEntrySchema>;

export const StockCountStatusSchema = z.enum(["OPEN", "COMPLETED"]);
export type StockCountStatus = z.infer<typeof StockCountStatusSchema>;

// A physical count is the only instrument that can FALSIFY the deduction
// engine: theoretical deduction is arithmetic over data we control and will
// always agree with itself. Mutable while OPEN, immutable once COMPLETED —
// enforced by trigger in both stores.
export const StockCountSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  business_date: z.string().regex(/^\d{4}-\d{2}-\d{2}$/),
  status: StockCountStatusSchema,
  started_at: z.string().datetime(),
  completed_at: z.string().datetime().nullable(),
  counted_by_user_id: z.string().uuid().nullable(),
  note: z.string().nullable(),
  schema_version: z.literal(1),
});
export type StockCount = z.infer<typeof StockCountSchema>;

export const StockCountLineSchema = z.object({
  id: z.string().uuid(),
  stock_count_id: z.string().uuid(),
  inventory_item_id: z.string().uuid(),
  inventory_item_name: z.string().min(1),
  dimension: DimensionSchema,
  counted_quantity_micro: microQuantity(),
  // The theoretical balance AT THE MOMENT OF COUNTING, snapshotted so variance
  // stays reproducible. Recomputing it later compares today's theory against
  // yesterday's shelf. Signed: theory can be negative.
  expected_quantity_micro: microQuantity(),
  note: z.string().nullable(),
  schema_version: z.literal(1),
});
export type StockCountLine = z.infer<typeof StockCountLineSchema>;

export const StockDeductionGapReasonSchema = z.enum([
  "NO_RECIPE",
  "NO_VARIANT",
  "CYCLE",
  "DEPTH_EXCEEDED",
  "UNKNOWN_UNIT",
  // 0.5.1: a parent asking for 180 g of a recipe that yields ml. There is
  // nothing to convert through — a recipe is not an inventory item, so no
  // density row exists. The cloud rejects this at write time; the edge, which
  // may hold config from an older cloud, degrades to a gap and completes the
  // sale, exactly as it does for a cycle.
  "DIMENSION_MISMATCH",
]);
export type StockDeductionGapReason = z.infer<typeof StockDeductionGapReasonSchema>;

// A SIGNAL, NEVER A CORRECTION. Deductions are never backfilled when the recipe
// is later authored — that would retro-alter history. In the variance report it
// appears as a named term ("N sales unaccounted"), never folded into shrinkage.
//
// Cloud-visible because the person who can SEE it and the person who can FIX it
// are different people in different places: fixing means authoring a recipe,
// which is cloud config under recipe.manage.
export const StockDeductionGapSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  order_id: z.string().uuid(),
  order_item_id: z.string().uuid(),
  menu_item_id: z.string().uuid(),
  menu_item_variant_id: z.string().uuid().nullable(), // null is itself a reason
  menu_item_name: z.string().min(1),
  // Sellable units sold unaccounted — a plain count, NOT a micro-quantity:
  // nothing was resolved to an ingredient, which is the point of the row.
  quantity: z.number().int().positive(),
  reason: StockDeductionGapReasonSchema,
  occurred_at: z.string().datetime(),
  business_date: z.string().regex(/^\d{4}-\d{2}-\d{2}$/),
  schema_version: z.literal(1),
});
export type StockDeductionGap = z.infer<typeof StockDeductionGapSchema>;
