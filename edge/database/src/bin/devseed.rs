//! Seeds a development edge SQLite database so the POS can log in offline.
//!
//! DEVELOPMENT ONLY. Never runs at an outlet, never ships in the installer.
//! See docs/DEV_SETUP.md.
//!
//! Why this exists: `Db::open` applies the frozen contract schema itself, so a
//! fresh device gets every table — but it gets *zero rows*, and nothing else
//! ever fills them. `edge/sync` implements the cloud→edge config pull
//! (`GET /sync/config`), but `apps/pos/src-tauri/src/lib.rs` never starts the
//! sync worker, so on a developer machine the config bundle never arrives.
//! Until device enrollment and sync startup are wired, this binary stands in
//! for both by writing the same rows a config pull would have applied.
//!
//! It goes through the crate's public repository functions rather than raw
//! SQL so it cannot drift from the frozen schema (ADR-003: nothing outside
//! this crate touches the SQLite file directly).

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use holler_edge_database::crypto::EncryptionKey;
use holler_edge_database::inventory::{grams, kilograms, litres, millilitres, pieces};
use holler_edge_database::model::{
    AppUser, ComplianceVersion, Device, DiscountDefinition, InventoryItem, InvoiceSeries,
    ItemUnitConversion, MenuCategory, MenuItem, MenuItemModifier, MenuItemVariant,
    ModifierIngredientDelta, Outlet, OutletFiscalProfile, Printer, Recipe, RecipeIngredient,
    RestaurantTable, Station, TaxProfile, TaxRule,
};
use holler_edge_database::{repo, Db};

// Fixed development ids. MUST match the constants in
// backend/cmd/devseed/main.go — the two seeders describe the same outlet.
const TENANT_ID: &str = "0191a000-0000-7000-8000-000000000001";
const BRAND_ID: &str = "0191a000-0000-7000-8000-000000000002";
const OUTLET_ID: &str = "0191a000-0000-7000-8000-00000000000a";
const DEVICE_ID: &str = "0191a000-0000-7000-8000-00000000000b";
const CASHIER_ID: &str = "0191a000-0000-7000-8000-00000000000c";
/// The M5 criterion 5 approver. Matches `buyerID` in `backend/cmd/devseed`.
const BUYER_ID: &str = "0191a000-0000-7000-8000-000000000022";
const BUYER_EMAIL: &str = "buyer@holler.test";
// KDS screen device row (T12, Milestone 2). Next in the fixed devseed
// sequence after the cashier (...000c). This id is ALSO hand-pinned into
// apps/kds/.env.dev by the coordinator ahead of this change landing — it
// MUST stay exactly this value, and scripts/dev-bootstrap.ps1 must write the
// same id into apps/kds/.env.dev. Postgres has no matching row: `device` is
// edge-SQLite-only (packages/contracts/sqlite/0001_init.sql), so
// backend/cmd/devseed/main.go needs no change for this id to exist.
const KDS_DEVICE_ID: &str = "0191a000-0000-7000-8000-00000000000d";
const CATEGORY_ID: &str = "0191a000-0000-7000-8000-000000000010";
const ITEM_CHAI_ID: &str = "0191a000-0000-7000-8000-000000000011";
const ITEM_THALI_ID: &str = "0191a000-0000-7000-8000-000000000012";
const VARIANT_ID: &str = "0191a000-0000-7000-8000-000000000013";
const MOD_LESS_SUGAR_ID: &str = "0191a000-0000-7000-8000-000000000014";
const MOD_EXTRA_SUGAR_ID: &str = "0191a000-0000-7000-8000-000000000015";
const TABLE_1_ID: &str = "0191a000-0000-7000-8000-000000000020";
const TABLE_2_ID: &str = "0191a000-0000-7000-8000-000000000021";
// Kitchen routing (T12): without a station and a menu_item_station row,
// send_order_to_kitchen_with_outbox produces zero KOTs for every seeded item
// — the item-1 runbook (send an order to the kitchen and see it on a KDS
// screen) had nothing to route through until this was added.
const STATION_ID: &str = "0191a000-0000-7000-8000-000000000030";
const STATION_CODE: &str = "MAIN_KITCHEN";

// ---- T0b: the real seed menu (HOLLER_DEV_MENU_SPEC.md) ----
//
// Everything below is ADDITIVE to the fixture above. The original category
// (CATEGORY_ID "Beverages"), its two items (ITEM_CHAI_ID, ITEM_THALI_ID,
// with VARIANT_ID/MOD_LESS_SUGAR_ID/MOD_EXTRA_SUGAR_ID) and the original
// station (STATION_ID "MAIN_KITCHEN") are left completely untouched on
// purpose: `tests/e2e-scenario/harness` pins those exact ids, that exact
// price (4000 paise), that exact single-station routing and that exact
// `tax_profile_id = None` fallback behaviour, and does not run with
// `HOLLER_SEED_BILLING=1`. Renaming, re-pricing or re-routing any of them
// would silently break that harness. The spec's own "Beverages" category
// (which happens to share a name with the legacy one) is therefore seeded
// as a SECOND, separate category below rather than folded into CATEGORY_ID.
//
// DEV VALUES ONLY: every price, HSN/SAC code and tax profile below is a
// representative development fixture chosen to exercise the tax/KOT-routing
// engine end to end. A production outlet configures its own catalogue,
// prices, HSN/SAC codes and tax profiles — none of this ships.
//
// Ids are generated deterministically from small integer sequences (below)
// rather than hand-typed one at a time, so the ~39 items in the spec don't
// need ~39 hand-maintained constants. Re-running devseed with the same
// inputs always produces the same ids and the same rows (byte-stable
// snapshot), because the sequence -> id mapping is a pure function.
fn menu_category_id(seq: u32) -> String {
    format!("0191e100-0000-7000-8000-{seq:012x}")
}
fn menu_item_id(seq: u32) -> String {
    format!("0191e200-0000-7000-8000-{seq:012x}")
}
fn menu_variant_id(seq: u32) -> String {
    format!("0191e300-0000-7000-8000-{seq:012x}")
}
fn menu_modifier_id(seq: u32) -> String {
    format!("0191e400-0000-7000-8000-{seq:012x}")
}

// ---- T1b: inventory items and recipes (ADR-018, contracts 0.5.0-0.5.3) ----
// Same deterministic sequence -> id scheme as the menu ids above, in their
// own disjoint id ranges so nothing here can ever collide with a menu id.
fn inventory_item_id(seq: u32) -> String {
    format!("0191e800-0000-7000-8000-{seq:012x}")
}
fn item_unit_conversion_id(seq: u32) -> String {
    format!("0191e810-0000-7000-8000-{seq:012x}")
}
fn recipe_id(seq: u32) -> String {
    format!("0191e820-0000-7000-8000-{seq:012x}")
}
fn recipe_ingredient_id(seq: u32) -> String {
    format!("0191e830-0000-7000-8000-{seq:012x}")
}
fn modifier_ingredient_delta_id(seq: u32) -> String {
    format!("0191e840-0000-7000-8000-{seq:012x}")
}

/// The internal, non-sellable menu item/variant/category a sub-recipe binds
/// to — `recipe.menu_item_variant_id` is NOT NULL (0015), so even a
/// component that is never sold directly (a gravy, a masala base) needs a
/// real variant row. `is_available: false` keeps it out of any ordering UI
/// that filters on it; nothing else distinguishes it from a sellable item,
/// because the schema has no "internal" flag — the same soft spot
/// `crate::inventory::resolve`'s module doc comment describes for the
/// missing "every item has a variant" invariant.
const INTERNAL_CATEGORY_ID: &str = "0191e850-0000-7000-8000-000000000001";
const ITEM_MAKHANI_GRAVY_ID: &str = "0191e850-0000-7000-8000-000000000002";
const VARIANT_MAKHANI_GRAVY_ID: &str = "0191e850-0000-7000-8000-000000000003";
const ITEM_ONION_TOMATO_BASE_ID: &str = "0191e850-0000-7000-8000-000000000004";
const VARIANT_ONION_TOMATO_BASE_ID: &str = "0191e850-0000-7000-8000-000000000005";
const RECIPE_MAKHANI_GRAVY_ID: &str = "0191e850-0000-7000-8000-000000000006";
const RECIPE_ONION_TOMATO_BASE_ID: &str = "0191e850-0000-7000-8000-000000000007";

/// The three GST 2.0 (post-Sept-2025) tax profiles the spec's Beverages
/// category needs to exercise mixed-rate invoicing (5% / 18% / 40%) in one
/// order. Seeded unconditionally (menu_item.tax_profile_id is a NOT-NULL-
/// enforceable foreign key once set, and PRAGMA foreign_keys is ON — see
/// pragma.rs), but deliberately `is_default: false` and with NO `tax_rule`
/// children here: a `tax_rule` needs a `compliance_version_id`, and the
/// only `compliance_version` this crate seeds lives behind
/// `HOLLER_SEED_BILLING=1` in `seed_billing` below, precisely so the
/// harness's bare (non-billing) devseed run never gains a second
/// `compliance_version` row and never sees its own resolution silently
/// redirected to this one (`tax::resolve_compliance_version` has no
/// tie-break beyond insertion order). Menu items below reference these
/// profiles explicitly (never `None`), so the `is_default` fallback these
/// items would otherwise trigger is never reached — only the untouched
/// legacy ITEM_CHAI_ID/ITEM_THALI_ID pair relies on that fallback.
const TAX_PROFILE_FOOD5_ID: &str = "0191e600-0000-7000-8000-000000000001";
const TAX_PROFILE_PACKAGED18_ID: &str = "0191e600-0000-7000-8000-000000000002";
const TAX_PROFILE_AERATED40_ID: &str = "0191e600-0000-7000-8000-000000000003";

/// The five stations the spec's KOT routing needs (ADR-014). Distinct from
/// the legacy STATION_ID/"MAIN_KITCHEN" above for the same harness-safety
/// reason as the tax profiles: nothing renames or reroutes a fixture the
/// harness already pins.
const STATION_TANDOOR_ID: &str = "0191e500-0000-7000-8000-000000000001";
const STATION_TANDOOR_CODE: &str = "TANDOOR";
const STATION_MAIN_ID: &str = "0191e500-0000-7000-8000-000000000002";
const STATION_MAIN_CODE: &str = "MAIN";
const STATION_CHAT_ID: &str = "0191e500-0000-7000-8000-000000000003";
const STATION_CHAT_CODE: &str = "CHAT";
const STATION_BAR_ID: &str = "0191e500-0000-7000-8000-000000000004";
const STATION_BAR_CODE: &str = "BAR";
const STATION_DESSERT_ID: &str = "0191e500-0000-7000-8000-000000000005";
const STATION_DESSERT_CODE: &str = "DESSERT";

/// One row per spec menu item. `variants`/`modifier_groups` are literal
/// spec data: the spec gives variant NAMES (Half/Full, Dry/Gravy, ...) with
/// no price deltas, so every seeded variant carries `price_delta_paise: 0`
/// — a representative dev value, not a claim about real half-portion
/// pricing. Modifier deltas are exactly the paise figures the spec lists.
struct SeedItem {
    name: &'static str,
    price_paise: i64,
    tax_profile_id: &'static str,
    hsn_sac: &'static str,
    station_code: &'static str,
    variants: &'static [&'static str],
    modifier_groups: &'static [(&'static str, &'static [(&'static str, i64)])],
}

const SPICE_GROUP: (&str, &[(&str, i64)]) = ("Spice", &[("Mild", 0), ("Med", 0), ("Hot", 0)]);

// ============================================================================
// T1b: inventory items, unit conversions, recipes, modifier deltas
// (ADR-018, contracts 0.5.0-0.5.3, `packages/contracts/sqlite/
// 0015_m4_inventory_config.sql` / `0019_recipe_output.sql` /
// `0020_recipe_ingredient_dimension.sql`).
//
// DEV VALUES ONLY, same posture as the menu above: every SKU, reorder
// level, conversion ratio and recipe quantity below is a representative
// development fixture chosen to exercise the recipe/deduction engine end to
// end (mixed dimensions, a real pack conversion, a real cross-dimension
// density conversion, real sub-recipes at a genuinely fractional multiplier,
// and a deliberate mix of costed and un-costed items/modifiers). A
// production outlet configures its own larder, its own pack sizes and its
// own recipes — none of this ships.
//
// UNIT ARITHMETIC: every `*_micro` value below is computed against the
// FROZEN Tier 1 map (`crate::inventory::units::DIMENSIONAL_CONVERSIONS`,
// mirrored in `packages/contracts`): grams and pieces scale ×1_000_000
// (`g`, `piece`), litres scale ×1_000_000 (`l`), but `mg`/`ml` scale only
// ×1_000 because they are already 1/1000 of their dimension's canonical
// unit. So 220 g = 220_000_000, but 180 ml = 180_000 — NOT 180_000_000.
// Getting this wrong would not fail any CHECK (both are just integers); it
// would silently under- or over-deduct every volume ingredient in this
// file by a factor of 1000, which is exactly the class of silent-wrongness
// 0.5.1/0.5.2 exist to prevent elsewhere. Every figure below was computed
// from a real quantity (e.g. "300 ml" -> 300 * 1_000) rather than invented
// as a round micro number, so the arithmetic can be checked against the
// stated real-world quantity in each comment.
// ============================================================================

/// One row of the dev larder. `reorder_level_micro: None` on a few items
/// (Salt, Sugar, Kasuri Methi) is deliberate — ADR-018 Rule 1 makes the
/// threshold optional config, not every item needs one, and this is the
/// seed's demonstration of that nullability rather than an oversight.
struct SeedInventoryItem {
    sku: &'static str,
    name: &'static str,
    category: &'static str,
    /// `"MASS" | "VOLUME" | "COUNT"`.
    dimension: &'static str,
    reorder_level_micro: Option<i64>,
}

const SEED_INVENTORY_ITEMS: &[SeedInventoryItem] = &[
    SeedInventoryItem {
        sku: "INV-PANEER",
        name: "Paneer",
        category: "Dairy",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(5)),
    },
    SeedInventoryItem {
        sku: "INV-CHICKEN",
        name: "Chicken (Curry Cut, Bone-In)",
        category: "Meat & Poultry",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(15)),
    },
    SeedInventoryItem {
        sku: "INV-MUTTON",
        name: "Mutton (Curry Cut)",
        category: "Meat & Poultry",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(8)),
    },
    SeedInventoryItem {
        sku: "INV-FISH",
        name: "Fish Fillet (Basa)",
        category: "Meat & Poultry",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(6)),
    },
    SeedInventoryItem {
        sku: "INV-EGGS",
        name: "Eggs",
        category: "Dairy",
        dimension: "COUNT",
        reorder_level_micro: Some(pieces(60)),
    },
    SeedInventoryItem {
        sku: "INV-TOMATO",
        name: "Tomato",
        category: "Produce",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(20)),
    },
    SeedInventoryItem {
        sku: "INV-ONION",
        name: "Onion",
        category: "Produce",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(25)),
    },
    SeedInventoryItem {
        sku: "INV-GINGARLIC",
        name: "Ginger-Garlic Paste",
        category: "Produce",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(3)),
    },
    SeedInventoryItem {
        sku: "INV-SPINACH",
        name: "Spinach",
        category: "Produce",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(10)),
    },
    SeedInventoryItem {
        sku: "INV-LIME",
        name: "Lime",
        category: "Produce",
        dimension: "COUNT",
        reorder_level_micro: Some(pieces(50)),
    },
    SeedInventoryItem {
        sku: "INV-CREAM",
        name: "Fresh Cream",
        category: "Dairy",
        dimension: "VOLUME",
        reorder_level_micro: Some(litres(5)),
    },
    SeedInventoryItem {
        sku: "INV-BUTTER",
        name: "Butter",
        category: "Dairy",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(4)),
    },
    SeedInventoryItem {
        sku: "INV-GHEE",
        name: "Ghee",
        category: "Dairy",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(4)),
    },
    SeedInventoryItem {
        sku: "INV-MILK",
        name: "Milk",
        category: "Dairy",
        dimension: "VOLUME",
        reorder_level_micro: Some(litres(10)),
    },
    SeedInventoryItem {
        sku: "INV-CURD",
        name: "Curd (Yogurt)",
        category: "Dairy",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(5)),
    },
    SeedInventoryItem {
        sku: "INV-ATTA",
        name: "Atta (Wheat Flour)",
        category: "Grains",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(20)),
    },
    SeedInventoryItem {
        sku: "INV-BASMATI",
        name: "Basmati Rice",
        category: "Grains",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(20)),
    },
    SeedInventoryItem {
        sku: "INV-URADDAL",
        name: "Urad Dal (Whole Black Lentil)",
        category: "Grains",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(10)),
    },
    SeedInventoryItem {
        sku: "INV-SUGAR",
        name: "Sugar",
        category: "Grains",
        dimension: "MASS",
        reorder_level_micro: None,
    },
    SeedInventoryItem {
        sku: "INV-OIL",
        name: "Sunflower Oil",
        category: "Oils",
        dimension: "VOLUME",
        reorder_level_micro: Some(litres(10)),
    },
    SeedInventoryItem {
        sku: "INV-GARAMMASALA",
        name: "Garam Masala",
        category: "Spices",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(1)),
    },
    SeedInventoryItem {
        sku: "INV-CHILLIPOWDER",
        name: "Red Chilli Powder",
        category: "Spices",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(1)),
    },
    SeedInventoryItem {
        sku: "INV-TURMERIC",
        name: "Turmeric Powder",
        category: "Spices",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(1)),
    },
    SeedInventoryItem {
        sku: "INV-CUMIN",
        name: "Cumin Seeds",
        category: "Spices",
        dimension: "MASS",
        reorder_level_micro: Some(grams(500)),
    },
    SeedInventoryItem {
        sku: "INV-CORIANDER",
        name: "Coriander Powder",
        category: "Spices",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(1)),
    },
    SeedInventoryItem {
        sku: "INV-KASURIMETHI",
        name: "Kasuri Methi",
        category: "Spices",
        dimension: "MASS",
        reorder_level_micro: None,
    },
    SeedInventoryItem {
        sku: "INV-SALT",
        name: "Salt",
        category: "Spices",
        dimension: "MASS",
        reorder_level_micro: None,
    },
    SeedInventoryItem {
        sku: "INV-TEALEAVES",
        name: "Tea Leaves",
        category: "Beverages",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(1)),
    },
    SeedInventoryItem {
        sku: "INV-SODAWATER",
        name: "Soda Water (Carbonated)",
        category: "Beverages",
        dimension: "VOLUME",
        reorder_level_micro: Some(litres(5)),
    },
    SeedInventoryItem {
        sku: "INV-BOTTLEDWATER",
        name: "Bottled Water 1L",
        category: "Beverages",
        dimension: "COUNT",
        reorder_level_micro: Some(pieces(24)),
    },
    SeedInventoryItem {
        sku: "INV-COKECAN",
        name: "Coca-Cola Can",
        category: "Beverages",
        dimension: "COUNT",
        reorder_level_micro: Some(pieces(24)),
    },
    SeedInventoryItem {
        sku: "INV-THUMSUPCAN",
        name: "Thums Up Can",
        category: "Beverages",
        dimension: "COUNT",
        reorder_level_micro: Some(pieces(24)),
    },
];

/// A pack-size or cross-dimension conversion, scoped to one item's own SKU.
/// `pack_unit_label` is checked against the frozen dimensional map (kg/g/ml/
/// l/piece/dozen/...) at the schema level — `"tin"`/`"packet"`/`"sack"`/
/// `"crate"` are real pack names, never a unit the frozen map already owns.
struct SeedItemUnitConversion {
    sku: &'static str,
    pack_unit_label: &'static str,
    /// The dimension the pack label is itself measured in — need not equal
    /// the item's own `dimension` (0015's header: this is where a density
    /// conversion lives).
    source_dimension: &'static str,
    numerator: i64,
    denominator: i64,
}

const SEED_ITEM_UNIT_CONVERSIONS: &[SeedItemUnitConversion] = &[
    // "1 packet Paneer = 200 g" — the exact worked example in
    // `crate::inventory::units`'s own doc tests.
    SeedItemUnitConversion {
        sku: "INV-PANEER",
        pack_unit_label: "packet",
        source_dimension: "MASS",
        numerator: grams(200),
        denominator: 1,
    },
    // "1 sack Atta = 25 kg".
    SeedItemUnitConversion {
        sku: "INV-ATTA",
        pack_unit_label: "sack",
        source_dimension: "MASS",
        numerator: kilograms(25),
        denominator: 1,
    },
    // "1 sack Basmati Rice = 25 kg".
    SeedItemUnitConversion {
        sku: "INV-BASMATI",
        pack_unit_label: "sack",
        source_dimension: "MASS",
        numerator: kilograms(25),
        denominator: 1,
    },
    // CROSS-DIMENSION: Sunflower Oil is stocked as VOLUME (it is measured
    // in ml at the cook-line), but a tin is sold and labelled by WEIGHT.
    // "1 tin (nominally 15 kg) ~= 15 L at this oil's density" — a
    // representative dev density, not a physical claim; a real outlet's
    // actual tin size and density are its own procurement data.
    // CORRECTED after the T2c sweep flagged it. The stored value was
    // `grams(15)` — 15 g of oil per tin, a thousandth of the 15 kg the
    // comment claimed, and in the wrong dimension besides: this item is
    // measured in VOLUME, so the numerator must be micro-LITRES however the
    // pack is labelled. Two independent scale errors in one seed row, which
    // is precisely why the constructors above now exist.
    //
    // The correct value is a rational, and this row is the schema's
    // cross-dimension case working as designed: one tin is 15 kg of oil, and
    // sunflower oil is ~0.92 kg per litre, so the volume is 15/0.92 litres =
    // 1500/92 litres exactly. Integer numerator over integer denominator, no
    // float and no pre-rounded decimal — the density lives in the ratio.
    SeedItemUnitConversion {
        sku: "INV-OIL",
        pack_unit_label: "tin",
        source_dimension: "MASS",
        numerator: litres(1500),
        denominator: 92,
    },
    // "1 crate Coca-Cola = 24 cans" — a same-dimension (COUNT) pack, unlike
    // the oil tin above.
    SeedItemUnitConversion {
        sku: "INV-COKECAN",
        pack_unit_label: "crate",
        source_dimension: "COUNT",
        numerator: pieces(24),
        denominator: 1,
    },
];

/// One component of a recipe: either a raw inventory item (by SKU) or a
/// reference to one of the two internal sub-recipes below (by `key`).
/// `quantity_micro`/`dimension` are the AUTHOR's own figures — see this
/// block's header on why volume and mass scale differently, and
/// `crate::inventory::resolve`'s module doc comment on why this dimension
/// is written directly rather than derived from the referent.
enum Comp {
    Item(&'static str, i64, &'static str),
    Sub(&'static str, i64, &'static str),
}

/// The two internal sub-recipes (`MAKHANI_GRAVY`, `ONION_TOMATO_BASE`), each
/// bound to its own hidden carrier item/variant (`seed_recipes` inserts
/// those directly — see `ITEM_MAKHANI_GRAVY_ID` etc above). Referenced by
/// root recipes below at a genuinely FRACTIONAL amount of their batch yield
/// (180 ml / 300 ml, 100 g / 500 g, ...) — never a 1x multiplier, which is
/// the exact case 0.5.1 was written to get right (see that migration's
/// header on why a multiplier-only encoding silently corrupts every parent
/// when the sub-recipe's own yield changes).
const MAKHANI_GRAVY_OUTPUT_DIMENSION: &str = "VOLUME";
/// 300 ml batch (`ml` scales ×1_000 — see this block's header).
const MAKHANI_GRAVY_OUTPUT_MICRO: i64 = millilitres(300);
const MAKHANI_GRAVY_INGREDIENTS: &[Comp] = &[
    Comp::Item("INV-TOMATO", grams(250), "MASS"), // 250 g
    Comp::Item("INV-BUTTER", grams(40), "MASS"),  // 40 g
    Comp::Item("INV-CREAM", millilitres(60), "VOLUME"), // 60 ml
    Comp::Item("INV-GINGARLIC", grams(15), "MASS"), // 15 g
    Comp::Item("INV-GARAMMASALA", grams(5), "MASS"), // 5 g
    Comp::Item("INV-KASURIMETHI", grams(2), "MASS"), // 2 g
    Comp::Item("INV-CHILLIPOWDER", grams(5), "MASS"), // 5 g
];

const ONION_TOMATO_BASE_OUTPUT_DIMENSION: &str = "MASS";
/// 500 g batch (`g` scales ×1_000_000).
const ONION_TOMATO_BASE_OUTPUT_MICRO: i64 = grams(500);
const ONION_TOMATO_BASE_INGREDIENTS: &[Comp] = &[
    Comp::Item("INV-ONION", grams(300), "MASS"),      // 300 g
    Comp::Item("INV-TOMATO", grams(200), "MASS"),     // 200 g
    Comp::Item("INV-GINGARLIC", grams(20), "MASS"),   // 20 g
    Comp::Item("INV-OIL", millilitres(50), "VOLUME"), // 50 ml
    Comp::Item("INV-TURMERIC", grams(5), "MASS"),     // 5 g
    Comp::Item("INV-CHILLIPOWDER", grams(8), "MASS"), // 8 g
    Comp::Item("INV-CORIANDER", grams(8), "MASS"),    // 8 g
    Comp::Item("INV-SALT", grams(6), "MASS"),         // 6 g
];

/// One root recipe: binds to a real sellable `(item_name, variant_name)`
/// from `SEED_CATEGORIES` (or the "Regular" variant added to a few
/// naturally variant-less items so they can carry one at all — see the
/// comment on each of those `SeedItem`s). Every root recipe here yields
/// exactly one serving: `output_dimension = COUNT`,
/// `output_quantity_micro = 1_000_000` — the shape every directly-sellable
/// dish takes (0.5.1's own worked example).
struct SeedRecipe {
    item_name: &'static str,
    variant_name: &'static str,
    ingredients: &'static [Comp],
}

/// 22 of the 39 menu items get a recipe here — "a good share", not all of
/// them. The other 17 are LEFT WITHOUT ONE, on purpose, in two genuinely
/// different ways (both worth exercising — `crate::inventory::resolve`'s
/// `GapReason::NoVariant` vs `GapReason::NoRecipe`):
///
///   - Samosa, Pani Puri, Aloo Tikki Chaat, Seekh Kebab, Egg Bhurji, Jeera
///     Rice, Steamed Rice, Laccha Paratha, Filter Coffee, Gulab Jamun,
///     Gajar Halwa carry NO variant at all (the spec itself gives them
///     none) — a line ordering one of these hits `NoVariant`.
///   - Chana Masala, Mixed Veg Curry, Fish Curry, Mutton Biryani, Sweet
///     Lassi, Packaged Fruit Juice DO carry a variant but deliberately get
///     no recipe row for it — a line ordering one of these hits
///     `NoRecipe`. This is the more realistic gap in practice: a real
///     kitchen costs its signature dishes first and gets to the rest later.
const SEED_RECIPES: &[SeedRecipe] = &[
    SeedRecipe {
        item_name: "Paneer Tikka",
        variant_name: "Full",
        ingredients: &[
            Comp::Item("INV-PANEER", grams(180), "MASS"), // 180 g
            Comp::Item("INV-OIL", millilitres(15), "VOLUME"), // 15 ml
            Comp::Item("INV-CURD", grams(40), "MASS"),    // 40 g
            Comp::Item("INV-GARAMMASALA", grams(3), "MASS"), // 3 g
            Comp::Item("INV-CHILLIPOWDER", grams(3), "MASS"), // 3 g
        ],
    },
    SeedRecipe {
        item_name: "Veg Manchurian",
        variant_name: "Gravy",
        ingredients: &[
            Comp::Item("INV-ONION", grams(40), "MASS"),       // 40 g
            Comp::Item("INV-OIL", millilitres(20), "VOLUME"), // 20 ml
            Comp::Item("INV-GINGARLIC", grams(10), "MASS"),   // 10 g
            Comp::Item("INV-CHILLIPOWDER", grams(3), "MASS"), // 3 g
        ],
    },
    SeedRecipe {
        item_name: "Chicken 65",
        variant_name: "Full",
        ingredients: &[
            Comp::Item("INV-CHICKEN", grams(220), "MASS"), // 220 g
            Comp::Item("INV-OIL", millilitres(30), "VOLUME"), // 30 ml
            Comp::Item("INV-GINGARLIC", grams(12), "MASS"), // 12 g
            Comp::Item("INV-CHILLIPOWDER", grams(5), "MASS"), // 5 g
            Comp::Item("INV-CURD", grams(20), "MASS"),     // 20 g
        ],
    },
    SeedRecipe {
        item_name: "Tandoori Chicken",
        variant_name: "Full",
        ingredients: &[
            Comp::Item("INV-CHICKEN", grams(400), "MASS"), // 400 g
            Comp::Item("INV-CURD", grams(100), "MASS"),    // 100 g
            Comp::Item("INV-GARAMMASALA", grams(6), "MASS"), // 6 g
            Comp::Item("INV-CHILLIPOWDER", grams(6), "MASS"), // 6 g
            Comp::Item("INV-GHEE", grams(15), "MASS"),     // 15 g
        ],
    },
    SeedRecipe {
        item_name: "Malai Tikka",
        variant_name: "Full",
        ingredients: &[
            Comp::Item("INV-CHICKEN", grams(220), "MASS"), // 220 g
            Comp::Item("INV-CREAM", millilitres(40), "VOLUME"), // 40 ml
            Comp::Item("INV-GINGARLIC", grams(10), "MASS"), // 10 g
            Comp::Item("INV-CUMIN", grams(3), "MASS"),     // 3 g
        ],
    },
    // Paneer Butter Masala: the same Makhani Gravy sub-recipe as Butter
    // Chicken below, referenced at a DIFFERENT fractional amount (150 ml of
    // the same 300 ml batch) — proof the resolver re-derives each parent's
    // own multiplier independently rather than sharing one.
    SeedRecipe {
        item_name: "Paneer Butter Masala",
        variant_name: "Full",
        ingredients: &[
            Comp::Item("INV-PANEER", grams(180), "MASS"), // 180 g
            Comp::Sub("MAKHANI_GRAVY", millilitres(150), "VOLUME"), // 150 ml of a 300 ml batch
            Comp::Item("INV-BUTTER", grams(20), "MASS"),  // 20 g
            Comp::Item("INV-CREAM", millilitres(20), "VOLUME"), // 20 ml
        ],
    },
    SeedRecipe {
        item_name: "Dal Makhani",
        variant_name: "Full",
        ingredients: &[
            Comp::Item("INV-URADDAL", grams(100), "MASS"), // 100 g
            Comp::Item("INV-BUTTER", grams(30), "MASS"),   // 30 g
            Comp::Item("INV-CREAM", millilitres(30), "VOLUME"), // 30 ml
            Comp::Item("INV-GINGARLIC", grams(8), "MASS"), // 8 g
            Comp::Item("INV-TOMATO", grams(40), "MASS"),   // 40 g
        ],
    },
    // Palak Paneer: the Onion-Tomato Masala Base sub-recipe at 100 g of its
    // 500 g batch (0.2x) — the MASS-dimension sub-recipe counterpart to
    // Butter Chicken's VOLUME one.
    SeedRecipe {
        item_name: "Palak Paneer",
        variant_name: "Full",
        ingredients: &[
            Comp::Item("INV-PANEER", grams(150), "MASS"),  // 150 g
            Comp::Item("INV-SPINACH", grams(200), "MASS"), // 200 g
            Comp::Sub("ONION_TOMATO_BASE", grams(100), "MASS"), // 100 g of a 500 g batch
            Comp::Item("INV-CREAM", millilitres(20), "VOLUME"), // 20 ml
        ],
    },
    // Butter Chicken: docs/spec/inventory.md's own worked example (Chicken
    // 220g, Makhani gravy 180ml, Butter 20g, Cream 30ml, Kasuri methi 2g) —
    // the exact 180/300 = 0.6 fractional sub-recipe reference contracts
    // 0.5.1 was written to make correct.
    SeedRecipe {
        item_name: "Butter Chicken",
        variant_name: "Full",
        ingredients: &[
            Comp::Item("INV-CHICKEN", grams(220), "MASS"), // 220 g
            Comp::Sub("MAKHANI_GRAVY", millilitres(180), "VOLUME"), // 180 ml of a 300 ml batch
            Comp::Item("INV-BUTTER", grams(20), "MASS"),   // 20 g
            Comp::Item("INV-CREAM", millilitres(30), "VOLUME"), // 30 ml
            Comp::Item("INV-KASURIMETHI", grams(2), "MASS"), // 2 g
        ],
    },
    SeedRecipe {
        item_name: "Chicken Curry",
        variant_name: "Full",
        ingredients: &[
            Comp::Item("INV-CHICKEN", grams(250), "MASS"), // 250 g
            Comp::Sub("ONION_TOMATO_BASE", grams(120), "MASS"), // 120 g of a 500 g batch
            Comp::Item("INV-OIL", millilitres(15), "VOLUME"), // 15 ml
            Comp::Item("INV-CORIANDER", grams(3), "MASS"), // 3 g
        ],
    },
    SeedRecipe {
        item_name: "Mutton Rogan Josh",
        variant_name: "Full",
        ingredients: &[
            Comp::Item("INV-MUTTON", grams(280), "MASS"), // 280 g
            Comp::Item("INV-ONION", grams(60), "MASS"),   // 60 g
            Comp::Item("INV-CURD", grams(50), "MASS"),    // 50 g
            Comp::Item("INV-CHILLIPOWDER", grams(8), "MASS"), // 8 g
            Comp::Item("INV-GARAMMASALA", grams(5), "MASS"), // 5 g
            Comp::Item("INV-GHEE", grams(15), "MASS"),    // 15 g
        ],
    },
    SeedRecipe {
        item_name: "Chicken Biryani",
        variant_name: "Full",
        ingredients: &[
            Comp::Item("INV-BASMATI", grams(200), "MASS"), // 200 g
            Comp::Item("INV-CHICKEN", grams(180), "MASS"), // 180 g
            Comp::Item("INV-CURD", grams(40), "MASS"),     // 40 g
            Comp::Item("INV-GHEE", grams(20), "MASS"),     // 20 g
            Comp::Item("INV-GARAMMASALA", grams(4), "MASS"), // 4 g
        ],
    },
    SeedRecipe {
        item_name: "Veg Biryani",
        variant_name: "Full",
        ingredients: &[
            Comp::Item("INV-BASMATI", grams(200), "MASS"), // 200 g
            Comp::Item("INV-ONION", grams(50), "MASS"),    // 50 g
            Comp::Item("INV-GHEE", grams(15), "MASS"),     // 15 g
            Comp::Item("INV-GARAMMASALA", grams(3), "MASS"), // 3 g
        ],
    },
    SeedRecipe {
        item_name: "Tandoori Roti",
        variant_name: "Plain",
        ingredients: &[
            Comp::Item("INV-ATTA", grams(60), "MASS"), // 60 g
            Comp::Item("INV-GHEE", grams(5), "MASS"),  // 5 g
        ],
    },
    SeedRecipe {
        item_name: "Fresh Lime Soda",
        variant_name: "Sweet",
        ingredients: &[
            Comp::Item("INV-LIME", pieces(1), "COUNT"), // 1 piece
            Comp::Item("INV-SODAWATER", millilitres(200), "VOLUME"), // 200 ml
            Comp::Item("INV-SUGAR", grams(15), "MASS"), // 15 g
        ],
    },
    SeedRecipe {
        item_name: "Kulfi",
        variant_name: "Malai",
        ingredients: &[
            Comp::Item("INV-MILK", millilitres(120), "VOLUME"), // 120 ml
            Comp::Item("INV-SUGAR", grams(20), "MASS"),         // 20 g
            Comp::Item("INV-CREAM", millilitres(20), "VOLUME"), // 20 ml
        ],
    },
    // The following six items got a "Regular" variant added ABOVE
    // specifically so they could carry a recipe at all (the spec itself
    // lists them with no variant) — see the comment on each `SeedItem`.
    SeedRecipe {
        item_name: "Masala Chai",
        variant_name: "Regular",
        ingredients: &[
            Comp::Item("INV-TEALEAVES", grams(4), "MASS"), // 4 g
            Comp::Item("INV-MILK", millilitres(100), "VOLUME"), // 100 ml
            Comp::Item("INV-SUGAR", grams(10), "MASS"),    // 10 g
        ],
    },
    SeedRecipe {
        item_name: "Butter Naan",
        variant_name: "Regular",
        ingredients: &[
            Comp::Item("INV-ATTA", grams(70), "MASS"),   // 70 g
            Comp::Item("INV-BUTTER", grams(10), "MASS"), // 10 g
        ],
    },
    SeedRecipe {
        item_name: "Garlic Naan",
        variant_name: "Regular",
        ingredients: &[
            Comp::Item("INV-ATTA", grams(70), "MASS"), // 70 g
            Comp::Item("INV-GHEE", grams(5), "MASS"),  // 5 g
        ],
    },
    // Bottled Water / Coca-Cola / Thums Up: the simplest possible recipe —
    // one COUNT dish deducting exactly one COUNT stock unit, straight
    // passthrough with no cooking step. Deliberately included: not every
    // "recipe" is a multi-ingredient dish, and the resolver's arithmetic
    // must be exercised at this trivial end of the range too.
    SeedRecipe {
        item_name: "Bottled Water 1L",
        variant_name: "Regular",
        ingredients: &[
            Comp::Item("INV-BOTTLEDWATER", pieces(1), "COUNT"), // 1 piece
        ],
    },
    SeedRecipe {
        item_name: "Coca-Cola (can)",
        variant_name: "Regular",
        ingredients: &[
            Comp::Item("INV-COKECAN", pieces(1), "COUNT"), // 1 piece
        ],
    },
    SeedRecipe {
        item_name: "Thums Up (can)",
        variant_name: "Regular",
        ingredients: &[
            Comp::Item("INV-THUMSUPCAN", pieces(1), "COUNT"), // 1 piece
        ],
    },
];

/// One `modifier_ingredient_delta` row: `(item_name, group_name,
/// option_name)` addresses the modifier exactly as seeded above (or, for
/// the two legacy Sugar rows, `None` — those look up `MOD_EXTRA_SUGAR_ID`/
/// `MOD_LESS_SUGAR_ID` directly, the untouched T0b fixture). SIGNED:
/// positive adds stock consumption, negative reduces it. Most of the
/// modifiers seeded above get NO row here at all — deliberately; a modifier
/// with no row deducts nothing (0015's header), and that path needs seed
/// coverage as much as the costed one does.
struct SeedModifierDelta {
    lookup: ModifierLookup,
    sku: &'static str,
    quantity_micro: i64,
}

enum ModifierLookup {
    Named(&'static str, &'static str, &'static str),
    LegacyExtraSugar,
    LegacyLessSugar,
}

const SEED_MODIFIER_DELTAS: &[SeedModifierDelta] = &[
    // The legacy T0b "Sugar" group on the untouched ITEM_CHAI_ID fixture —
    // the SIGNED pair this table exists to prove: "Extra Sugar" consumes
    // MORE stock, "Less Sugar" consumes LESS (a negative delta), same item,
    // same inventory SKU.
    SeedModifierDelta {
        lookup: ModifierLookup::LegacyExtraSugar,
        sku: "INV-SUGAR",
        quantity_micro: grams(8),
    }, // +8 g
    SeedModifierDelta {
        lookup: ModifierLookup::LegacyLessSugar,
        sku: "INV-SUGAR",
        quantity_micro: -grams(8),
    }, // -8 g
    SeedModifierDelta {
        lookup: ModifierLookup::Named("Butter Naan", "Extras", "Extra butter"),
        sku: "INV-BUTTER",
        quantity_micro: grams(10),
    }, // +10 g
    SeedModifierDelta {
        lookup: ModifierLookup::Named("Dal Makhani", "Extras", "Butter"),
        sku: "INV-BUTTER",
        quantity_micro: grams(15),
    }, // +15 g
    SeedModifierDelta {
        lookup: ModifierLookup::Named("Aloo Tikki Chaat", "Extras", "Extra dahi"),
        sku: "INV-CURD",
        quantity_micro: grams(30),
    }, // +30 g
    // Priced at +0 paise (see its SeedItem above) but NOT free in stock —
    // the deliberate example that a modifier's price and its stock cost are
    // two independent numbers.
    SeedModifierDelta {
        lookup: ModifierLookup::Named("Masala Chai", "Extras", "Extra strong"),
        sku: "INV-TEALEAVES",
        quantity_micro: grams(2),
    }, // +2 g
    SeedModifierDelta {
        lookup: ModifierLookup::Named("Chicken Biryani", "Extras", "Extra raita"),
        sku: "INV-CURD",
        quantity_micro: grams(40),
    }, // +40 g
    SeedModifierDelta {
        lookup: ModifierLookup::Named("Veg Biryani", "Extras", "Extra raita"),
        sku: "INV-CURD",
        quantity_micro: grams(40),
    }, // +40 g
    SeedModifierDelta {
        lookup: ModifierLookup::Named("Butter Chicken", "Extras", "Extra gravy"),
        sku: "INV-CREAM",
        quantity_micro: millilitres(20),
    }, // +20 ml
    SeedModifierDelta {
        lookup: ModifierLookup::Named("Paneer Butter Masala", "Extras", "Extra gravy"),
        sku: "INV-CREAM",
        quantity_micro: millilitres(20),
    }, // +20 ml
];

/// (category name, sort_order, items) — sort_order continues on from the
/// legacy "Beverages" category (sort_order 1) so the legacy category still
/// sorts first in any dev UI that orders by it.
const SEED_CATEGORIES: &[(&str, i64, &[SeedItem])] = &[
    (
        "Starters & Chaat",
        2,
        &[
            SeedItem {
                name: "Samosa (2 pc)",
                price_paise: 6000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_CHAT_CODE,
                variants: &[],
                modifier_groups: &[("Extras", &[("Extra chutney", 1500)])],
            },
            SeedItem {
                name: "Paneer Tikka",
                price_paise: 32000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_TANDOOR_CODE,
                variants: &["Half", "Full"],
                modifier_groups: &[SPICE_GROUP],
            },
            SeedItem {
                name: "Veg Manchurian",
                price_paise: 24000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &["Dry", "Gravy"],
                modifier_groups: &[SPICE_GROUP],
            },
            SeedItem {
                name: "Chicken 65",
                price_paise: 34000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &["Half", "Full"],
                modifier_groups: &[SPICE_GROUP],
            },
            SeedItem {
                name: "Pani Puri (6 pc)",
                price_paise: 8000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_CHAT_CODE,
                variants: &[],
                modifier_groups: &[("Extras", &[("Extra puri", 3000)])],
            },
            SeedItem {
                name: "Aloo Tikki Chaat",
                price_paise: 12000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_CHAT_CODE,
                variants: &[],
                modifier_groups: &[("Extras", &[("Extra dahi", 2000)])],
            },
        ],
    ),
    (
        "Tandoor & Kebabs",
        3,
        &[
            SeedItem {
                name: "Tandoori Chicken",
                price_paise: 42000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_TANDOOR_CODE,
                variants: &["Half", "Full"],
                modifier_groups: &[SPICE_GROUP],
            },
            SeedItem {
                name: "Seekh Kebab (4 pc)",
                price_paise: 36000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_TANDOOR_CODE,
                variants: &[],
                modifier_groups: &[SPICE_GROUP],
            },
            SeedItem {
                name: "Malai Tikka",
                price_paise: 34000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_TANDOOR_CODE,
                variants: &["Half", "Full"],
                modifier_groups: &[],
            },
        ],
    ),
    (
        "Main Course — Veg",
        4,
        &[
            SeedItem {
                name: "Paneer Butter Masala",
                price_paise: 32000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &["Half", "Full"],
                modifier_groups: &[SPICE_GROUP, ("Extras", &[("Extra gravy", 4000)])],
            },
            SeedItem {
                name: "Dal Makhani",
                price_paise: 26000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &["Half", "Full"],
                modifier_groups: &[("Extras", &[("Butter", 2000)])],
            },
            SeedItem {
                name: "Palak Paneer",
                price_paise: 30000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &["Half", "Full"],
                modifier_groups: &[SPICE_GROUP],
            },
            SeedItem {
                name: "Chana Masala",
                price_paise: 22000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &["Half", "Full"],
                modifier_groups: &[SPICE_GROUP],
            },
            SeedItem {
                name: "Mixed Veg Curry",
                price_paise: 24000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &["Half", "Full"],
                modifier_groups: &[],
            },
        ],
    ),
    (
        "Main Course — Non-Veg",
        5,
        &[
            SeedItem {
                name: "Butter Chicken",
                price_paise: 38000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &["Half", "Full"],
                modifier_groups: &[SPICE_GROUP, ("Extras", &[("Extra gravy", 4000)])],
            },
            SeedItem {
                name: "Chicken Curry",
                price_paise: 34000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &["Half", "Full"],
                modifier_groups: &[SPICE_GROUP],
            },
            SeedItem {
                name: "Mutton Rogan Josh",
                price_paise: 46000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &["Half", "Full"],
                modifier_groups: &[SPICE_GROUP],
            },
            SeedItem {
                name: "Fish Curry",
                price_paise: 40000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &["Half", "Full"],
                modifier_groups: &[SPICE_GROUP],
            },
            SeedItem {
                name: "Egg Bhurji",
                price_paise: 18000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &[],
                modifier_groups: &[],
            },
        ],
    ),
    (
        "Biryani & Rice",
        6,
        &[
            SeedItem {
                name: "Chicken Biryani",
                price_paise: 32000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &["Half", "Full"],
                modifier_groups: &[SPICE_GROUP, ("Extras", &[("Extra raita", 3000)])],
            },
            SeedItem {
                name: "Veg Biryani",
                price_paise: 26000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &["Half", "Full"],
                modifier_groups: &[("Extras", &[("Extra raita", 3000)])],
            },
            SeedItem {
                name: "Mutton Biryani",
                price_paise: 42000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &["Half", "Full"],
                modifier_groups: &[SPICE_GROUP],
            },
            SeedItem {
                name: "Jeera Rice",
                price_paise: 14000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &[],
                modifier_groups: &[],
            },
            SeedItem {
                name: "Steamed Rice",
                price_paise: 10000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_MAIN_CODE,
                variants: &[],
                modifier_groups: &[],
            },
        ],
    ),
    (
        "Breads",
        7,
        &[
            SeedItem {
                name: "Butter Naan",
                price_paise: 6000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_TANDOOR_CODE,
                // T1b: a "Regular" variant added so this item can carry a
                // recipe — `recipe.menu_item_variant_id` is NOT NULL, so a
                // variant-less item structurally cannot be costed. See the
                // T1b comment block below for the full list and rationale.
                variants: &["Regular"],
                modifier_groups: &[("Extras", &[("Extra butter", 1500)])],
            },
            SeedItem {
                name: "Garlic Naan",
                price_paise: 7000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_TANDOOR_CODE,
                variants: &["Regular"],
                modifier_groups: &[],
            },
            SeedItem {
                name: "Tandoori Roti",
                price_paise: 4000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_TANDOOR_CODE,
                variants: &["Plain", "Butter"],
                modifier_groups: &[],
            },
            SeedItem {
                name: "Laccha Paratha",
                price_paise: 7000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_TANDOOR_CODE,
                variants: &[],
                modifier_groups: &[],
            },
        ],
    ),
    (
        "Beverages",
        8,
        &[
            SeedItem {
                name: "Masala Chai",
                price_paise: 4000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_BAR_CODE,
                variants: &["Regular"],
                modifier_groups: &[("Extras", &[("Extra strong", 0)])],
            },
            SeedItem {
                name: "Filter Coffee",
                price_paise: 5000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_BAR_CODE,
                variants: &[],
                modifier_groups: &[],
            },
            SeedItem {
                name: "Fresh Lime Soda",
                price_paise: 8000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_BAR_CODE,
                variants: &["Sweet", "Salted"],
                modifier_groups: &[],
            },
            SeedItem {
                name: "Sweet Lassi",
                price_paise: 9000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_BAR_CODE,
                variants: &["Sweet", "Mango"],
                modifier_groups: &[],
            },
            SeedItem {
                name: "Bottled Water 1L",
                price_paise: 2000,
                tax_profile_id: TAX_PROFILE_PACKAGED18_ID,
                hsn_sac: "2201",
                station_code: STATION_BAR_CODE,
                variants: &["Regular"],
                modifier_groups: &[],
            },
            SeedItem {
                name: "Packaged Fruit Juice",
                price_paise: 6000,
                tax_profile_id: TAX_PROFILE_PACKAGED18_ID,
                hsn_sac: "2202",
                station_code: STATION_BAR_CODE,
                variants: &["Mango", "Mixed"],
                modifier_groups: &[],
            },
            SeedItem {
                name: "Coca-Cola (can)",
                price_paise: 5000,
                tax_profile_id: TAX_PROFILE_AERATED40_ID,
                hsn_sac: "2202",
                station_code: STATION_BAR_CODE,
                variants: &["Regular"],
                modifier_groups: &[],
            },
            SeedItem {
                name: "Thums Up (can)",
                price_paise: 5000,
                tax_profile_id: TAX_PROFILE_AERATED40_ID,
                hsn_sac: "2202",
                station_code: STATION_BAR_CODE,
                variants: &["Regular"],
                modifier_groups: &[],
            },
        ],
    ),
    (
        "Desserts",
        9,
        &[
            SeedItem {
                name: "Gulab Jamun (2 pc)",
                price_paise: 8000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_DESSERT_CODE,
                variants: &[],
                modifier_groups: &[],
            },
            SeedItem {
                name: "Gajar Halwa",
                price_paise: 12000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_DESSERT_CODE,
                variants: &[],
                modifier_groups: &[("Extras", &[("Extra dry fruits", 3000)])],
            },
            SeedItem {
                name: "Kulfi",
                price_paise: 9000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_DESSERT_CODE,
                variants: &["Malai", "Pista"],
                modifier_groups: &[],
            },
        ],
    ),
];

// ---- Billing / acceptance fixtures (opt-in, HOLLER_SEED_BILLING=1) ----
//
// OPT-IN ON PURPOSE. `tests/e2e-scenario/harness` invokes this binary and
// then seeds its OWN billing config (its own fiscal profile, its own active
// SALES series). If these rows were unconditional, that outlet would carry
// two active SALES series and two effective fiscal profiles, and
// `issue_invoice_impl` picks a series with `.find()` — so which one an
// invoice numbered against would depend on row order. That is exactly the
// kind of silent nondeterminism the harness exists to catch, so the default
// stays off and the harness keeps seeding its own.
//
// Set HOLLER_SEED_BILLING=1 for a manual acceptance run of the POS, which
// otherwise fails at "Issue Bill" with NO_FISCAL_PROFILE_CONFIGURED — devseed
// has never seeded any of this.
const COMPLIANCE_VERSION_ID: &str = "0191a000-0000-7000-8000-000000000040";
const TAX_PROFILE_ID: &str = "0191a000-0000-7000-8000-000000000041";
const FISCAL_PROFILE_ID: &str = "0191a000-0000-7000-8000-000000000042";
const INVOICE_SERIES_ID: &str = "0191a000-0000-7000-8000-000000000043";
const DISCOUNT_PCT_ID: &str = "0191a000-0000-7000-8000-000000000044";
const DISCOUNT_SPOILAGE_ID: &str = "0191a000-0000-7000-8000-000000000045";
const DISCOUNT_MANAGER_ID: &str = "0191a000-0000-7000-8000-000000000046";
const PRINTER_BILL_ID: &str = "0191a000-0000-7000-8000-000000000047";
const PRINTER_KITCHEN_ID: &str = "0191a000-0000-7000-8000-000000000048";

/// Where the seeded printers point when no file sink is configured. A
/// deliberately non-existent device path: with `HOLLER_PRINTER_FILE_SINK_DIR`
/// set (the acceptance path) the transport never opens it, and without the
/// sink a print fails loudly as a FAILED `print_job` naming this address —
/// which is the honest outcome on a machine with no printer attached, and is
/// visible in the POS's own failed-print banner.
const UNATTACHED_DEVICE_PATH: &str = r"\\.\COM_HOLLER_NO_PRINTER_ATTACHED";

const CASHIER_EMAIL: &str = "cashier@holler.test";

/// Permissions for the seeded cashier, from the `Permission` enum in
/// packages/contracts/src/types/identity.ts. The edge stores the flattened
/// list for THIS outlet (§50.1, replace-not-merge).
/// Kept IDENTICAL to the list in `backend/cmd/devseed/main.go` -- a config pull
/// REPLACES this list rather than merging into it, so a permission seeded on
/// only one side disappears the first time the outlet syncs.
///
/// `procurement.manage` (M5) is what makes the receiving and purchase-return
/// surfaces reachable; `canManageProcurement` gates both. `procurement.approve`
/// is deliberately absent: the edge must never approve a purchase order, and
/// the POS consults that permission nowhere.
const CASHIER_PERMISSIONS: &str = r#"["order.create","order.modify","table.manage","inventory.manage","inventory.count","procurement.manage"]"#;

/// The buyer's flattened list, mirroring the BUYER role in
/// `backend/cmd/devseed`.
///
/// THE CEILING IS NOT HERE, AND CANNOT BE. `po_approval_limit_paise` lives on
/// `role`, and THERE IS NO `role` TABLE IN SQLITE AT ALL -- the edge flattens
/// permissions onto `app_user`. That is by design, not an omission: the edge
/// must never approve a purchase order, so it has no business holding the
/// amount that would let it decide. `procurement.approve` appears here only
/// because the edge caches faithfully what the cloud says about a user, and
/// the POS consults this permission nowhere
/// (`apps/pos/src/domain/procurement.ts`).
const BUYER_PERMISSIONS: &str =
    r#"["order.create","order.modify","procurement.manage","procurement.approve"]"#;

/// Fixed timestamp for seeded rows. A constant rather than "now" so re-running
/// the seeder produces an identical database — this crate has no clock
/// dependency and a dev fixture does not need a real one.
const SEEDED_AT: &str = "2026-08-09T00:00:00Z";

/// Config version the seeded rows claim. Matches what backend/cmd/devseed
/// writes to Postgres, so a later real config pull supersedes rather than
/// conflicts with these rows.
const CONFIG_VERSION: i64 = 1;

fn main() -> ExitCode {
    match run() {
        Ok(path) => {
            println!(
                "devseed: sealed edge database written to {}",
                path.display()
            );
            println!(
                "devseed: login as {CASHIER_EMAIL} with the password from backend/cmd/devseed"
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("devseed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<PathBuf, String> {
    let key_hex = require_env("HOLLER_DB_KEY_HEX")?;
    let password_hash = require_env("HOLLER_SEED_PASSWORD_HASH")?;
    let key = parse_key_hex(&key_hex)?;

    // Must match AppState::open in apps/pos/src-tauri/src/state.rs: the POS
    // reads <app_data_dir>/edge.db.enc, so the seeder must write exactly there
    // or the POS will create a second, empty database and login will fail.
    let data_dir = match env::var("HOLLER_EDGE_DATA_DIR") {
        Ok(d) => PathBuf::from(d),
        Err(_) => default_app_data_dir()?,
    };
    std::fs::create_dir_all(&data_dir).map_err(|e| format!("creating {data_dir:?}: {e}"))?;

    let sealed_path = data_dir.join("edge.db.enc");
    let plaintext_path = data_dir.join("edge.db");

    let db =
        Db::open(&sealed_path, &plaintext_path, key).map_err(|e| format!("opening db: {e}"))?;

    seed(&db, &password_hash).map_err(|e| format!("seeding: {e}"))?;

    // close() checkpoints, re-seals with a fresh nonce and wipes the plaintext
    // working copy. Skipping it would leave an unencrypted edge.db on disk.
    db.close().map_err(|e| format!("sealing db: {e}"))?;

    // Optional end-to-end check: reopen the sealed file and run the same
    // offline-login path the POS uses. This is what proves the Go-generated
    // Argon2id hash actually verifies here — a format mismatch between the
    // two implementations would otherwise only surface at the login screen.
    if let Ok(password) = env::var("HOLLER_SEED_PASSWORD") {
        let key = parse_key_hex(&key_hex)?;
        let db = Db::open(&sealed_path, &plaintext_path, key)
            .map_err(|e| format!("reopening for verification: {e}"))?;
        let result =
            repo::verify_offline_login(db.connection(), OUTLET_ID, CASHIER_EMAIL, &password);
        db.close()
            .map_err(|e| format!("resealing after verification: {e}"))?;

        let user = result.map_err(|e| format!("offline login verification FAILED: {e}"))?;
        println!(
            "devseed: verified offline login for {} (permissions: {})",
            user.email, user.permissions_json
        );
    }

    Ok(sealed_path)
}

fn seed(db: &Db, password_hash: &str) -> Result<(), holler_edge_database::DbError> {
    let conn = db.connection();

    repo::upsert_outlet(
        conn,
        &Outlet {
            id: OUTLET_ID.to_string(),
            brand_id: BRAND_ID.to_string(),
            name: "Pune Test Outlet".to_string(),
            timezone: "Asia/Kolkata".to_string(),
            config_version: CONFIG_VERSION,
            created_at: SEEDED_AT.to_string(),
            updated_at: SEEDED_AT.to_string(),
        },
    )?;

    repo::upsert_device(
        conn,
        &Device {
            id: DEVICE_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            kind: "POS".to_string(),
            name: "Dev Till 1".to_string(),
            last_seen_at: None,
            created_at: SEEDED_AT.to_string(),
        },
    )?;

    // KDS screen, so a developer's `edge/device` kds-lan-server bin and
    // apps/kds have a real seeded device to connect as (T12).
    repo::upsert_device(
        conn,
        &Device {
            id: KDS_DEVICE_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            kind: "KDS".to_string(),
            name: "Dev Kitchen Screen 1".to_string(),
            last_seen_at: None,
            created_at: SEEDED_AT.to_string(),
        },
    )?;

    // The hash is generated by backend/cmd/devseed via
    // internal/platform/crypto — one Argon2id implementation, verified here by
    // edge/database/src/auth.rs. The plaintext never reaches this process.
    repo::replace_app_user(
        conn,
        &AppUser {
            id: CASHIER_ID.to_string(),
            tenant_id: TENANT_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            email: CASHIER_EMAIL.to_string(),
            full_name: "Dev Cashier".to_string(),
            password_hash: password_hash.to_string(),
            pin_hash: None,
            is_active: true,
            permissions_json: CASHIER_PERMISSIONS.to_string(),
            config_version: CONFIG_VERSION,
            updated_at: SEEDED_AT.to_string(),
        },
    )?;

    repo::replace_app_user(
        conn,
        &AppUser {
            id: BUYER_ID.to_string(),
            tenant_id: TENANT_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            email: BUYER_EMAIL.to_string(),
            full_name: "Dev Buyer".to_string(),
            password_hash: password_hash.to_string(),
            pin_hash: None,
            is_active: true,
            permissions_json: BUYER_PERMISSIONS.to_string(),
            config_version: CONFIG_VERSION,
            updated_at: SEEDED_AT.to_string(),
        },
    )?;

    for (id, label) in [(TABLE_1_ID, "T1"), (TABLE_2_ID, "T2")] {
        repo::upsert_restaurant_table(
            conn,
            &RestaurantTable {
                id: id.to_string(),
                outlet_id: OUTLET_ID.to_string(),
                section: "Main".to_string(),
                label: label.to_string(),
                seat_count: 4,
                is_active: true,
                config_version: CONFIG_VERSION,
            },
        )?;
    }

    repo::upsert_station(
        conn,
        &Station {
            id: STATION_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            code: STATION_CODE.to_string(),
            name: "Main Kitchen".to_string(),
            sort_order: 1,
            is_active: true,
            config_version: CONFIG_VERSION,
        },
    )?;

    repo::upsert_menu_category(
        conn,
        &MenuCategory {
            id: CATEGORY_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            name: "Beverages".to_string(),
            sort_order: 1,
            config_version: CONFIG_VERSION,
        },
    )?;

    for (id, name, price) in [
        (ITEM_CHAI_ID, "Masala Chai", 4000),
        (ITEM_THALI_ID, "Veg Thali", 22000),
    ] {
        repo::upsert_menu_item(
            conn,
            &MenuItem {
                id: id.to_string(),
                outlet_id: OUTLET_ID.to_string(),
                category_id: CATEGORY_ID.to_string(),
                name: name.to_string(),
                base_price_paise: price,
                is_available: true,
                config_version: CONFIG_VERSION,
                tax_profile_id: None,
                // SAC 9963: restaurant/catering services — both items here
                // are prepared food/beverage, not packaged goods (ADR-016
                // 0.4.5 §3). Without this an invoice can never issue against
                // the dev-seeded catalogue.
                hsn_sac: Some("9963".to_string()),
            },
        )?;
        repo::replace_menu_item_stations(conn, id, &[STATION_ID.to_string()], CONFIG_VERSION)?;
    }

    repo::upsert_menu_item_variant(
        conn,
        &MenuItemVariant {
            id: VARIANT_ID.to_string(),
            menu_item_id: ITEM_CHAI_ID.to_string(),
            name: "Large".to_string(),
            price_delta_paise: 1500,
            // The only variant this item has — default by construction.
            is_default: true,
            config_version: CONFIG_VERSION,
        },
    )?;

    // One modifier group ("Sugar") with two options.
    for (id, option, delta) in [
        (MOD_LESS_SUGAR_ID, "Less Sugar", 0),
        (MOD_EXTRA_SUGAR_ID, "Extra Sugar", 500),
    ] {
        repo::upsert_menu_item_modifier(
            conn,
            &MenuItemModifier {
                id: id.to_string(),
                menu_item_id: ITEM_CHAI_ID.to_string(),
                group_name: "Sugar".to_string(),
                option_name: option.to_string(),
                price_delta_paise: delta,
                min_selection: 0,
                max_selection: 1,
                config_version: CONFIG_VERSION,
            },
        )?;
    }

    let menu_ids = seed_menu(conn)?;
    let inventory_ids = seed_inventory(conn)?;
    seed_recipes(conn, &menu_ids, &inventory_ids)?;
    seed_modifier_deltas(conn, &menu_ids, &inventory_ids)?;

    if env::var("HOLLER_SEED_BILLING").is_ok_and(|v| v == "1") {
        seed_billing(conn)?;
    }

    // Without a sync_state row the outbox has no cursor to advance against
    // once the sync worker is eventually wired up.
    repo::init_sync_state(conn, OUTLET_ID)?;

    Ok(())
}

/// Seeds the real dev menu from `HOLLER_DEV_MENU_SPEC.md`: 5 stations, 3
/// (rule-less — see `TAX_PROFILE_FOOD5_ID` doc comment) tax profiles, 8
/// categories and 39 items with their variants, modifier groups and
/// station routing. Unconditional (not gated by `HOLLER_SEED_BILLING`):
/// menu display, ordering and KOT routing need none of the billing
/// fixtures, and gating the catalogue itself behind that flag would leave
/// the default `devseed` run back at the 2-item placeholder this task
/// exists to retire.
/// Ids `seed_menu` minted, keyed by the spec's own names — how `seed_recipes`
/// and `seed_modifier_deltas` (T1b) address a specific variant/modifier
/// without re-deriving the sequence-based id scheme themselves. No
/// `item_id` map: nothing downstream needs to address a bare menu item by
/// name, only a specific (item, variant) or (item, group, option).
struct MenuIds {
    variant_id: std::collections::HashMap<(&'static str, &'static str), String>,
    modifier_id: std::collections::HashMap<(&'static str, &'static str, &'static str), String>,
}

fn seed_menu(conn: &rusqlite::Connection) -> Result<MenuIds, holler_edge_database::DbError> {
    for (id, code, name, sort_order) in [
        (STATION_TANDOOR_ID, STATION_TANDOOR_CODE, "Tandoor", 2),
        (
            STATION_MAIN_ID,
            STATION_MAIN_CODE,
            "Main Kitchen (Curries)",
            3,
        ),
        (STATION_CHAT_ID, STATION_CHAT_CODE, "Chat / Cold Counter", 4),
        (STATION_BAR_ID, STATION_BAR_CODE, "Bar / Beverages", 5),
        (STATION_DESSERT_ID, STATION_DESSERT_CODE, "Dessert", 6),
    ] {
        repo::upsert_station(
            conn,
            &Station {
                id: id.to_string(),
                outlet_id: OUTLET_ID.to_string(),
                code: code.to_string(),
                name: name.to_string(),
                sort_order,
                is_active: true,
                config_version: CONFIG_VERSION,
            },
        )?;
    }

    // GST 2.0 (post-Sept-2025) profiles. INCLUSIVE pricing_mode: the spec's
    // menu prices already include tax, per Indian restaurant convention —
    // this exercises the engine's inclusive-mode back-computation path,
    // which the legacy EXCLUSIVE GST_5 profile in `seed_billing` never did.
    for (id, code, name) in [
        (TAX_PROFILE_FOOD5_ID, "GST_FOOD_5", "GST 5% (food)"),
        (
            TAX_PROFILE_PACKAGED18_ID,
            "GST_PACKAGED_18",
            "GST 18% (packaged, non-aerated)",
        ),
        (
            TAX_PROFILE_AERATED40_ID,
            "GST_AERATED_40",
            "GST 40% (aerated/sweetened)",
        ),
    ] {
        repo::upsert_tax_profile(
            conn,
            &TaxProfile {
                id: id.to_string(),
                outlet_id: OUTLET_ID.to_string(),
                code: code.to_string(),
                name: name.to_string(),
                pricing_mode: "INCLUSIVE".to_string(),
                is_default: false,
                is_active: true,
                config_version: CONFIG_VERSION,
            },
        )?;
    }

    let mut category_seq = 0u32;
    let mut item_seq = 0u32;
    let mut variant_seq = 0u32;
    let mut modifier_seq = 0u32;

    let mut variant_id_by_name = std::collections::HashMap::new();
    let mut modifier_id_by_name = std::collections::HashMap::new();

    for (category_name, sort_order, items) in SEED_CATEGORIES {
        category_seq += 1;
        let category_id = menu_category_id(category_seq);
        repo::upsert_menu_category(
            conn,
            &MenuCategory {
                id: category_id.clone(),
                outlet_id: OUTLET_ID.to_string(),
                name: category_name.to_string(),
                sort_order: *sort_order,
                config_version: CONFIG_VERSION,
            },
        )?;

        for item in *items {
            item_seq += 1;
            let item_id = menu_item_id(item_seq);
            let station_id = match item.station_code {
                s if s == STATION_TANDOOR_CODE => STATION_TANDOOR_ID,
                s if s == STATION_MAIN_CODE => STATION_MAIN_ID,
                s if s == STATION_CHAT_CODE => STATION_CHAT_ID,
                s if s == STATION_BAR_CODE => STATION_BAR_ID,
                s if s == STATION_DESSERT_CODE => STATION_DESSERT_ID,
                other => {
                    return Err(holler_edge_database::DbError::InvalidInput(format!(
                        "devseed: unknown station code {other} for item {}",
                        item.name
                    )))
                }
            };

            repo::upsert_menu_item(
                conn,
                &MenuItem {
                    id: item_id.clone(),
                    outlet_id: OUTLET_ID.to_string(),
                    category_id: category_id.clone(),
                    name: item.name.to_string(),
                    base_price_paise: item.price_paise,
                    is_available: true,
                    config_version: CONFIG_VERSION,
                    tax_profile_id: Some(item.tax_profile_id.to_string()),
                    hsn_sac: Some(item.hsn_sac.to_string()),
                },
            )?;
            repo::replace_menu_item_stations(
                conn,
                &item_id,
                &[station_id.to_string()],
                CONFIG_VERSION,
            )?;

            for (variant_index, variant_name) in item.variants.iter().enumerate() {
                variant_seq += 1;
                let variant_id = menu_variant_id(variant_seq);
                repo::upsert_menu_item_variant(
                    conn,
                    &MenuItemVariant {
                        id: variant_id.clone(),
                        menu_item_id: item_id.clone(),
                        name: variant_name.to_string(),
                        // The spec names variant options but gives no price
                        // deltas for any of them (unlike modifiers, which
                        // always carry an explicit figure) — 0 is the
                        // representative dev value, not a claim that e.g.
                        // Half and Full cost the same at a real outlet.
                        // is_default: the first listed variant, so every
                        // seeded multi-variant item satisfies ADR-018 §2.1's
                        // "every menu item has at least one variant" via an
                        // explicit default rather than relying on the
                        // auto-created-Regular fallback this loop never hits.
                        price_delta_paise: 0,
                        is_default: variant_index == 0,
                        config_version: CONFIG_VERSION,
                    },
                )?;
                variant_id_by_name.insert((item.name, *variant_name), variant_id);
            }

            for (group_name, options) in item.modifier_groups {
                for (option_name, delta) in *options {
                    modifier_seq += 1;
                    let modifier_id = menu_modifier_id(modifier_seq);
                    repo::upsert_menu_item_modifier(
                        conn,
                        &MenuItemModifier {
                            id: modifier_id.clone(),
                            menu_item_id: item_id.clone(),
                            group_name: group_name.to_string(),
                            option_name: option_name.to_string(),
                            price_delta_paise: *delta,
                            min_selection: 0,
                            max_selection: 1,
                            config_version: CONFIG_VERSION,
                        },
                    )?;
                    modifier_id_by_name.insert((item.name, *group_name, *option_name), modifier_id);
                }
            }
        }
    }

    println!("devseed: seed menu — {item_seq} items across {category_seq} categories, 5 stations, 3 tax profiles (GST_FOOD_5/GST_PACKAGED_18/GST_AERATED_40)");
    Ok(MenuIds {
        variant_id: variant_id_by_name,
        modifier_id: modifier_id_by_name,
    })
}

/// Seeds `SEED_INVENTORY_ITEMS` and `SEED_ITEM_UNIT_CONVERSIONS` (T1b, ADR-018).
/// Unconditional, same rationale as `seed_menu`: the larder is config a
/// recipe needs to resolve against regardless of whether billing fixtures
/// are turned on. Returns the sku -> id map `seed_recipes`/
/// `seed_modifier_deltas` need to build their own foreign keys.
fn seed_inventory(
    conn: &rusqlite::Connection,
) -> Result<std::collections::HashMap<&'static str, String>, holler_edge_database::DbError> {
    let mut item_id_by_sku = std::collections::HashMap::new();

    for (seq, item) in SEED_INVENTORY_ITEMS.iter().enumerate() {
        let id = inventory_item_id(seq as u32 + 1);
        repo::upsert_inventory_item(
            conn,
            &InventoryItem {
                id: id.clone(),
                outlet_id: OUTLET_ID.to_string(),
                sku: item.sku.to_string(),
                name: item.name.to_string(),
                category: Some(item.category.to_string()),
                dimension: item.dimension.to_string(),
                reorder_level_micro: item.reorder_level_micro,
                par_level_micro: None,
                storage_location: None,
                is_active: true,
                yield_factor_ppm: 1_000_000, // identity; DEFERRED to M5 (0015)
                config_version: CONFIG_VERSION,
            },
        )?;
        item_id_by_sku.insert(item.sku, id);
    }

    for (seq, conv) in SEED_ITEM_UNIT_CONVERSIONS.iter().enumerate() {
        let inventory_item_id_for_sku = item_id_by_sku.get(conv.sku).ok_or_else(|| {
            holler_edge_database::DbError::InvalidInput(format!(
                "devseed: item_unit_conversion for unknown sku {}",
                conv.sku
            ))
        })?;
        repo::upsert_item_unit_conversion(
            conn,
            &ItemUnitConversion {
                id: item_unit_conversion_id(seq as u32 + 1),
                inventory_item_id: inventory_item_id_for_sku.clone(),
                pack_unit_label: conv.pack_unit_label.to_string(),
                source_dimension: conv.source_dimension.to_string(),
                numerator: conv.numerator,
                denominator: conv.denominator,
                config_version: CONFIG_VERSION,
            },
        )?;
    }

    println!(
        "devseed: seed inventory — {} items, {} unit conversions",
        SEED_INVENTORY_ITEMS.len(),
        SEED_ITEM_UNIT_CONVERSIONS.len()
    );
    Ok(item_id_by_sku)
}

/// Seeds the two internal sub-recipes, `SEED_RECIPES` (22 of the 39 menu
/// items — see that const's own doc comment for exactly which, and why the
/// other 17 are deliberately left without one), and every
/// `recipe_ingredient` row underneath them. Unconditional, same rationale
/// as `seed_menu`/`seed_inventory`.
fn seed_recipes(
    conn: &rusqlite::Connection,
    menu: &MenuIds,
    inventory: &std::collections::HashMap<&'static str, String>,
) -> Result<(), holler_edge_database::DbError> {
    // The hidden category + two carrier items/variants a sub-recipe binds
    // to (see ITEM_MAKHANI_GRAVY_ID's doc comment above). `is_available:
    // false` keeps them off any ordering UI that filters on it.
    repo::upsert_menu_category(
        conn,
        &MenuCategory {
            id: INTERNAL_CATEGORY_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            name: "Kitchen Prep (internal — not sold)".to_string(),
            sort_order: 99,
            config_version: CONFIG_VERSION,
        },
    )?;
    for (item_id, variant_id, name) in [
        (
            ITEM_MAKHANI_GRAVY_ID,
            VARIANT_MAKHANI_GRAVY_ID,
            "Makhani Gravy (internal batch)",
        ),
        (
            ITEM_ONION_TOMATO_BASE_ID,
            VARIANT_ONION_TOMATO_BASE_ID,
            "Onion-Tomato Masala Base (internal batch)",
        ),
    ] {
        repo::upsert_menu_item(
            conn,
            &MenuItem {
                id: item_id.to_string(),
                outlet_id: OUTLET_ID.to_string(),
                category_id: INTERNAL_CATEGORY_ID.to_string(),
                name: name.to_string(),
                base_price_paise: 0,
                is_available: false,
                config_version: CONFIG_VERSION,
                tax_profile_id: None,
                hsn_sac: None,
            },
        )?;
        repo::upsert_menu_item_variant(
            conn,
            &MenuItemVariant {
                id: variant_id.to_string(),
                menu_item_id: item_id.to_string(),
                name: "Batch".to_string(),
                price_delta_paise: 0,
                // Sole variant for this internal sub-recipe item.
                is_default: true,
                config_version: CONFIG_VERSION,
            },
        )?;
    }

    let sub_recipe_ids: std::collections::HashMap<&'static str, &'static str> = [
        ("MAKHANI_GRAVY", RECIPE_MAKHANI_GRAVY_ID),
        ("ONION_TOMATO_BASE", RECIPE_ONION_TOMATO_BASE_ID),
    ]
    .into_iter()
    .collect();

    let mut ingredient_seq = 0u32;
    let mut sub_recipe_ref_count = 0u32;

    let insert_ingredients = |conn: &rusqlite::Connection,
                              recipe_id_for_row: &str,
                              ingredients: &[Comp],
                              ingredient_seq: &mut u32,
                              sub_recipe_ref_count: &mut u32|
     -> Result<(), holler_edge_database::DbError> {
        for (sort_order, comp) in ingredients.iter().enumerate() {
            *ingredient_seq += 1;
            let (
                component_kind,
                inventory_item_id_val,
                sub_recipe_id_val,
                quantity_micro,
                quantity_dimension,
            ) = match comp {
                Comp::Item(sku, qty, dim) => {
                    let item_id = inventory.get(sku).ok_or_else(|| {
                        holler_edge_database::DbError::InvalidInput(format!(
                            "devseed: recipe_ingredient references unknown inventory sku {sku}"
                        ))
                    })?;
                    (
                        "ITEM".to_string(),
                        Some(item_id.clone()),
                        None,
                        *qty,
                        dim.to_string(),
                    )
                }
                Comp::Sub(key, qty, dim) => {
                    *sub_recipe_ref_count += 1;
                    let sub_id = sub_recipe_ids.get(key).ok_or_else(|| {
                        holler_edge_database::DbError::InvalidInput(format!(
                            "devseed: recipe_ingredient references unknown sub-recipe key {key}"
                        ))
                    })?;
                    (
                        "SUB_RECIPE".to_string(),
                        None,
                        Some((*sub_id).to_string()),
                        *qty,
                        dim.to_string(),
                    )
                }
            };
            repo::upsert_recipe_ingredient(
                conn,
                &RecipeIngredient {
                    id: recipe_ingredient_id(*ingredient_seq),
                    recipe_id: recipe_id_for_row.to_string(),
                    component_kind,
                    inventory_item_id: inventory_item_id_val,
                    sub_recipe_id: sub_recipe_id_val,
                    quantity_micro,
                    quantity_dimension,
                    yield_factor_ppm: 1_000_000, // identity; DEFERRED to M5 (0015)
                    sort_order: sort_order as i64,
                    config_version: CONFIG_VERSION,
                },
            )?;
        }
        Ok(())
    };

    // The two sub-recipes themselves.
    repo::upsert_recipe(
        conn,
        &Recipe {
            id: RECIPE_MAKHANI_GRAVY_ID.to_string(),
            menu_item_variant_id: VARIANT_MAKHANI_GRAVY_ID.to_string(),
            name: "Makhani Gravy".to_string(),
            recipe_version: 1,
            output_dimension: MAKHANI_GRAVY_OUTPUT_DIMENSION.to_string(),
            output_quantity_micro: MAKHANI_GRAVY_OUTPUT_MICRO,
            config_version: CONFIG_VERSION,
        },
    )?;
    insert_ingredients(
        conn,
        RECIPE_MAKHANI_GRAVY_ID,
        MAKHANI_GRAVY_INGREDIENTS,
        &mut ingredient_seq,
        &mut sub_recipe_ref_count,
    )?;

    repo::upsert_recipe(
        conn,
        &Recipe {
            id: RECIPE_ONION_TOMATO_BASE_ID.to_string(),
            menu_item_variant_id: VARIANT_ONION_TOMATO_BASE_ID.to_string(),
            name: "Onion-Tomato Masala Base".to_string(),
            recipe_version: 1,
            output_dimension: ONION_TOMATO_BASE_OUTPUT_DIMENSION.to_string(),
            output_quantity_micro: ONION_TOMATO_BASE_OUTPUT_MICRO,
            config_version: CONFIG_VERSION,
        },
    )?;
    insert_ingredients(
        conn,
        RECIPE_ONION_TOMATO_BASE_ID,
        ONION_TOMATO_BASE_INGREDIENTS,
        &mut ingredient_seq,
        &mut sub_recipe_ref_count,
    )?;

    // The 22 real dish recipes.
    let mut recipe_seq = 0u32;
    for r in SEED_RECIPES {
        recipe_seq += 1;
        let this_recipe_id = recipe_id(recipe_seq);
        let variant_id = menu
            .variant_id
            .get(&(r.item_name, r.variant_name))
            .ok_or_else(|| {
                holler_edge_database::DbError::InvalidInput(format!(
                    "devseed: recipe for {} ({}) references a variant that was never seeded",
                    r.item_name, r.variant_name
                ))
            })?;
        repo::upsert_recipe(
            conn,
            &Recipe {
                id: this_recipe_id.clone(),
                menu_item_variant_id: variant_id.clone(),
                name: r.item_name.to_string(),
                recipe_version: 1,
                output_dimension: "COUNT".to_string(),
                output_quantity_micro: pieces(1), // one serving
                config_version: CONFIG_VERSION,
            },
        )?;
        insert_ingredients(
            conn,
            &this_recipe_id,
            r.ingredients,
            &mut ingredient_seq,
            &mut sub_recipe_ref_count,
        )?;
    }

    println!(
        "devseed: seed recipes — {} recipes ({} dish + 2 internal sub-recipes), {} sub-recipe references, {} recipe_ingredient rows",
        SEED_RECIPES.len() + 2,
        SEED_RECIPES.len(),
        sub_recipe_ref_count,
        ingredient_seq
    );
    Ok(())
}

/// Seeds `SEED_MODIFIER_DELTAS` (T1b). Unconditional, same rationale as
/// the other T1b seed functions.
fn seed_modifier_deltas(
    conn: &rusqlite::Connection,
    menu: &MenuIds,
    inventory: &std::collections::HashMap<&'static str, String>,
) -> Result<(), holler_edge_database::DbError> {
    let mut seq = 0u32;
    for d in SEED_MODIFIER_DELTAS {
        seq += 1;
        let modifier_id = match d.lookup {
            ModifierLookup::LegacyExtraSugar => MOD_EXTRA_SUGAR_ID.to_string(),
            ModifierLookup::LegacyLessSugar => MOD_LESS_SUGAR_ID.to_string(),
            ModifierLookup::Named(item_name, group_name, option_name) => menu
                .modifier_id
                .get(&(item_name, group_name, option_name))
                .cloned()
                .ok_or_else(|| {
                    holler_edge_database::DbError::InvalidInput(format!(
                        "devseed: modifier_ingredient_delta references a modifier that was never seeded: {item_name}/{group_name}/{option_name}"
                    ))
                })?,
        };
        let inventory_item_id_val = inventory.get(d.sku).cloned().ok_or_else(|| {
            holler_edge_database::DbError::InvalidInput(format!(
                "devseed: modifier_ingredient_delta references unknown inventory sku {}",
                d.sku
            ))
        })?;
        repo::upsert_modifier_ingredient_delta(
            conn,
            &ModifierIngredientDelta {
                id: modifier_ingredient_delta_id(seq),
                menu_item_modifier_id: modifier_id,
                inventory_item_id: inventory_item_id_val,
                quantity_micro: d.quantity_micro,
                config_version: CONFIG_VERSION,
            },
        )?;
    }

    println!(
        "devseed: seed modifier ingredient deltas — {} rows ({} costed modifiers; most modifiers deliberately carry none)",
        SEED_MODIFIER_DELTAS.len(),
        SEED_MODIFIER_DELTAS.len()
    );
    Ok(())
}

/// The config a bill needs before one can be issued: a compliance version, a
/// GST 5% default tax profile (CGST 2.5% + SGST 2.5%), the outlet's fiscal
/// identity as printed on the invoice, an active SALES numbering series,
/// three discount definitions, and two printers with roles.
///
/// Opt-in — see the const block above for why the e2e harness must not get
/// these rows.
fn seed_billing(conn: &rusqlite::Connection) -> Result<(), holler_edge_database::DbError> {
    repo::upsert_compliance_version(
        conn,
        &ComplianceVersion {
            id: COMPLIANCE_VERSION_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            label: "GST dev".to_string(),
            effective_from: "2020-01-01T00:00:00Z".to_string(),
            notes: None,
            config_version: CONFIG_VERSION,
        },
    )?;

    // is_default = true and every seeded menu_item leaves tax_profile_id
    // NULL, so all of them resolve here through the tax engine's fallback.
    repo::upsert_tax_profile(
        conn,
        &TaxProfile {
            id: TAX_PROFILE_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            code: "GST_5".to_string(),
            name: "GST 5%".to_string(),
            pricing_mode: "EXCLUSIVE".to_string(),
            is_default: true,
            is_active: true,
            config_version: CONFIG_VERSION,
        },
    )?;
    for (component, rate_bps) in [("CGST", 250i64), ("SGST", 250i64)] {
        repo::upsert_tax_rule(
            conn,
            &TaxRule {
                id: format!("{TAX_PROFILE_ID}-{component}"),
                tax_profile_id: TAX_PROFILE_ID.to_string(),
                compliance_version_id: COMPLIANCE_VERSION_ID.to_string(),
                component: component.to_string(),
                rate_bps,
                effective_from: "2020-01-01T00:00:00Z".to_string(),
                effective_to: None,
                config_version: CONFIG_VERSION,
            },
        )?;
    }

    // Rates for the spec's three menu tax profiles (seed_menu creates the
    // profiles themselves unconditionally, but withholds their tax_rule
    // rows because a rule needs a compliance_version — see
    // TAX_PROFILE_FOOD5_ID's doc comment). Reusing COMPLIANCE_VERSION_ID
    // here rather than minting a second compliance_version is what keeps
    // `tax::resolve_compliance_version` unambiguous for this outlet: one
    // compliance version, four tax profiles hanging off it.
    for (profile_id, cgst_bps, sgst_bps) in [
        (TAX_PROFILE_FOOD5_ID, 250i64, 250i64),
        (TAX_PROFILE_PACKAGED18_ID, 900i64, 900i64),
        (TAX_PROFILE_AERATED40_ID, 2000i64, 2000i64),
    ] {
        for (component, rate_bps) in [("CGST", cgst_bps), ("SGST", sgst_bps)] {
            repo::upsert_tax_rule(
                conn,
                &TaxRule {
                    id: format!("{profile_id}-{component}"),
                    tax_profile_id: profile_id.to_string(),
                    compliance_version_id: COMPLIANCE_VERSION_ID.to_string(),
                    component: component.to_string(),
                    rate_bps,
                    effective_from: "2020-01-01T00:00:00Z".to_string(),
                    effective_to: None,
                    config_version: CONFIG_VERSION,
                },
            )?;
        }
    }

    // Fictional GSTIN/FSSAI: valid in FORMAT so the renderer and any
    // validation exercise real shapes, but registered to nobody. Never put a
    // real business's registration in a dev fixture.
    repo::upsert_outlet_fiscal_profile(
        conn,
        &OutletFiscalProfile {
            id: FISCAL_PROFILE_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            legal_name: "Holler Dev Kitchens Pvt Ltd".to_string(),
            trade_name: "Pune Test Outlet".to_string(),
            address_line1: "123 MG Road".to_string(),
            address_line2: Some("Camp".to_string()),
            city: "Pune".to_string(),
            state_code: "27".to_string(),
            state_name: "Maharashtra".to_string(),
            pincode: "411001".to_string(),
            gstin: "27AAAAA0000A1Z5".to_string(),
            fssai_number: Some("11522998000123".to_string()),
            invoice_footer_text: Some("Thank you — dev fixture, not a real bill".to_string()),
            effective_from: "2020-01-01T00:00:00Z".to_string(),
            config_version: CONFIG_VERSION,
        },
    )?;

    repo::upsert_invoice_series(
        conn,
        &InvoiceSeries {
            id: INVOICE_SERIES_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            code: "SALES".to_string(),
            prefix_template: "DEV/".to_string(),
            reset_policy: "NEVER".to_string(),
            padding_width: 6,
            is_active: true,
            config_version: CONFIG_VERSION,
        },
    )?;

    // Three definitions covering the apply path and both governance gates,
    // so a manual run can see a refusal as well as an application. The
    // seeded cashier holds order.create/order.modify/table.manage, so the
    // manager discount below is refusable BY CONSTRUCTION.
    repo::upsert_discount_definition(
        conn,
        &DiscountDefinition {
            id: DISCOUNT_PCT_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            code: "STAFF_10".to_string(),
            name: "Staff 10%".to_string(),
            scope: "LINE".to_string(),
            method: "PERCENT".to_string(),
            value_bps: Some(1000),
            value_paise: None,
            max_discount_paise: None,
            required_permission: None,
            requires_reason: false,
            is_active: true,
            effective_from: "2020-01-01T00:00:00Z".to_string(),
            effective_to: None,
            config_version: CONFIG_VERSION,
        },
    )?;
    repo::upsert_discount_definition(
        conn,
        &DiscountDefinition {
            id: DISCOUNT_SPOILAGE_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            code: "SPOILAGE".to_string(),
            name: "Spoilage write-off (Rs 5)".to_string(),
            scope: "LINE".to_string(),
            method: "AMOUNT".to_string(),
            value_bps: None,
            value_paise: Some(500),
            max_discount_paise: None,
            required_permission: None,
            requires_reason: true,
            is_active: true,
            effective_from: "2020-01-01T00:00:00Z".to_string(),
            effective_to: None,
            config_version: CONFIG_VERSION,
        },
    )?;
    repo::upsert_discount_definition(
        conn,
        &DiscountDefinition {
            id: DISCOUNT_MANAGER_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            code: "MANAGER_50".to_string(),
            name: "Manager 50% (needs order.void)".to_string(),
            scope: "LINE".to_string(),
            method: "PERCENT".to_string(),
            value_bps: Some(5000),
            value_paise: None,
            max_discount_paise: None,
            required_permission: Some("order.void".to_string()),
            requires_reason: false,
            is_active: true,
            effective_from: "2020-01-01T00:00:00Z".to_string(),
            effective_to: None,
            config_version: CONFIG_VERSION,
        },
    )?;

    // ESCPOS_USB pointed at a device that does not exist on a dev machine —
    // see UNATTACHED_DEVICE_PATH. With HOLLER_PRINTER_FILE_SINK_DIR set, the
    // transport is replaced before this address is ever opened.
    for (id, name) in [
        (PRINTER_BILL_ID, "Dev Bill Printer"),
        (PRINTER_KITCHEN_ID, "Dev Kitchen Printer"),
    ] {
        repo::upsert_printer(
            conn,
            &Printer {
                id: id.to_string(),
                outlet_id: OUTLET_ID.to_string(),
                name: name.to_string(),
                connection_kind: "ESCPOS_USB".to_string(),
                address: UNATTACHED_DEVICE_PATH.to_string(),
                paper_width_mm: 80,
                is_active: true,
                config_version: CONFIG_VERSION,
            },
        )?;
    }
    // printer_role (contracts 0.4.7): a printer with no role row is a
    // candidate for neither path, so these two rows are what make
    // `print_invoice` resolve at all.
    repo::replace_printer_roles(conn, PRINTER_BILL_ID, &["BILL".to_string()], CONFIG_VERSION)?;
    repo::replace_printer_roles(
        conn,
        PRINTER_KITCHEN_ID,
        &["KITCHEN".to_string()],
        CONFIG_VERSION,
    )?;
    repo::replace_station_printers(
        conn,
        STATION_ID,
        &[PRINTER_KITCHEN_ID.to_string()],
        CONFIG_VERSION,
    )?;

    println!("devseed: billing config seeded (HOLLER_SEED_BILLING=1)");
    println!("devseed:   tax GST 5% (CGST 2.5 + SGST 2.5), series DEV/, GSTIN 27AAAAA0000A1Z5");
    println!("devseed:   discounts STAFF_10 (applies), SPOILAGE (needs a reason), MANAGER_50 (needs order.void — the cashier lacks it)");
    println!("devseed:   printers: Dev Bill Printer [BILL], Dev Kitchen Printer [KITCHEN]");
    Ok(())
}

fn require_env(key: &'static str) -> Result<String, String> {
    env::var(key).map_err(|_| format!("environment variable {key} is required"))
}

/// Mirrors Tauri v2's `app_data_dir()` on Windows: `%APPDATA%\<identifier>`,
/// where the identifier is `com.holler.pos` from tauri.conf.json. Override
/// with HOLLER_EDGE_DATA_DIR if your Tauri version resolves it differently.
fn default_app_data_dir() -> Result<PathBuf, String> {
    let appdata = env::var("APPDATA")
        .map_err(|_| "APPDATA is not set; pass HOLLER_EDGE_DATA_DIR explicitly".to_string())?;
    Ok(PathBuf::from(appdata).join("com.holler.pos"))
}

/// Same 32-byte hex key parsing as apps/pos/src-tauri/src/state.rs, duplicated
/// rather than shared because that helper is private to the POS crate and this
/// is a dev-only tool that must not change POS code.
fn parse_key_hex(hex: &str) -> Result<EncryptionKey, String> {
    if hex.len() != 64 {
        return Err("HOLLER_DB_KEY_HEX must be exactly 64 hex characters (32 bytes)".to_string());
    }
    let mut bytes = [0u8; 32];
    for i in 0..32 {
        bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| "HOLLER_DB_KEY_HEX contains a non-hex character".to_string())?;
    }
    Ok(EncryptionKey::new(bytes))
}

// T1b: seeded data is only real coverage if it actually resolves — a
// fixture nobody has run through the resolver it exists to feed is exactly
// the "green on absent data" trap CLAUDE.md warns against. These run the
// REAL `seed` function (never a hand-rolled subset) against an in-memory
// database and drive the REAL `holler_edge_database::inventory::
// resolve_recipe_for_variant`.
#[cfg(test)]
mod t1b_seed_resolves_tests {
    use super::*;
    use holler_edge_database::inventory::{resolve_recipe_for_variant, GapReason, ResolveOutcome};

    fn seeded_db() -> Db {
        let db = Db::open_in_memory_for_tests().expect("open in-memory db");
        seed(&db, "unused-in-tests-hash").expect("seed");
        db
    }

    /// docs/spec/inventory.md's own worked example, through a real
    /// sub-recipe at a genuinely fractional multiplier (180/300 = 0.6).
    #[test]
    fn butter_chicken_resolves_through_the_makhani_gravy_sub_recipe() {
        let db = seeded_db();
        let variant_id = &db
            .connection()
            .query_row(
                "SELECT v.id FROM menu_item_variant v JOIN menu_item m ON m.id = v.menu_item_id \
                 WHERE m.name = 'Butter Chicken' AND v.name = 'Full'",
                [],
                |r| r.get::<_, String>(0),
            )
            .expect("Butter Chicken / Full variant exists");

        let outcome =
            resolve_recipe_for_variant(db.connection(), Some(variant_id), 1).expect("no DbError");
        let ResolveOutcome::Resolved(resolution) = outcome else {
            panic!("expected Butter Chicken to resolve, got {outcome:?}");
        };
        assert_eq!(resolution.recipe_name, "Butter Chicken");

        // Chicken: a plain ITEM row, 220 g direct.
        let chicken = resolution
            .leaves
            .iter()
            .find(|l| l.inventory_item_name == "Chicken (Curry Cut, Bone-In)")
            .expect("chicken leaf present");
        assert_eq!(chicken.applied_micro, grams(220));

        // Tomato: ONLY reachable through the Makhani Gravy sub-recipe,
        // scaled by 180/300 of the batch's 250 g -> 150 g exactly.
        let tomato = resolution
            .leaves
            .iter()
            .find(|l| l.inventory_item_name == "Tomato")
            .expect("tomato leaf present (via the sub-recipe)");
        assert_eq!(
            tomato.applied_micro, grams(150),
            "180/300 of the gravy batch's 250 g tomato must be exactly 150 g, not a rounded approximation"
        );

        // Cream: BOTH a direct Butter Chicken ingredient (30 ml) AND inside
        // the gravy (60 ml * 180/300 = 36 ml) — must sum, not overwrite.
        let cream = resolution
            .leaves
            .iter()
            .find(|l| l.inventory_item_name == "Fresh Cream")
            .expect("cream leaf present");
        assert_eq!(cream.applied_micro, millilitres(30) + millilitres(36));
    }

    /// A 2x order quantity scales every leaf by 2, including through the
    /// sub-recipe.
    #[test]
    fn butter_chicken_scales_by_order_quantity_through_the_sub_recipe() {
        let db = seeded_db();
        let variant_id: String = db
            .connection()
            .query_row(
                "SELECT v.id FROM menu_item_variant v JOIN menu_item m ON m.id = v.menu_item_id \
                 WHERE m.name = 'Butter Chicken' AND v.name = 'Full'",
                [],
                |r| r.get(0),
            )
            .expect("variant exists");
        let outcome =
            resolve_recipe_for_variant(db.connection(), Some(&variant_id), 2).expect("no DbError");
        let ResolveOutcome::Resolved(resolution) = outcome else {
            panic!("expected resolution");
        };
        let chicken = resolution
            .leaves
            .iter()
            .find(|l| l.inventory_item_name == "Chicken (Curry Cut, Bone-In)")
            .unwrap();
        assert_eq!(chicken.applied_micro, grams(440));
    }

    /// Bottled Water: the simplest recipe in the seed, a straight COUNT ->
    /// COUNT passthrough with no sub-recipe and no other ingredient.
    #[test]
    fn bottled_water_resolves_as_a_single_count_passthrough() {
        let db = seeded_db();
        let variant_id: String = db
            .connection()
            .query_row(
                "SELECT v.id FROM menu_item_variant v JOIN menu_item m ON m.id = v.menu_item_id \
                 WHERE m.name = 'Bottled Water 1L' AND v.name = 'Regular'",
                [],
                |r| r.get(0),
            )
            .expect("variant exists");
        let outcome =
            resolve_recipe_for_variant(db.connection(), Some(&variant_id), 3).expect("no DbError");
        let ResolveOutcome::Resolved(resolution) = outcome else {
            panic!("expected resolution");
        };
        assert_eq!(resolution.leaves.len(), 1);
        assert_eq!(resolution.leaves[0].inventory_item_name, "Bottled Water 1L");
        assert_eq!(resolution.leaves[0].applied_micro, pieces(3));
    }

    /// Chana Masala has a variant (0.5.0's own requirement) but was
    /// deliberately left without a recipe row for it — `NoRecipe`, not
    /// `NoVariant`.
    #[test]
    fn chana_masala_is_deliberately_a_no_recipe_gap() {
        let db = seeded_db();
        let variant_id: String = db
            .connection()
            .query_row(
                "SELECT v.id FROM menu_item_variant v JOIN menu_item m ON m.id = v.menu_item_id \
                 WHERE m.name = 'Chana Masala' AND v.name = 'Full'",
                [],
                |r| r.get(0),
            )
            .expect("Chana Masala DOES have a variant — the seed must not remove it");
        let outcome =
            resolve_recipe_for_variant(db.connection(), Some(&variant_id), 1).expect("no DbError");
        assert_eq!(outcome, ResolveOutcome::Gap(GapReason::NoRecipe));
    }

    /// Samosa has no variant at all (the spec itself gives it none) — an
    /// order line for it carries no `menu_item_variant_id`, which is
    /// `NoVariant`, structurally different from `Chana Masala`'s gap above.
    #[test]
    fn samosa_has_no_variant_at_all() {
        let db = seeded_db();
        let variant_count: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM menu_item_variant v JOIN menu_item m ON m.id = v.menu_item_id \
                 WHERE m.name = 'Samosa (2 pc)'",
                [],
                |r| r.get(0),
            )
            .expect("query succeeds");
        assert_eq!(variant_count, 0);
        let outcome = resolve_recipe_for_variant(db.connection(), None, 1).expect("no DbError");
        assert_eq!(outcome, ResolveOutcome::Gap(GapReason::NoVariant));
    }

    /// The signed modifier delta pair on the legacy Sugar group: positive
    /// for Extra Sugar, negative for Less Sugar, same inventory item.
    #[test]
    fn extra_and_less_sugar_deltas_are_signed_opposites_on_the_same_item() {
        let db = seeded_db();
        let (extra, less): (i64, i64) = (
            db.connection()
                .query_row(
                    "SELECT quantity_micro FROM modifier_ingredient_delta WHERE menu_item_modifier_id = ?1",
                    [MOD_EXTRA_SUGAR_ID],
                    |r| r.get(0),
                )
                .expect("extra sugar delta row exists"),
            db.connection()
                .query_row(
                    "SELECT quantity_micro FROM modifier_ingredient_delta WHERE menu_item_modifier_id = ?1",
                    [MOD_LESS_SUGAR_ID],
                    |r| r.get(0),
                )
                .expect("less sugar delta row exists"),
        );
        assert_eq!(extra, grams(8));
        assert_eq!(less, -grams(8));
    }

    /// Every seeded `recipe_ingredient.quantity_dimension` must agree with
    /// whatever it references (item or sub-recipe) — this seed authors
    /// consistent data on purpose (dimension-mismatch fixtures belong to
    /// `tests/inventory_recipe_resolution.rs`, not here), so every one of
    /// the 22 dish recipes must resolve cleanly with quantity > 0 and never
    /// hit `DimensionMismatch`.
    #[test]
    fn every_seeded_dish_recipe_resolves_cleanly() {
        let db = seeded_db();
        let mut stmt = db
            .connection()
            .prepare(
                "SELECT v.id, m.name FROM recipe r \
                 JOIN menu_item_variant v ON v.id = r.menu_item_variant_id \
                 JOIN menu_item m ON m.id = v.menu_item_id \
                 WHERE m.category_id != ?1",
            )
            .unwrap();
        let rows: Vec<(String, String)> = stmt
            .query_map([INTERNAL_CATEGORY_ID], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            rows.len(),
            SEED_RECIPES.len(),
            "every SEED_RECIPES row must have landed"
        );
        for (variant_id, name) in rows {
            let outcome = resolve_recipe_for_variant(db.connection(), Some(&variant_id), 1)
                .unwrap_or_else(|e| panic!("{name}: DbError: {e}"));
            let ResolveOutcome::Resolved(resolution) = outcome else {
                panic!("{name}: expected Resolved, got a gap: {outcome:?}");
            };
            assert!(
                !resolution.leaves.is_empty(),
                "{name}: resolved with zero leaves"
            );
            for leaf in &resolution.leaves {
                assert!(
                    leaf.applied_micro > 0,
                    "{name}: leaf {} applied a non-positive quantity",
                    leaf.inventory_item_name
                );
            }
        }
    }

    /// Counts `(item rows, variant instances)` out of `HOLLER_DEV_MENU_SPEC.md`'s
    /// own category tables — the same "read the spec, don't hand-copy it"
    /// discipline `tests/inventory_recipe_resolution.rs` already applies to
    /// the Butter Chicken worked example. A table row's "variants" column
    /// is either an em-dash (no variant) or a `/`-joined list (`Half/Full`
    /// counts as 2) — mirroring exactly how `SEED_CATEGORIES` below reads.
    fn count_spec_items_and_variants() -> (usize, usize) {
        let spec_path = format!(
            "{}/../../HOLLER_DEV_MENU_SPEC.md",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&spec_path)
            .unwrap_or_else(|e| panic!("could not read {spec_path}: {e}"));

        let mut item_count = 0usize;
        let mut variant_count = 0usize;
        let mut in_table = false;
        for line in text.lines() {
            if line.starts_with("| item ") {
                in_table = true;
                continue;
            }
            if !in_table {
                continue;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() || !trimmed.starts_with('|') {
                in_table = false;
                continue;
            }
            // The header/body separator row: only '|', '-' and whitespace.
            if trimmed
                .chars()
                .all(|c| c == '|' || c == '-' || c.is_whitespace())
            {
                continue;
            }
            let cols: Vec<&str> = trimmed
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect();
            item_count += 1;
            if let Some(variants_field) = cols.get(5) {
                if *variants_field != "—" && !variants_field.is_empty() {
                    variant_count += variants_field.split('/').count();
                }
            }
        }
        (item_count, variant_count)
    }

    /// Falsification target for spec/seed drift: `HOLLER_DEV_MENU_SPEC.md`
    /// and `SEED_CATEGORIES` below are two independently hand-maintained
    /// copies of the same 39-item menu, and nothing but this test would
    /// notice if someone edited one without the other — exactly the
    /// silent-drift failure mode CLAUDE.md's spec/seed rule exists to catch.
    /// The 39/28/50 literals are a snapshot of both sides AT THE TIME this
    /// guard was written (quoted here so a future diff on either side is
    /// visible without cross-referencing); a deliberate change to the menu
    /// updates all four numbers together, never just one.
    #[test]
    fn spec_and_seed_agree_on_item_and_variant_counts() {
        let (spec_items, spec_variants) = count_spec_items_and_variants();
        assert_eq!(
            spec_items, 39,
            "HOLLER_DEV_MENU_SPEC.md's item-row count changed — if deliberate, \
             update this guard's literals together with SEED_CATEGORIES"
        );
        assert_eq!(
            spec_variants, 50,
            "HOLLER_DEV_MENU_SPEC.md's variant-instance count changed — if \
             deliberate, update this guard's literals together with SEED_CATEGORIES"
        );

        let db = seeded_db();
        let seeded_items: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM menu_item WHERE category_id NOT IN (?1, ?2)",
                [CATEGORY_ID, INTERNAL_CATEGORY_ID],
                |r| r.get(0),
            )
            .expect("count spec-seeded menu_item rows");
        assert!(
            seeded_items > 0,
            "spec-seeded menu_item fixture did not land"
        );
        let seeded_variants: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM menu_item_variant v \
                 JOIN menu_item m ON m.id = v.menu_item_id \
                 WHERE m.category_id NOT IN (?1, ?2)",
                [CATEGORY_ID, INTERNAL_CATEGORY_ID],
                |r| r.get(0),
            )
            .expect("count spec-seeded menu_item_variant rows");
        assert!(
            seeded_variants > 0,
            "spec-seeded menu_item_variant fixture did not land"
        );

        assert_eq!(
            seeded_items as usize, spec_items,
            "devseed's SEED_CATEGORIES item count must match HOLLER_DEV_MENU_SPEC.md exactly"
        );
        assert_eq!(
            seeded_variants as usize, spec_variants,
            "devseed's SEED_CATEGORIES variant count must match HOLLER_DEV_MENU_SPEC.md exactly"
        );
    }
}
