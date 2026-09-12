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
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use holler_edge_database::crypto::EncryptionKey;
use holler_edge_database::inventory::{grams, kilograms, litres, millilitres, pieces};
use holler_edge_database::model::{
    AppUser, ComplianceVersion, Device, DiscountDefinition, InventoryItem, InvoiceSeries,
    ItemUnitConversion, MenuCategory, MenuItem, MenuItemModifier, MenuItemVariant,
    ModifierIngredientDelta, NewGoodsReceiptNote, NewGrnLine, NewStockCount, NewStockCountLine,
    Outlet, OutletFiscalProfile, Printer, Recipe, RecipeIngredient, RestaurantTable, Station,
    SupplierConfig, SupplierItemConfig, TaxProfile, TaxRule,
};
use holler_edge_database::{repo, Db, DbError};
use rusqlite::OptionalExtension;
use serde_json::{json, Value};

/// The client menu catalogue, generated from the client's workbook. It lives in a
/// module DIRECTORY (`src/bin/devseed/`) rather than beside this file,
/// because a loose `.rs` under `src/bin/` is auto-discovered by cargo as its
/// own binary target and would fail to build for want of a `main`.
#[path = "devseed/client_menu.rs"]
mod client_menu;

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
/// Veg Thali's only variant. ADDITIVE and zero-delta: the legacy fixture's
/// id, its 22000-paise price and its routing are untouched, which is what
/// `tests/e2e-scenario/harness` pins. It exists because a variant-less item
/// cannot be ordered from `apps/captain` at all (it refuses the line as a
/// seed defect) and cannot carry a recipe, since `recipe` binds to
/// `menu_item_variant_id` — so such an item silently deducts no stock.
const VARIANT_THALI_ID: &str = "0191a000-0000-7000-8000-000000000016";
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
// Work item 2 (demo build): supplier/supplier_item/GRN/GRN-line ids, in
// their own disjoint range, same deterministic scheme.
fn supplier_item_id(seq: u32) -> String {
    format!("0191e860-0000-7000-8000-{seq:012x}")
}
fn grn_line_seed_id(seq: u32) -> String {
    format!("0191e870-0000-7000-8000-{seq:012x}")
}
/// `tax_rule.id` is `UUID PRIMARY KEY` in Postgres
/// (`packages/contracts/postgres/0007_m3_billing.sql`). It was previously
/// `format!("{tax_profile_id}-{component}")`, which SQLite stores as an
/// opaque TEXT primary key without complaint but Postgres rejects outright
/// (`22P02 invalid input syntax for type uuid`) -- caught only by running
/// the cloud seeder against the real store, never by anything SQLite-backed.
/// Own disjoint range, same deterministic seq -> id scheme as every other
/// id in this file.
fn tax_rule_id(seq: u32) -> String {
    format!("0191e590-0000-7000-8000-{seq:012x}")
}
fn stock_ledger_entry_seed_id(seq: u32) -> String {
    format!("0191e880-0000-7000-8000-{seq:012x}")
}

/// Round half away from zero, exact `i128` rational — mirrors
/// `crate::inventory::round_ratio_half_away_from_zero` (not reused directly:
/// that function is `pub(crate)` to `holler_edge_database` and this binary
/// only sees its `pub` surface). Only ever called here with non-negative
/// inputs (money and quantity, never negative in this file's own fixture
/// data), so the simpler unsigned-shaped implementation is exact for every
/// input this file actually produces.
fn round_half_away_from_zero(numerator: i128, denominator: i128) -> i128 {
    assert!(
        denominator > 0,
        "devseed: round with a non-positive denominator"
    );
    assert!(numerator >= 0, "devseed: round with a negative numerator");
    (numerator * 2 + denominator) / (denominator * 2)
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
const ITEM_GREEN_CURRY_PASTE_ID: &str = "0191e850-0000-7000-8000-000000000002";
const VARIANT_GREEN_CURRY_PASTE_ID: &str = "0191e850-0000-7000-8000-000000000003";
const ITEM_STONE_BOWL_SAUCE_ID: &str = "0191e850-0000-7000-8000-000000000004";
const VARIANT_STONE_BOWL_SAUCE_ID: &str = "0191e850-0000-7000-8000-000000000005";
const RECIPE_GREEN_CURRY_PASTE_ID: &str = "0191e850-0000-7000-8000-000000000006";
const RECIPE_STONE_BOWL_SAUCE_ID: &str = "0191e850-0000-7000-8000-000000000007";

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
/// Alcohol. The client's card taxes every bar line as VAT, and VAT has NO
/// expressible component under contracts 0.8.1 --
/// `tax_rule.component` is CHECKed to CGST/SGST/IGST/CESS. Rather than print
/// a real rate under a wrong label, this profile is CGST 0 / SGST 0: a bar
/// line is orderable and billable and NO WRONG TAX AMOUNT IS EVER CHARGED OR
/// PRINTED. `rate_bps INTEGER NOT NULL CHECK (rate_bps >= 0)` makes the zero
/// rate expressible without a contract change.
///
/// This is a demo accommodation with a visible hole, not a tax position: the
/// bar's real VAT is simply not collected. The VAT component itself is a
/// pilot-readiness contracts item -- any bar in India needs it.
const TAX_PROFILE_ALCOHOL_VAT_ID: &str = "0191e600-0000-7000-8000-000000000004";

/// The five stations the spec's KOT routing needs (ADR-014). Distinct from
/// the legacy STATION_ID/"MAIN_KITCHEN" above for the same harness-safety
/// reason as the tax profiles: nothing renames or reroutes a fixture the
/// harness already pins.
const STATION_KITCHEN_ID: &str = "0191e500-0000-7000-8000-000000000001";
const STATION_KITCHEN_CODE: &str = "KITCHEN";
const STATION_WOK_ID: &str = "0191e500-0000-7000-8000-000000000002";
const STATION_WOK_CODE: &str = "WOK";
const STATION_SUSHI_ID: &str = "0191e500-0000-7000-8000-000000000003";
const STATION_SUSHI_CODE: &str = "SUSHI_BAR";
/// Pinned by `tests/e2e-scenario/harness` as STATION_2_ID/STATION_2_CODE:
/// this id MUST keep the code "BAR". The client card routes its own bar to the
/// same station, so nothing had to move.
const STATION_BAR_ID: &str = "0191e500-0000-7000-8000-000000000004";
const STATION_BAR_CODE: &str = "BAR";
const STATION_DESSERT_ID: &str = "0191e500-0000-7000-8000-000000000005";
const STATION_DESSERT_CODE: &str = "DESSERT";
const STATION_DIMSUM_ID: &str = "0191e500-0000-7000-8000-000000000006";
const STATION_DIMSUM_CODE: &str = "DIMSUM";
const STATION_BEVERAGE_ID: &str = "0191e500-0000-7000-8000-000000000007";
const STATION_BEVERAGE_CODE: &str = "BEVERAGE";

/// One row per menu item. `variants` and `modifier_groups` carry
/// `(name, price_delta_paise)` pairs, read off the client's card by
/// `scripts/menu-to-seed.py`.
///
/// The card prints an ABSOLUTE price per variant while the contract stores
/// one `base_price_paise` per item plus a delta per variant, so the
/// generator takes the CHEAPEST printed variant as the base and every delta
/// is that variant's printed price minus the base. No delta is ever
/// negative, and every variant still rings up at its printed price.
struct SeedItem {
    name: &'static str,
    price_paise: i64,
    tax_profile_id: &'static str,
    hsn_sac: &'static str,
    station_code: &'static str,
    variants: &'static [(&'static str, i64)],
    modifier_groups: &'static [(&'static str, &'static [(&'static str, i64)])],
}

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
        sku: "INV-CHICKEN",
        name: "Chicken (Boneless, Diced)",
        category: "Meat & Poultry",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(12)),
    },
    SeedInventoryItem {
        sku: "INV-PRAWN",
        name: "Prawns (16/20, Peeled)",
        category: "Seafood",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(6)),
    },
    SeedInventoryItem {
        sku: "INV-LAMB",
        name: "Lamb (Leg, Diced)",
        category: "Meat & Poultry",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(5)),
    },
    SeedInventoryItem {
        sku: "INV-PORK",
        name: "Belgian Pork Belly",
        category: "Meat & Poultry",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(5)),
    },
    SeedInventoryItem {
        sku: "INV-TOFU",
        name: "Firm Tofu",
        category: "Vegetarian Protein",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(4)),
    },
    SeedInventoryItem {
        sku: "INV-SHIITAKE",
        name: "Shiitake Mushrooms",
        category: "Produce",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(3)),
    },
    SeedInventoryItem {
        sku: "INV-PAKCHOI",
        name: "Pak Choi",
        category: "Produce",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(4)),
    },
    SeedInventoryItem {
        sku: "INV-BAMBOO",
        name: "Bamboo Shoots",
        category: "Produce",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(3)),
    },
    SeedInventoryItem {
        sku: "INV-CORN",
        name: "Sweet Corn Kernels",
        category: "Produce",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(4)),
    },
    SeedInventoryItem {
        sku: "INV-SPINACH",
        name: "Spinach",
        category: "Produce",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(3)),
    },
    SeedInventoryItem {
        sku: "INV-PAPAYA",
        name: "Raw Papaya",
        category: "Produce",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(4)),
    },
    SeedInventoryItem {
        sku: "INV-ONION",
        name: "Onions",
        category: "Produce",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(10)),
    },
    SeedInventoryItem {
        sku: "INV-SPRINGONION",
        name: "Spring Onions",
        category: "Produce",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(2)),
    },
    SeedInventoryItem {
        sku: "INV-GINGARLIC",
        name: "Ginger-Garlic Paste",
        category: "Produce",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(3)),
    },
    SeedInventoryItem {
        sku: "INV-THAIBASIL",
        name: "Thai Basil",
        category: "Herbs & Aromatics",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(1)),
    },
    SeedInventoryItem {
        sku: "INV-LEMONGRASS",
        name: "Lemongrass",
        category: "Herbs & Aromatics",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(1)),
    },
    SeedInventoryItem {
        sku: "INV-GALANGAL",
        name: "Galangal",
        category: "Herbs & Aromatics",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(1)),
    },
    SeedInventoryItem {
        sku: "INV-GREENCHILLI",
        name: "Green Chillies (Bird Eye)",
        category: "Herbs & Aromatics",
        dimension: "MASS",
        reorder_level_micro: None,
    },
    SeedInventoryItem {
        sku: "INV-KIMCHI",
        name: "Kimchi",
        category: "Pickles & Ferments",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(4)),
    },
    SeedInventoryItem {
        sku: "INV-JASMINERICE",
        name: "Jasmine Rice",
        category: "Dry Store",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(25)),
    },
    SeedInventoryItem {
        sku: "INV-RICENOODLE",
        name: "Flat Rice Noodles",
        category: "Dry Store",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(8)),
    },
    SeedInventoryItem {
        sku: "INV-HAKKANOODLE",
        name: "Hakka Noodles",
        category: "Dry Store",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(8)),
    },
    SeedInventoryItem {
        sku: "INV-UDON",
        name: "Udon Noodles",
        category: "Dry Store",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(6)),
    },
    SeedInventoryItem {
        sku: "INV-PEANUT",
        name: "Roasted Peanuts",
        category: "Dry Store",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(3)),
    },
    SeedInventoryItem {
        sku: "INV-PALMSUGAR",
        name: "Palm Sugar",
        category: "Dry Store",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(3)),
    },
    SeedInventoryItem {
        sku: "INV-SUGAR",
        name: "Sugar",
        category: "Dry Store",
        dimension: "MASS",
        reorder_level_micro: None,
    },
    SeedInventoryItem {
        sku: "INV-SALT",
        name: "Salt",
        category: "Dry Store",
        dimension: "MASS",
        reorder_level_micro: None,
    },
    SeedInventoryItem {
        sku: "INV-TEALEAVES",
        name: "Black Tea Leaves",
        category: "Dry Store",
        dimension: "MASS",
        reorder_level_micro: Some(kilograms(2)),
    },
    SeedInventoryItem {
        sku: "INV-OIL",
        name: "Refined Oil",
        category: "Oils & Sauces",
        dimension: "VOLUME",
        reorder_level_micro: Some(litres(15)),
    },
    SeedInventoryItem {
        sku: "INV-SESAMEOIL",
        name: "Toasted Sesame Oil",
        category: "Oils & Sauces",
        dimension: "VOLUME",
        reorder_level_micro: Some(litres(3)),
    },
    SeedInventoryItem {
        sku: "INV-SOYSAUCE",
        name: "Light Soy Sauce",
        category: "Oils & Sauces",
        dimension: "VOLUME",
        reorder_level_micro: Some(litres(6)),
    },
    SeedInventoryItem {
        sku: "INV-FISHSAUCE",
        name: "Fish Sauce",
        category: "Oils & Sauces",
        dimension: "VOLUME",
        reorder_level_micro: Some(litres(4)),
    },
    SeedInventoryItem {
        sku: "INV-OYSTERSAUCE",
        name: "Oyster Sauce",
        category: "Oils & Sauces",
        dimension: "VOLUME",
        reorder_level_micro: Some(litres(4)),
    },
    SeedInventoryItem {
        sku: "INV-COCONUTMILK",
        name: "Coconut Milk",
        category: "Oils & Sauces",
        dimension: "VOLUME",
        reorder_level_micro: Some(litres(10)),
    },
    SeedInventoryItem {
        sku: "INV-EGGS",
        name: "Eggs",
        category: "Dairy & Eggs",
        dimension: "COUNT",
        reorder_level_micro: Some(pieces(60)),
    },
    SeedInventoryItem {
        sku: "INV-LIME",
        name: "Limes",
        category: "Produce",
        dimension: "COUNT",
        reorder_level_micro: Some(pieces(40)),
    },
    SeedInventoryItem {
        sku: "INV-COKECAN",
        name: "Coca-Cola Can",
        category: "Beverages",
        dimension: "COUNT",
        reorder_level_micro: Some(pieces(24)),
    },
    SeedInventoryItem {
        sku: "INV-BOTTLEDWATER",
        name: "Bottled Water 1L",
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
    // "1 sack Jasmine Rice = 25 kg".
    SeedItemUnitConversion {
        sku: "INV-JASMINERICE",
        pack_unit_label: "sack",
        source_dimension: "MASS",
        numerator: kilograms(25),
        denominator: 1,
    },
    // "1 packet Flat Rice Noodles = 400 g".
    SeedItemUnitConversion {
        sku: "INV-RICENOODLE",
        pack_unit_label: "packet",
        source_dimension: "MASS",
        numerator: grams(400),
        denominator: 1,
    },
    // "1 case Coconut Milk = 12 tins of 400 ml = 4.8 L" -- a same-dimension
    // (VOLUME) pack, unlike the oil tin below.
    SeedItemUnitConversion {
        sku: "INV-COCONUTMILK",
        pack_unit_label: "case",
        source_dimension: "VOLUME",
        numerator: millilitres(4800),
        denominator: 1,
    },
    // CROSS-DIMENSION, and the reason this column exists: refined oil is
    // stocked as VOLUME (it is measured in ml at the wok) but a tin is sold
    // and labelled by WEIGHT. One tin is 15 kg of oil at roughly 0.92 kg per
    // litre, so the volume is 15/0.92 L = 1500/92 L EXACTLY. Integer
    // numerator over integer denominator, no float and no pre-rounded
    // decimal -- the density lives in the ratio. A representative dev
    // density, not a claim about the client's actual supplier.
    SeedItemUnitConversion {
        sku: "INV-OIL",
        pack_unit_label: "tin",
        source_dimension: "MASS",
        numerator: litres(1500),
        denominator: 92,
    },
    // "1 crate Coca-Cola = 24 cans" -- a same-dimension (COUNT) pack.
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

/// The two internal sub-recipes (`GREEN_CURRY_PASTE`, `STONE_BOWL_SAUCE`), each
/// bound to its own hidden carrier item/variant (`seed_recipes` inserts
/// those directly — see `ITEM_GREEN_CURRY_PASTE_ID` etc above). Referenced by
/// root recipes below at a genuinely FRACTIONAL amount of their batch yield
/// (180 ml / 300 ml, 100 g / 500 g, ...) — never a 1x multiplier, which is
/// the exact case 0.5.1 was written to get right (see that migration's
/// header on why a multiplier-only encoding silently corrupts every parent
/// when the sub-recipe's own yield changes).
const GREEN_CURRY_PASTE_OUTPUT_DIMENSION: &str = "VOLUME";
/// 300 ml batch (`ml` scales ×1_000 — see this block's header).
const GREEN_CURRY_PASTE_OUTPUT_MICRO: i64 = millilitres(300);
const GREEN_CURRY_PASTE_INGREDIENTS: &[Comp] = &[
    Comp::Item("INV-GREENCHILLI", grams(60), "MASS"), // 60 g
    Comp::Item("INV-LEMONGRASS", grams(50), "MASS"),  // 50 g
    Comp::Item("INV-GALANGAL", grams(40), "MASS"),    // 40 g
    Comp::Item("INV-GINGARLIC", grams(45), "MASS"),   // 45 g
    Comp::Item("INV-THAIBASIL", grams(25), "MASS"),   // 25 g
    Comp::Item("INV-FISHSAUCE", millilitres(30), "VOLUME"), // 30 ml
    Comp::Item("INV-OIL", millilitres(40), "VOLUME"), // 40 ml
    Comp::Item("INV-SALT", grams(10), "MASS"),        // 10 g
];

const STONE_BOWL_SAUCE_OUTPUT_DIMENSION: &str = "VOLUME";
/// 480 ml batch (`ml` scales ×1_000 -- see this block's header).
const STONE_BOWL_SAUCE_OUTPUT_MICRO: i64 = millilitres(480);
const STONE_BOWL_SAUCE_INGREDIENTS: &[Comp] = &[
    Comp::Item("INV-SOYSAUCE", millilitres(180), "VOLUME"), // 180 ml
    Comp::Item("INV-OYSTERSAUCE", millilitres(120), "VOLUME"), // 120 ml
    Comp::Item("INV-SESAMEOIL", millilitres(60), "VOLUME"), // 60 ml
    Comp::Item("INV-PALMSUGAR", grams(80), "MASS"),         // 80 g
    Comp::Item("INV-GINGARLIC", grams(50), "MASS"),         // 50 g
    Comp::Item("INV-SPRINGONION", grams(40), "MASS"),       // 40 g
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
    // THE STOCK-DEDUCTION DISH the demo script uses (step 3): a Stone Bowl
    // through the sauce sub-recipe, at a genuinely fractional share of its
    // batch -- 60 ml of a 480 ml batch, never a 1x multiplier.
    SeedRecipe {
        item_name: "Kimchi",
        variant_name: "Chicken",
        ingredients: &[
            Comp::Item("INV-JASMINERICE", grams(180), "MASS"), // 180 g
            Comp::Item("INV-CHICKEN", grams(150), "MASS"),     // 150 g
            Comp::Item("INV-KIMCHI", grams(80), "MASS"),       // 80 g
            Comp::Item("INV-BAMBOO", grams(30), "MASS"),       // 30 g
            Comp::Item("INV-SPRINGONION", grams(15), "MASS"),  // 15 g
            Comp::Item("INV-EGGS", pieces(1), "COUNT"),        // 1 egg
            Comp::Sub("STONE_BOWL_SAUCE", millilitres(60), "VOLUME"), // 60 ml of a 480 ml batch
        ],
    },
    SeedRecipe {
        item_name: "Kimchi",
        variant_name: "Veg: Corn and Mushrooms",
        ingredients: &[
            Comp::Item("INV-JASMINERICE", grams(180), "MASS"), // 180 g
            Comp::Item("INV-CORN", grams(70), "MASS"),         // 70 g
            Comp::Item("INV-SHIITAKE", grams(60), "MASS"),     // 60 g
            Comp::Item("INV-KIMCHI", grams(80), "MASS"),       // 80 g
            Comp::Item("INV-SPRINGONION", grams(15), "MASS"),  // 15 g
            Comp::Sub("STONE_BOWL_SAUCE", millilitres(55), "VOLUME"), // 55 ml of a 480 ml batch
        ],
    },
    SeedRecipe {
        item_name: "Mala",
        variant_name: "Chicken",
        ingredients: &[
            Comp::Item("INV-JASMINERICE", grams(180), "MASS"), // 180 g
            Comp::Item("INV-CHICKEN", grams(150), "MASS"),     // 150 g
            Comp::Item("INV-BAMBOO", grams(40), "MASS"),       // 40 g
            Comp::Item("INV-GREENCHILLI", grams(8), "MASS"),   // 8 g
            Comp::Sub("STONE_BOWL_SAUCE", millilitres(60), "VOLUME"), // 60 ml of a 480 ml batch
        ],
    },
    // THE OTHER SUB-RECIPE PARENT: curry paste at 45 ml of a 300 ml batch.
    SeedRecipe {
        item_name: "Thai Green Curry",
        variant_name: "Chicken",
        ingredients: &[
            Comp::Item("INV-COCONUTMILK", millilitres(220), "VOLUME"), // 220 ml
            Comp::Item("INV-CHICKEN", grams(160), "MASS"),             // 160 g
            Comp::Item("INV-PAKCHOI", grams(50), "MASS"),              // 50 g
            Comp::Item("INV-THAIBASIL", grams(8), "MASS"),             // 8 g
            Comp::Sub("GREEN_CURRY_PASTE", millilitres(45), "VOLUME"), // 45 ml of a 300 ml batch
        ],
    },
    SeedRecipe {
        item_name: "Thai Green Curry",
        variant_name: "Prawn",
        ingredients: &[
            Comp::Item("INV-COCONUTMILK", millilitres(220), "VOLUME"), // 220 ml
            Comp::Item("INV-PRAWN", grams(140), "MASS"),               // 140 g
            Comp::Item("INV-PAKCHOI", grams(50), "MASS"),              // 50 g
            Comp::Sub("GREEN_CURRY_PASTE", millilitres(45), "VOLUME"), // 45 ml of a 300 ml batch
        ],
    },
    SeedRecipe {
        item_name: "Thai Green Curry",
        variant_name: "Veg",
        ingredients: &[
            Comp::Item("INV-COCONUTMILK", millilitres(220), "VOLUME"), // 220 ml
            Comp::Item("INV-TOFU", grams(120), "MASS"),                // 120 g
            Comp::Item("INV-PAKCHOI", grams(60), "MASS"),              // 60 g
            Comp::Sub("GREEN_CURRY_PASTE", millilitres(40), "VOLUME"), // 40 ml of a 300 ml batch
        ],
    },
    SeedRecipe {
        item_name: "Pad Thai",
        variant_name: "Regular",
        ingredients: &[
            Comp::Item("INV-RICENOODLE", grams(160), "MASS"), // 160 g
            Comp::Item("INV-EGGS", pieces(1), "COUNT"),       // 1 egg
            Comp::Item("INV-PEANUT", grams(20), "MASS"),      // 20 g
            Comp::Item("INV-PALMSUGAR", grams(12), "MASS"),   // 12 g
            Comp::Item("INV-FISHSAUCE", millilitres(20), "VOLUME"), // 20 ml
            Comp::Item("INV-SPRINGONION", grams(15), "MASS"), // 15 g
            Comp::Item("INV-LIME", pieces(1), "COUNT"),       // 1 lime
        ],
    },
    SeedRecipe {
        item_name: "Tom Yum",
        variant_name: "Prawn",
        ingredients: &[
            Comp::Item("INV-PRAWN", grams(120), "MASS"),     // 120 g
            Comp::Item("INV-LEMONGRASS", grams(15), "MASS"), // 15 g
            Comp::Item("INV-GALANGAL", grams(10), "MASS"),   // 10 g
            Comp::Item("INV-SHIITAKE", grams(40), "MASS"),   // 40 g
            Comp::Item("INV-FISHSAUCE", millilitres(20), "VOLUME"), // 20 ml
            Comp::Item("INV-LIME", pieces(1), "COUNT"),      // 1 lime
        ],
    },
    SeedRecipe {
        item_name: "Laksa",
        variant_name: "Chicken",
        ingredients: &[
            Comp::Item("INV-COCONUTMILK", millilitres(200), "VOLUME"), // 200 ml
            Comp::Item("INV-CHICKEN", grams(130), "MASS"),             // 130 g
            Comp::Item("INV-RICENOODLE", grams(120), "MASS"),          // 120 g
            Comp::Item("INV-EGGS", pieces(1), "COUNT"),                // 1 egg
        ],
    },
    SeedRecipe {
        item_name: "Som Tam",
        variant_name: "Regular",
        ingredients: &[
            Comp::Item("INV-PAPAYA", grams(220), "MASS"),   // 220 g
            Comp::Item("INV-PEANUT", grams(20), "MASS"),    // 20 g
            Comp::Item("INV-LIME", pieces(1), "COUNT"),     // 1 lime
            Comp::Item("INV-PALMSUGAR", grams(10), "MASS"), // 10 g
            Comp::Item("INV-FISHSAUCE", millilitres(15), "VOLUME"), // 15 ml
        ],
    },
    SeedRecipe {
        item_name: "Stir Fried Hakka Noodles",
        variant_name: "Regular",
        ingredients: &[
            Comp::Item("INV-HAKKANOODLE", grams(170), "MASS"), // 170 g
            Comp::Item("INV-PAKCHOI", grams(50), "MASS"),      // 50 g
            Comp::Item("INV-SOYSAUCE", millilitres(25), "VOLUME"), // 25 ml
            Comp::Item("INV-OIL", millilitres(20), "VOLUME"),  // 20 ml
        ],
    },
    SeedRecipe {
        item_name: "Yaki Udon",
        variant_name: "Regular",
        ingredients: &[
            Comp::Item("INV-UDON", grams(200), "MASS"),    // 200 g
            Comp::Item("INV-SHIITAKE", grams(50), "MASS"), // 50 g
            Comp::Item("INV-SOYSAUCE", millilitres(20), "VOLUME"), // 20 ml
            Comp::Item("INV-SESAMEOIL", millilitres(10), "VOLUME"), // 10 ml
        ],
    },
    SeedRecipe {
        item_name: "Burnt Garlic Corn And Spinach Fried Rice",
        variant_name: "Regular",
        ingredients: &[
            Comp::Item("INV-JASMINERICE", grams(200), "MASS"), // 200 g
            Comp::Item("INV-CORN", grams(60), "MASS"),         // 60 g
            Comp::Item("INV-SPINACH", grams(50), "MASS"),      // 50 g
            Comp::Item("INV-GINGARLIC", grams(15), "MASS"),    // 15 g
            Comp::Item("INV-OIL", millilitres(20), "VOLUME"),  // 20 ml
        ],
    },
    // THE SECOND DEMO ITEM (step 3's non-kitchen line): a drink that still
    // deducts, so the stock screen shows two unrelated items moving.
    SeedRecipe {
        item_name: "Iced Tea",
        variant_name: "Regular",
        ingredients: &[
            Comp::Item("INV-TEALEAVES", grams(8), "MASS"), // 8 g
            Comp::Item("INV-SUGAR", grams(15), "MASS"),    // 15 g
            Comp::Item("INV-LIME", pieces(1), "COUNT"),    // 1 lime
        ],
    },
    SeedRecipe {
        item_name: "Jasmine",
        variant_name: "Regular",
        ingredients: &[
            Comp::Item("INV-TEALEAVES", grams(6), "MASS"), // 6 g
        ],
    },
    // A COUNT passthrough: one can sold is one can gone, no conversion.
    SeedRecipe {
        item_name: "Aerated Water",
        variant_name: "Coke",
        ingredients: &[
            Comp::Item("INV-COKECAN", pieces(1), "COUNT"), // 1 piece
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
    // The legacy T0b "Sugar" group on the untouched ITEM_CHAI_ID fixture --
    // the SIGNED pair this table exists to prove: "Extra Sugar" consumes
    // MORE stock, "Less Sugar" consumes LESS (a negative delta), same item,
    // same inventory SKU. Kept through the client menu swap because the signed
    // path has no other seed coverage.
    SeedModifierDelta {
        lookup: ModifierLookup::LegacyExtraSugar,
        sku: "INV-SUGAR",
        quantity_micro: grams(8),
    }, // +8 g
    SeedModifierDelta {
        lookup: ModifierLookup::LegacyLessSugar,
        sku: "INV-SUGAR",
        quantity_micro: -grams(8),
    }, // -4 g
    // The card's three Staple add-ons, costed on two of the seven Staple
    // dishes. The other five carry the same modifiers with NO delta row at
    // all, which is legitimate (0015's header) and is the uncosted path this
    // seed also has to cover.
    SeedModifierDelta {
        lookup: ModifierLookup::Named("Pad Thai", "Add-on", "Add Chicken"),
        sku: "INV-CHICKEN",
        quantity_micro: grams(80),
    }, // +80 g
    SeedModifierDelta {
        lookup: ModifierLookup::Named("Pad Thai", "Add-on", "Add Prawns"),
        sku: "INV-PRAWN",
        quantity_micro: grams(70),
    }, // +70 g
    // "Add Mixed Meat" is TWO rows against one modifier: a delta is per
    // (modifier, inventory_item), so a mixed add-on is expressed as one row
    // per protein rather than a fabricated blended SKU.
    SeedModifierDelta {
        lookup: ModifierLookup::Named("Pad Thai", "Add-on", "Add Mixed Meat"),
        sku: "INV-CHICKEN",
        quantity_micro: grams(45),
    }, // +45 g
    SeedModifierDelta {
        lookup: ModifierLookup::Named("Pad Thai", "Add-on", "Add Mixed Meat"),
        sku: "INV-PRAWN",
        quantity_micro: grams(35),
    }, // +35 g
    SeedModifierDelta {
        lookup: ModifierLookup::Named("Stir Fried Hakka Noodles", "Add-on", "Add Chicken"),
        sku: "INV-CHICKEN",
        quantity_micro: grams(80),
    }, // +80 g
    SeedModifierDelta {
        lookup: ModifierLookup::Named("Stir Fried Hakka Noodles", "Add-on", "Add Prawns"),
        sku: "INV-PRAWN",
        quantity_micro: grams(70),
    }, // +70 g
];

/// The client menu catalogue, GENERATED from the client's own workbook by
/// `scripts/menu-to-seed.py` (see `seed/README.md`). Never hand-edited:
/// `client_menu_matches_the_generated_manifest` fails if either side moves
/// without the other.
///
/// sort_order starts at 2 in the generated data, so the legacy fixture
/// category (sort_order 1) still sorts first.
const SEED_CATEGORIES: &[(&str, i64, &[SeedItem])] = client_menu::CLIENT_CATEGORIES;

// ---- Demo build work item 2: supplier, pack sizes, opening stock, one
// received GRN (docs/demo-kickoff.md, seed/README.md). These rows are
// genuinely SHARED: `supplier`/`supplier_item` sync cloud->edge like every
// other procurement config row (ADR-019). The goods receipt is the
// deliberate exception `seed/README.md` documents — seeded directly into
// both stores from the same description, never by replay, because
// `edge/sync/src/route.rs` maps only `order`/`table_session` (carried gap
// A7) and a GRN would otherwise never reach the admin console this demo
// shows it in. Opening stock is likewise seeded directly (a stock count is
// edge-authoritative and has no cloud read surface this demo needs).
//
// DEV VALUES ONLY, same posture as the rest of this file: a production
// outlet configures its own supplier, pack sizes and prices.

const SUPPLIER_ID: &str = "0191a000-0000-7000-8000-000000000050";
const SUPPLIER_CODE: &str = "SUP-FRESHMART";
const SUPPLIER_NAME: &str = "FreshMart Wholesale Suppliers";
const SUPPLIER_GSTIN: &str = "27BBBBB1111B2Z6";

/// One `supplier_item`: a real pack size and a representative price. Also
/// the source for the single GRN below, entered as EXACTLY this pack size,
/// so every line CONVERTS cleanly — the only `grn_gap` the demo's receipt
/// produces is the expected, by-design `NO_PURCHASE_ORDER` gap (ADR-019: a
/// receipt with no PO is a legitimate walk-in delivery, accepted and
/// flagged, never a conversion defect).
struct SeedSupplierItem {
    sku: &'static str,
    purchase_unit: &'static str,
    pack_size_micro: i64,
    quantity_dimension: &'static str,
    last_price_paise: i64,
    /// Whole purchase units received on the one seeded GRN, x 1_000_000
    /// (contracts: `entered_quantity_micro` is micro-units of the ENTERED
    /// purchase unit, not of the item's base dimension).
    grn_entered_quantity_micro: i64,
}

const SEED_SUPPLIER_ITEMS: &[SeedSupplierItem] = &[
    SeedSupplierItem {
        sku: "INV-JASMINERICE",
        purchase_unit: "sack",
        pack_size_micro: kilograms(25),
        quantity_dimension: "MASS",
        last_price_paise: 310_000,             // Rs 3,100 / 25 kg sack
        grn_entered_quantity_micro: 2_000_000, // 2 sacks
    },
    SeedSupplierItem {
        sku: "INV-CHICKEN",
        purchase_unit: "kg",
        pack_size_micro: kilograms(1),
        quantity_dimension: "MASS",
        last_price_paise: 32_000,               // Rs 320 / kg
        grn_entered_quantity_micro: 20_000_000, // 20 kg
    },
    SeedSupplierItem {
        sku: "INV-PRAWN",
        purchase_unit: "kg",
        pack_size_micro: kilograms(1),
        quantity_dimension: "MASS",
        last_price_paise: 78_000,              // Rs 780 / kg
        grn_entered_quantity_micro: 8_000_000, // 8 kg
    },
    SeedSupplierItem {
        sku: "INV-RICENOODLE",
        purchase_unit: "packet",
        pack_size_micro: grams(400),
        quantity_dimension: "MASS",
        last_price_paise: 9_000,                // Rs 90 / 400 g packet
        grn_entered_quantity_micro: 24_000_000, // 24 packets
    },
    SeedSupplierItem {
        sku: "INV-COCONUTMILK",
        purchase_unit: "case",
        pack_size_micro: millilitres(4800),
        quantity_dimension: "VOLUME",
        last_price_paise: 168_000,             // Rs 1,680 / 12-tin case
        grn_entered_quantity_micro: 3_000_000, // 3 cases
    },
    SeedSupplierItem {
        sku: "INV-OIL",
        purchase_unit: "tin",
        pack_size_micro: litres(15),
        quantity_dimension: "VOLUME",
        last_price_paise: 195_000,             // Rs 1,950 / 15 L tin
        grn_entered_quantity_micro: 2_000_000, // 2 tins
    },
    SeedSupplierItem {
        sku: "INV-COKECAN",
        purchase_unit: "crate",
        pack_size_micro: pieces(24),
        quantity_dimension: "COUNT",
        last_price_paise: 96_000,              // Rs 960 / 24-can crate
        grn_entered_quantity_micro: 2_000_000, // 2 crates
    },
];

/// The single GRN's own header fields.
const GRN_ID: &str = "0191a000-0000-7000-8000-000000000051";
const GRN_RECEIVED_AT: &str = "2026-08-09T06:00:00Z";
const GRN_DELIVERY_NOTE_REF: &str = "DN-FRESHMART-0001";
/// The outlet-local business date `GRN_RECEIVED_AT` falls on (Asia/Kolkata,
/// day_start_time 05:00 — 06:00Z is 11:30 IST, well past the day start).
/// Matches `crate::procurement::numbering::next_grn_number`'s own
/// `(outlet_id, business_date)` key, so the edge's independently-minted
/// number and this JSON's number agree without either reading the other.
const GRN_BUSINESS_DATE: &str = "2026-08-09";
/// `procurement::numbering::format_grn_number(GRN_BUSINESS_DATE, 1)` — the
/// first receipt at this outlet on this business date, on a clean bootstrap.
const GRN_NUMBER: &str = "GRN/20260809/0001";

/// Opening stock: one `COUNT_ADJUSTMENT` line per inventory item, through the
/// same sanctioned public entry point a real physical count uses
/// (`Db::open_stock_count` / `add_or_update_stock_count_line` /
/// `Db::complete_stock_count`) — never a raw ledger insert (CLAUDE.md's
/// "enumerate the sinks" rule: `stock_ledger_entry` has exactly one non-test
/// INSERT, reached through four origins, and `COUNT_ADJUSTMENT` is the one
/// available to a caller outside this crate). A generous balance so no
/// recipe's deduction during a normal demo run walks any item negative:
/// 6x the reorder level where one is configured, else a flat per-dimension
/// default.
fn opening_stock_quantity_micro(dimension: &str, reorder_level_micro: Option<i64>) -> i64 {
    match reorder_level_micro {
        Some(reorder) => reorder.saturating_mul(6),
        None => match dimension {
            "VOLUME" => litres(15),
            "COUNT" => pieces(150),
            _ => kilograms(15), // MASS, and the fallback for any future dimension
        },
    }
}

const OPENING_STOCK_ID: &str = "0191a000-0000-7000-8000-000000000052";
const OPENING_STOCK_STARTED_AT: &str = "2026-08-09T05:30:00Z";
const OPENING_STOCK_COMPLETED_AT: &str = "2026-08-09T05:45:00Z";
/// Business date `OPENING_STOCK_STARTED_AT` falls on — same computation as
/// `GRN_BUSINESS_DATE` above.
const OPENING_STOCK_BUSINESS_DATE: &str = "2026-08-09";
/// THE RESTAURANT'S NAME, IN ONE PLACE, FOR THE WHOLE PRODUCT.
///
/// Holler is white-label: the till, the KDS, the captain page, the admin
/// console and the printed bill all show the RESTAURANT's name, and "Holler"
/// appears only as the product mark beside it. Every one of those surfaces
/// reads the name from the `outlet` (or `tenant`/`brand`) row this constant
/// seeds -- none of them hard-codes it -- so changing this line and re-seeding
/// renames the whole product.
///
/// The three names are separate constants rather than one string with
/// suffixes because they are genuinely three different things: the legal
/// entity that owns the GSTIN, the brand, and the trading name of this
/// particular outlet. A chain has one tenant, one brand and many outlet names.
const RESTAURANT_NAME: &str = "Shinjuku Yakitori";
/// The registered entity printed on a GST invoice. GSTIN-shaped placeholder
/// GSTIN (contracts-correct format, registered to nobody) -- the same posture
/// `seed_billing`'s existing `FISCAL_PROFILE_ID` fixture already takes.
const RESTAURANT_LEGAL_NAME: &str = "Shinjuku Yakitori Hospitality Pvt Ltd";
/// This outlet's trading name. One outlet today; a second would differ here
/// ("Shinjuku Yakitori — Koregaon Park") and nowhere else.
const OUTLET_NAME: &str = RESTAURANT_NAME;

// ---- Bill header, the other half of the single source ----------------------
//
// These print on the GST invoice and the receipt PDF, so they live beside the
// names rather than inline at the fiscal-profile upsert 1500 lines below.
// Changing a line here and re-seeding changes every bill.
//
// ADDRESS: Pune, Camp, 411001 -- the locality and pincode agree, and nothing
// here is invented. The street line the fixture used to carry ("123 MG Road")
// is GONE: a made-up street number on a client's bill reads as a fake document,
// which is worse than a shorter address. THE REAL REGISTERED ADDRESS REPLACES
// THESE THREE CONSTANTS AND NOTHING ELSE.
const OUTLET_ADDRESS_LINE1: &str = "Camp";
const OUTLET_ADDRESS_LINE2: Option<&str> = None;
const OUTLET_CITY: &str = "Pune";
const OUTLET_PINCODE: &str = "411001";
// Maharashtra. state_code is the GST state code and must be the first two
// digits of the GSTIN, or place-of-supply on the invoice is wrong.
const OUTLET_STATE_CODE: &str = "27";
const OUTLET_STATE_NAME: &str = "Maharashtra";
/// PLACEHOLDER, registered to nobody: contracts-correct FORMAT (2-digit state
/// code + PAN + entity digit + Z + checksum char) so the renderer and every
/// validation exercise a real shape. Never put a real business's registration
/// in a fixture, and never ship a demo with someone else's.
const OUTLET_GSTIN: &str = "27AAAAA0000A1Z5";
/// Placeholder in the same posture as the GSTIN.
const OUTLET_FSSAI: Option<&str> = Some("11522998000123");
/// Prints at the foot of every bill. Kept free of "dev", "fixture" and "test":
/// demo work item 6 is that no internal label reaches a screen the client sees,
/// and the receipt footer is a screen the client reads closely.
const INVOICE_FOOTER_TEXT: Option<&str> = Some("Thank you — please visit again");

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
    let args: Vec<String> = env::args().collect();
    if let Some(path_arg) = args.iter().position(|a| a == "--emit-json").map(|i| i + 1) {
        let Some(path) = args.get(path_arg) else {
            eprintln!("devseed: --emit-json requires a path argument");
            return ExitCode::FAILURE;
        };
        return match emit_json(PathBuf::from(path)) {
            Ok(path) => {
                println!("devseed: wrote shared catalogue to {}", path.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("devseed: {e}");
                ExitCode::FAILURE
            }
        };
    }

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

/// Builds the shared catalogue — exactly the "Shared" list in
/// `seed/README.md` — as a `serde_json::Value`, from the Rust seed structs
/// above (the authoring source). This is the ONLY place that walks
/// `SEED_CATEGORIES`/`SEED_INVENTORY_ITEMS`/`SEED_RECIPES`/
/// `SEED_MODIFIER_DELTAS`/`SEED_SUPPLIER_ITEMS` — [`seed_menu`],
/// [`seed_inventory`], [`seed_recipes`] and [`seed_modifier_deltas`] (called
/// from [`seed`]) read the emitted/committed JSON right back, never these
/// consts directly, so the edge and cloud stay fed by the same bytes
/// (seed/README.md).
///
/// Every id is minted by the same pure `seq -> id` functions the emitted
/// JSON's own ids come from, so re-emitting with unchanged inputs produces
/// byte-identical output — the property `scripts/check-seed-drift.mjs`
/// depends on.
///
/// NOTE on key order: `seed/README.md`'s "File format" section pins a key
/// order for human readability. `serde_json::Value`'s map type sorts keys
/// alphabetically on serialisation (this crate does not depend on
/// `serde_json`'s `preserve_order` feature — `Cargo.toml` is out of this
/// task's owned paths), so the emitted file's key order is alphabetical
/// rather than the literal order in that document. The file is still valid
/// JSON, still parses identically regardless of order, and re-emission is
/// still byte-stable — the drift check's actual guarantee — but a reader
/// diffing against that document's literal key order will see reordering.
fn build_shared_catalogue() -> Result<Value, String> {
    // ---- menu: categories, items, variants, modifiers ----
    // Order matches seed_menu/seed(): the two legacy T0b fixtures (fixed ids,
    // never renamed/repriced/rerouted -- tests/e2e-scenario/harness pins
    // them) first, then the spec's 8 categories, then the two hidden
    // sub-recipe carrier items.
    let mut menu_categories: Vec<Value> = Vec::new();
    let mut menu_items: Vec<Value> = Vec::new();
    let mut menu_item_variants: Vec<Value> = Vec::new();
    let mut menu_item_modifiers: Vec<Value> = Vec::new();

    // Legacy T0b fixture: CATEGORY_ID "Beverages", ITEM_CHAI_ID/ITEM_THALI_ID,
    // VARIANT_ID, MOD_LESS_SUGAR_ID/MOD_EXTRA_SUGAR_ID. Fixed ids, exact
    // values, never derived from a seq counter.
    // The legacy pair survives the client menu swap because
    // `tests/e2e-scenario/harness` pins these exact ids, that exact 4000-paise
    // price, that exact single-station routing and that exact
    // `tax_profile_id = None` fallback. They are NOT part of the client's card,
    // so both are `is_available: false` and their category sorts last under a
    // name that reads as internal: a greyed-out "Masala Chai" on a modern Asian
    // till is a blemish, an orderable one is a wrong menu.
    menu_categories.push(json!({
        "id": CATEGORY_ID, "outlet_id": OUTLET_ID,
        "name": "Test fixtures (internal -- not sold)", "sort_order": 98
    }));
    menu_items.push(json!({
        "id": ITEM_CHAI_ID, "outlet_id": OUTLET_ID, "category_id": CATEGORY_ID,
        "name": "Masala Chai", "base_price_paise": 4000, "is_available": false,
        "tax_profile_id": Value::Null, "hsn_sac": "9963", "station_code": STATION_CODE
    }));
    menu_items.push(json!({
        "id": ITEM_THALI_ID, "outlet_id": OUTLET_ID, "category_id": CATEGORY_ID,
        "name": "Veg Thali", "base_price_paise": 22000, "is_available": false,
        "tax_profile_id": Value::Null, "hsn_sac": "9963", "station_code": STATION_CODE
    }));
    menu_item_variants.push(json!({
        "id": VARIANT_ID, "menu_item_id": ITEM_CHAI_ID, "name": "Large",
        "price_delta_paise": 1500, "is_default": true
    }));
    menu_item_variants.push(json!({
        "id": VARIANT_THALI_ID, "menu_item_id": ITEM_THALI_ID, "name": "Regular",
        "price_delta_paise": 0, "is_default": true
    }));
    menu_item_modifiers.push(json!({
        "id": MOD_LESS_SUGAR_ID, "menu_item_id": ITEM_CHAI_ID, "group_name": "Sugar",
        "option_name": "Less Sugar", "price_delta_paise": 0, "min_selection": 0, "max_selection": 1
    }));
    menu_item_modifiers.push(json!({
        "id": MOD_EXTRA_SUGAR_ID, "menu_item_id": ITEM_CHAI_ID, "group_name": "Sugar",
        "option_name": "Extra Sugar", "price_delta_paise": 500, "min_selection": 0, "max_selection": 1
    }));

    let mut category_seq = 0u32;
    let mut item_seq = 0u32;
    let mut variant_seq = 0u32;
    let mut modifier_seq = 0u32;
    let mut variant_id_by_name: std::collections::HashMap<(&str, &str), String> =
        std::collections::HashMap::new();
    let mut modifier_id_by_name: std::collections::HashMap<(&str, &str, &str), String> =
        std::collections::HashMap::new();

    for (category_name, sort_order, items) in SEED_CATEGORIES {
        category_seq += 1;
        let category_id = menu_category_id(category_seq);
        menu_categories.push(json!({
            "id": category_id, "outlet_id": OUTLET_ID, "name": category_name,
            "sort_order": sort_order
        }));

        for item in *items {
            item_seq += 1;
            let item_id = menu_item_id(item_seq);
            menu_items.push(json!({
                "id": item_id, "outlet_id": OUTLET_ID, "category_id": category_id,
                "name": item.name, "base_price_paise": item.price_paise, "is_available": true,
                "tax_profile_id": item.tax_profile_id, "hsn_sac": item.hsn_sac,
                "station_code": item.station_code
            }));

            for (variant_index, (variant_name, price_delta_paise)) in
                item.variants.iter().enumerate()
            {
                variant_seq += 1;
                let variant_id = menu_variant_id(variant_seq);
                menu_item_variants.push(json!({
                    "id": variant_id, "menu_item_id": item_id, "name": variant_name,
                    "price_delta_paise": price_delta_paise, "is_default": variant_index == 0
                }));
                variant_id_by_name.insert((item.name, *variant_name), variant_id);
            }

            for (group_name, options) in item.modifier_groups {
                for (option_name, delta) in *options {
                    modifier_seq += 1;
                    let modifier_id = menu_modifier_id(modifier_seq);
                    menu_item_modifiers.push(json!({
                        "id": modifier_id, "menu_item_id": item_id, "group_name": group_name,
                        "option_name": option_name, "price_delta_paise": delta,
                        "min_selection": 0, "max_selection": 1
                    }));
                    modifier_id_by_name.insert((item.name, *group_name, *option_name), modifier_id);
                }
            }
        }
    }

    // The hidden category + two carrier items/variants a sub-recipe binds to
    // (ITEM_GREEN_CURRY_PASTE_ID's doc comment above). `is_available: false` --
    // never sold. `station_code`/`hsn_sac` still need real (non-blank)
    // values: both readers' wire types make these fields non-nullable
    // strings (`backend/cmd/devseed/seedfile.go`'s `seedMenuItem.HsnSac`/
    // `StationCode` are plain `string`, and the Go reader rejects a blank
    // hsn_sac outright), so these two never-orderable items borrow the
    // ordinary values rather than encoding "not applicable" as an empty
    // string the reader would treat as a real, wrong one.
    menu_categories.push(json!({
        "id": INTERNAL_CATEGORY_ID, "outlet_id": OUTLET_ID,
        "name": "Kitchen Prep (internal -- not sold)", "sort_order": 99
    }));
    for (item_id, variant_id, name) in [
        (
            ITEM_GREEN_CURRY_PASTE_ID,
            VARIANT_GREEN_CURRY_PASTE_ID,
            "Thai Green Curry Paste (internal batch)",
        ),
        (
            ITEM_STONE_BOWL_SAUCE_ID,
            VARIANT_STONE_BOWL_SAUCE_ID,
            "Stone Bowl Sauce Base (internal batch)",
        ),
    ] {
        menu_items.push(json!({
            "id": item_id, "outlet_id": OUTLET_ID, "category_id": INTERNAL_CATEGORY_ID,
            "name": name, "base_price_paise": 0, "is_available": false,
            "tax_profile_id": Value::Null, "hsn_sac": "9963",
            "station_code": STATION_WOK_CODE
        }));
        menu_item_variants.push(json!({
            "id": variant_id, "menu_item_id": item_id, "name": "Batch",
            "price_delta_paise": 0, "is_default": true
        }));
    }

    // ---- inventory items, unit conversions ----
    let mut inventory_items: Vec<Value> = Vec::new();
    let mut item_unit_conversions: Vec<Value> = Vec::new();
    let mut inventory_id_by_sku: std::collections::HashMap<&str, String> =
        std::collections::HashMap::new();

    for (seq, item) in SEED_INVENTORY_ITEMS.iter().enumerate() {
        let id = inventory_item_id(seq as u32 + 1);
        inventory_items.push(json!({
            "id": id, "outlet_id": OUTLET_ID, "sku": item.sku, "name": item.name,
            "category": item.category, "dimension": item.dimension,
            "reorder_level_micro": item.reorder_level_micro
        }));
        inventory_id_by_sku.insert(item.sku, id);
    }

    for (seq, conv) in SEED_ITEM_UNIT_CONVERSIONS.iter().enumerate() {
        let inventory_item_id_for_sku = inventory_id_by_sku.get(conv.sku).ok_or_else(|| {
            format!(
                "build_shared_catalogue: item_unit_conversion for unknown sku {}",
                conv.sku
            )
        })?;
        item_unit_conversions.push(json!({
            "id": item_unit_conversion_id(seq as u32 + 1),
            "inventory_item_id": inventory_item_id_for_sku,
            "pack_unit_label": conv.pack_unit_label, "source_dimension": conv.source_dimension,
            "numerator": conv.numerator, "denominator": conv.denominator
        }));
    }

    // ---- recipes, recipe_ingredients (two internal sub-recipes, then 22 dish recipes) ----
    // Neither `recipe.recipe_version` nor `recipe_ingredient.component_kind`/
    // `yield_factor_ppm`/`sort_order` are wire fields
    // (`backend/cmd/devseed/seedfile.go`'s `seedRecipe`/`seedRecipeIngredient`
    // carry none of them): recipe_version and yield_factor_ppm are always
    // identity/1 in this seed and each writer sets them itself;
    // component_kind is derived from which of inventory_item_id/sub_recipe_id
    // is set; sort_order is the array's own position.
    let mut recipes: Vec<Value> = Vec::new();
    let mut recipe_ingredients: Vec<Value> = Vec::new();
    let sub_recipe_ids: std::collections::HashMap<&'static str, &'static str> = [
        ("GREEN_CURRY_PASTE", RECIPE_GREEN_CURRY_PASTE_ID),
        ("STONE_BOWL_SAUCE", RECIPE_STONE_BOWL_SAUCE_ID),
    ]
    .into_iter()
    .collect();
    let mut ingredient_seq = 0u32;

    let push_ingredients = |recipe_id_for_row: &str,
                            ingredients: &[Comp],
                            ingredient_seq: &mut u32,
                            recipe_ingredients: &mut Vec<Value>|
     -> Result<(), String> {
        for comp in ingredients.iter() {
            *ingredient_seq += 1;
            let (inventory_item_id_val, sub_recipe_id_val, quantity_micro, dim) = match comp {
                Comp::Item(sku, qty, dim) => {
                    let item_id = inventory_id_by_sku.get(sku).ok_or_else(|| {
                        format!(
                            "build_shared_catalogue: recipe_ingredient references unknown inventory sku {sku}"
                        )
                    })?;
                    (Some(item_id.clone()), None, *qty, *dim)
                }
                Comp::Sub(key, qty, dim) => {
                    let sub_id = sub_recipe_ids.get(key).ok_or_else(|| {
                        format!(
                            "build_shared_catalogue: recipe_ingredient references unknown sub-recipe key {key}"
                        )
                    })?;
                    (None, Some(sub_id.to_string()), *qty, *dim)
                }
            };
            recipe_ingredients.push(json!({
                "id": recipe_ingredient_id(*ingredient_seq), "recipe_id": recipe_id_for_row,
                "inventory_item_id": inventory_item_id_val,
                "sub_recipe_id": sub_recipe_id_val, "quantity_micro": quantity_micro,
                "quantity_dimension": dim
            }));
        }
        Ok(())
    };

    recipes.push(json!({
        "id": RECIPE_GREEN_CURRY_PASTE_ID, "menu_item_variant_id": VARIANT_GREEN_CURRY_PASTE_ID,
        "name": "Makhani Gravy",
        "output_dimension": GREEN_CURRY_PASTE_OUTPUT_DIMENSION,
        "output_quantity_micro": GREEN_CURRY_PASTE_OUTPUT_MICRO
    }));
    push_ingredients(
        RECIPE_GREEN_CURRY_PASTE_ID,
        GREEN_CURRY_PASTE_INGREDIENTS,
        &mut ingredient_seq,
        &mut recipe_ingredients,
    )?;

    recipes.push(json!({
        "id": RECIPE_STONE_BOWL_SAUCE_ID, "menu_item_variant_id": VARIANT_STONE_BOWL_SAUCE_ID,
        "name": "Onion-Tomato Masala Base",
        "output_dimension": STONE_BOWL_SAUCE_OUTPUT_DIMENSION,
        "output_quantity_micro": STONE_BOWL_SAUCE_OUTPUT_MICRO
    }));
    push_ingredients(
        RECIPE_STONE_BOWL_SAUCE_ID,
        STONE_BOWL_SAUCE_INGREDIENTS,
        &mut ingredient_seq,
        &mut recipe_ingredients,
    )?;

    let mut recipe_seq = 0u32;
    for r in SEED_RECIPES {
        recipe_seq += 1;
        let this_recipe_id = recipe_id(recipe_seq);
        let variant_id = variant_id_by_name
            .get(&(r.item_name, r.variant_name))
            .ok_or_else(|| {
                format!(
                    "build_shared_catalogue: recipe for {} ({}) references a variant that was never seeded",
                    r.item_name, r.variant_name
                )
            })?;
        recipes.push(json!({
            "id": this_recipe_id, "menu_item_variant_id": variant_id, "name": r.item_name,
            "output_dimension": "COUNT", "output_quantity_micro": pieces(1)
        }));
        push_ingredients(
            &this_recipe_id,
            r.ingredients,
            &mut ingredient_seq,
            &mut recipe_ingredients,
        )?;
    }

    // ---- modifier_ingredient_deltas ----
    let mut modifier_ingredient_deltas: Vec<Value> = Vec::new();
    for (seq, d) in SEED_MODIFIER_DELTAS.iter().enumerate() {
        let modifier_id = match d.lookup {
            ModifierLookup::LegacyExtraSugar => MOD_EXTRA_SUGAR_ID.to_string(),
            ModifierLookup::LegacyLessSugar => MOD_LESS_SUGAR_ID.to_string(),
            ModifierLookup::Named(item_name, group_name, option_name) => modifier_id_by_name
                .get(&(item_name, group_name, option_name))
                .cloned()
                .ok_or_else(|| format!(
                    "build_shared_catalogue: modifier_ingredient_delta references a modifier that was never seeded: {item_name}/{group_name}/{option_name}"
                ))?,
        };
        let inventory_item_id_val = inventory_id_by_sku.get(d.sku).cloned().ok_or_else(|| {
            format!(
                "build_shared_catalogue: modifier_ingredient_delta references unknown inventory sku {}",
                d.sku
            )
        })?;
        modifier_ingredient_deltas.push(json!({
            "id": modifier_ingredient_delta_id(seq as u32 + 1),
            "menu_item_modifier_id": modifier_id, "inventory_item_id": inventory_item_id_val,
            "quantity_micro": d.quantity_micro
        }));
    }

    // ---- tax: the 3 GST 2.0 profiles menu items above reference, their
    // rules, and the one compliance_version they hang off. Shared per
    // seed/README.md; unconditional (not gated by HOLLER_SEED_BILLING) since
    // every spec menu item's tax_profile_id points at one of these.
    let compliance_versions = vec![json!({
        "id": COMPLIANCE_VERSION_ID, "outlet_id": OUTLET_ID, "label": "GST dev",
        "effective_from": "2020-01-01T00:00:00Z", "notes": Value::Null
    })];
    let mut tax_profiles: Vec<Value> = Vec::new();
    let mut tax_rules: Vec<Value> = Vec::new();
    let mut tax_rule_seq = 0u32;
    for (id, code, name, cgst_bps, sgst_bps) in [
        (
            TAX_PROFILE_FOOD5_ID,
            "GST_FOOD_5",
            "GST 5% (food)",
            250i64,
            250i64,
        ),
        (
            TAX_PROFILE_PACKAGED18_ID,
            "GST_PACKAGED_18",
            "GST 18% (packaged, non-aerated)",
            900,
            900,
        ),
        (
            TAX_PROFILE_AERATED40_ID,
            "GST_AERATED_40",
            "GST 40% (aerated/sweetened)",
            2000,
            2000,
        ),
        (
            TAX_PROFILE_ALCOHOL_VAT_ID,
            "ALCOHOL_VAT_UNCONFIGURED",
            "Alcohol - VAT not configured",
            0,
            0,
        ),
    ] {
        tax_profiles.push(json!({
            "id": id, "outlet_id": OUTLET_ID, "code": code, "name": name,
            "pricing_mode": "INCLUSIVE", "is_default": false, "is_active": true
        }));
        for (component, rate_bps) in [("CGST", cgst_bps), ("SGST", sgst_bps)] {
            tax_rule_seq += 1;
            tax_rules.push(json!({
                "id": tax_rule_id(tax_rule_seq), "tax_profile_id": id,
                "compliance_version_id": COMPLIANCE_VERSION_ID, "component": component,
                "rate_bps": rate_bps, "effective_from": "2020-01-01T00:00:00Z",
                "effective_to": Value::Null
            }));
        }
    }

    // ---- suppliers, supplier_items (demo build work item 2) ----
    let suppliers = vec![json!({
        "id": SUPPLIER_ID, "outlet_id": OUTLET_ID, "code": SUPPLIER_CODE, "name": SUPPLIER_NAME,
        "gstin": SUPPLIER_GSTIN, "phone": "+91-9800000000",
        "email": "orders@freshmart.example", "address": "Plot 14, MIDC, Bhosari, Pune",
        "payment_terms_days": 15, "is_active": true
    })];
    let mut supplier_items: Vec<Value> = Vec::new();
    for (seq, si) in SEED_SUPPLIER_ITEMS.iter().enumerate() {
        let inventory_item_id_val = inventory_id_by_sku.get(si.sku).ok_or_else(|| {
            format!(
                "build_shared_catalogue: supplier_item for unknown sku {}",
                si.sku
            )
        })?;
        supplier_items.push(json!({
            "id": supplier_item_id(seq as u32 + 1), "supplier_id": SUPPLIER_ID,
            "inventory_item_id": inventory_item_id_val, "purchase_unit": si.purchase_unit,
            "pack_size_micro": si.pack_size_micro, "quantity_dimension": si.quantity_dimension,
            "last_price_paise": si.last_price_paise, "is_preferred": true
        }));
    }

    // ---- the one received GRN (seed/README.md's deliberate exception) ----
    // Conversion computed HERE, once, by the same formulas
    // `crate::procurement::convert::resolve_line_conversion` uses (identity
    // yield, so `pack_size_micro_applied` is the supplier_item's own
    // `pack_size_micro` exactly): both readers do a literal insert of the
    // precomputed row, so this emitter is the one place that math runs for
    // the JSON's own numbers. The edge's own SQLite writer does NOT read
    // these fields back -- it re-derives the same values by calling
    // `Db::record_goods_receipt` (the real, tested conversion engine) against
    // the same supplier_item rows and the same entered quantities, which is
    // why `GRN_NUMBER`/`GRN_BUSINESS_DATE` above are chosen to equal what
    // that call independently mints, rather than being read out of this
    // struct.
    const MICRO: i128 = 1_000_000;
    let mut grn_lines: Vec<Value> = Vec::new();
    let mut grn_ledger_entries: Vec<Value> = Vec::new();
    for (seq, si) in SEED_SUPPLIER_ITEMS.iter().enumerate() {
        let line_number = seq as i64 + 1;
        let inv = SEED_INVENTORY_ITEMS
            .iter()
            .find(|i| i.sku == si.sku)
            .ok_or_else(|| {
                format!(
                    "build_shared_catalogue: grn line for unknown sku {}",
                    si.sku
                )
            })?;
        let inventory_item_id_val = inventory_id_by_sku.get(si.sku).cloned().ok_or_else(|| {
            format!(
                "build_shared_catalogue: grn line for unknown sku {}",
                si.sku
            )
        })?;
        let entered_quantity_micro = si.grn_entered_quantity_micro;
        let purchase_price_paise = si.last_price_paise;

        // Identity yield (every seeded inventory_item's yield_factor_ppm is
        // 1_000_000), so pack_size_micro_applied is the pack rate verbatim --
        // `procurement::convert::effective_rate`'s own exact-path case.
        let pack_size_micro_applied = si.pack_size_micro;
        let base_quantity_micro = i64::try_from(round_half_away_from_zero(
            i128::from(entered_quantity_micro) * i128::from(pack_size_micro_applied),
            MICRO,
        ))
        .map_err(|e| format!("build_shared_catalogue: base_quantity_micro overflow: {e}"))?;
        let line_total_paise = i64::try_from(round_half_away_from_zero(
            i128::from(entered_quantity_micro) * i128::from(purchase_price_paise),
            MICRO,
        ))
        .map_err(|e| format!("build_shared_catalogue: line_total_paise overflow: {e}"))?;
        let unit_cost_paise = i64::try_from(round_half_away_from_zero(
            i128::from(line_total_paise) * MICRO,
            i128::from(base_quantity_micro),
        ))
        .map_err(|e| format!("build_shared_catalogue: unit_cost_paise overflow: {e}"))?;

        grn_lines.push(json!({
            "id": grn_line_seed_id(seq as u32 + 1), "inventory_item_id": inventory_item_id_val,
            "line_number": line_number, "purchase_order_line_id": Value::Null,
            "entered_purchase_unit": si.purchase_unit,
            "entered_quantity_micro": entered_quantity_micro,
            "quantity_dimension": si.quantity_dimension,
            "base_quantity_micro": base_quantity_micro,
            "pack_size_micro_applied": pack_size_micro_applied,
            "unit_cost_paise": unit_cost_paise, "line_total_paise": line_total_paise,
            "batch_code": Value::Null, "expiry_date": Value::Null
        }));

        grn_ledger_entries.push(json!({
            "id": stock_ledger_entry_seed_id(seq as u32 + 1), "outlet_id": OUTLET_ID,
            "entry_seq": Value::Null, "inventory_item_id": inventory_item_id_val,
            "inventory_item_name": inv.name, "dimension": inv.dimension,
            "entry_type": "PURCHASE", "origin": "GOODS_RECEIPT",
            "quantity_micro": base_quantity_micro,
            "recipe_id": Value::Null, "recipe_version": Value::Null, "recipe_name": Value::Null,
            "reason_code": Value::Null, "note": Value::Null,
            "occurred_at": GRN_RECEIVED_AT, "business_date": GRN_BUSINESS_DATE,
            "created_by_user_id": CASHIER_ID,
            "modifier_delta_id": Value::Null, "modifier_name": Value::Null,
            "modifier_delta_version": Value::Null,
            "unit_cost_paise": unit_cost_paise, "line_total_paise": line_total_paise,
            "source_grn_id": GRN_ID, "source_purchase_return_id": Value::Null,
            "source_stock_transfer_out_id": Value::Null, "source_stock_count_id": Value::Null
        }));
    }
    let goods_receipt = json!({
        "id": GRN_ID, "outlet_id": OUTLET_ID, "purchase_order_id": Value::Null,
        "supplier_id": SUPPLIER_ID, "grn_number": GRN_NUMBER,
        "delivery_note_ref": GRN_DELIVERY_NOTE_REF,
        "received_at": GRN_RECEIVED_AT, "received_by_user_id": CASHIER_ID,
        "business_date": GRN_BUSINESS_DATE,
        "notes": "Opening delivery for the demo build", "lines": grn_lines,
        "ledger_entries": grn_ledger_entries
    });

    // ---- opening stock: one COUNT_ADJUSTMENT stock_ledger_entry per item ----
    let mut opening_stock: Vec<Value> = Vec::new();
    for (seq, item) in SEED_INVENTORY_ITEMS.iter().enumerate() {
        let inventory_item_id_val =
            inventory_id_by_sku.get(item.sku).cloned().ok_or_else(|| {
                format!(
                    "build_shared_catalogue: opening stock for unknown sku {}",
                    item.sku
                )
            })?;
        let quantity_micro = opening_stock_quantity_micro(item.dimension, item.reorder_level_micro);
        opening_stock.push(json!({
            "id": stock_ledger_entry_seed_id(1000 + seq as u32 + 1), "outlet_id": OUTLET_ID,
            "entry_seq": Value::Null, "inventory_item_id": inventory_item_id_val,
            "inventory_item_name": item.name, "dimension": item.dimension,
            "entry_type": "ADJUSTMENT", "origin": "COUNT_ADJUSTMENT",
            "quantity_micro": quantity_micro,
            "recipe_id": Value::Null, "recipe_version": Value::Null, "recipe_name": Value::Null,
            "reason_code": Value::Null, "note": "Opening stock for the demo build",
            "occurred_at": OPENING_STOCK_COMPLETED_AT, "business_date": OPENING_STOCK_BUSINESS_DATE,
            "created_by_user_id": CASHIER_ID,
            "modifier_delta_id": Value::Null, "modifier_name": Value::Null,
            "modifier_delta_version": Value::Null,
            "unit_cost_paise": Value::Null, "line_total_paise": Value::Null,
            "source_grn_id": Value::Null, "source_purchase_return_id": Value::Null,
            "source_stock_transfer_out_id": Value::Null,
            "source_stock_count_id": OPENING_STOCK_ID
        }));
    }

    Ok(json!({
        "schema_version": 1,
        "generated_by": "edge/database/src/bin/devseed.rs --emit-json",
        "tenant": { "id": TENANT_ID, "name": RESTAURANT_LEGAL_NAME },
        "brand": { "id": BRAND_ID, "tenant_id": TENANT_ID, "name": RESTAURANT_NAME },
        "outlet": {
            "id": OUTLET_ID, "brand_id": BRAND_ID, "name": OUTLET_NAME,
            "timezone": "Asia/Kolkata", "day_start_time": "05:00"
        },
        "tax_profiles": tax_profiles,
        "compliance_versions": compliance_versions,
        "tax_rules": tax_rules,
        "menu_categories": menu_categories,
        "menu_items": menu_items,
        "menu_item_variants": menu_item_variants,
        "menu_item_modifiers": menu_item_modifiers,
        "inventory_items": inventory_items,
        "item_unit_conversions": item_unit_conversions,
        "recipes": recipes,
        "recipe_ingredients": recipe_ingredients,
        "modifier_ingredient_deltas": modifier_ingredient_deltas,
        "suppliers": suppliers,
        "supplier_items": supplier_items,
        "goods_receipt": goods_receipt,
        "opening_stock": opening_stock,
    }))
}

/// `--emit-json <path>`: serialises [`build_shared_catalogue`] — the Rust
/// seed structs above, the authoring source — to `path`. Never touches
/// SQLite; does not require `HOLLER_DB_KEY_HEX`/`HOLLER_SEED_PASSWORD_HASH`.
/// See seed/README.md: this is the ONE emitter, and `seed/demo-outlet.json`
/// is the ONE committed artefact both seeders read.
fn emit_json(path: PathBuf) -> Result<PathBuf, String> {
    let catalogue = build_shared_catalogue()?;
    let mut text =
        serde_json::to_string_pretty(&catalogue).map_err(|e| format!("serialising: {e}"))?;
    text.push('\n');
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("creating {parent:?}: {e}"))?;
    }
    fs::write(&path, text).map_err(|e| format!("writing {path:?}: {e}"))?;
    Ok(path)
}

/// Locates the committed `seed/demo-outlet.json` relative to this crate
/// (`edge/database`), so the normal (non-`--emit-json`) seeding path works
/// regardless of the working directory `cargo run --bin devseed` is invoked
/// from.
fn shared_catalogue_path() -> PathBuf {
    if let Ok(p) = env::var("HOLLER_SEED_JSON_PATH") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("seed")
        .join("demo-outlet.json")
}

fn load_shared_catalogue() -> Result<Value, String> {
    let path = shared_catalogue_path();
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("reading committed seed file {path:?}: {e} — run `cargo run --bin devseed -- --emit-json {path:?}` first, or set HOLLER_SEED_JSON_PATH"))?;
    serde_json::from_str(&text).map_err(|e| format!("parsing {path:?}: {e}"))
}

fn run() -> Result<PathBuf, String> {
    let key_hex = require_env("HOLLER_DB_KEY_HEX")?;
    let password_hash = require_env("HOLLER_SEED_PASSWORD_HASH")?;
    let key = parse_key_hex(&key_hex)?;
    let catalogue = load_shared_catalogue()?;

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

    let mut db =
        Db::open(&sealed_path, &plaintext_path, key).map_err(|e| format!("opening db: {e}"))?;

    seed(&mut db, &password_hash, &catalogue).map_err(|e| format!("seeding: {e}"))?;

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

fn seed(
    db: &mut Db,
    password_hash: &str,
    catalogue: &Value,
) -> Result<(), holler_edge_database::DbError> {
    let conn = db.connection();

    repo::upsert_outlet(
        conn,
        &Outlet {
            id: OUTLET_ID.to_string(),
            brand_id: BRAND_ID.to_string(),
            name: OUTLET_NAME.to_string(),
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

    // Stations are EDGE ONLY (seed/README.md) -- never in the shared
    // catalogue. The legacy STATION_ID/"MAIN_KITCHEN" fixture first (fixed
    // id, pinned by tests/e2e-scenario/harness), then the five the spec
    // menu's `station_code` values route through.
    for (id, code, name, sort_order) in [
        (STATION_ID, STATION_CODE, "Main Kitchen", 1),
        (STATION_KITCHEN_ID, STATION_KITCHEN_CODE, "Kitchen", 2),
        (STATION_WOK_ID, STATION_WOK_CODE, "Wok", 3),
        (STATION_SUSHI_ID, STATION_SUSHI_CODE, "Sushi Bar", 4),
        (STATION_BAR_ID, STATION_BAR_CODE, "Bar", 5),
        (STATION_DESSERT_ID, STATION_DESSERT_CODE, "Dessert", 6),
        (STATION_DIMSUM_ID, STATION_DIMSUM_CODE, "Dimsum", 7),
        (STATION_BEVERAGE_ID, STATION_BEVERAGE_CODE, "Beverage", 8),
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

    // The menu (categories/items/variants/modifiers, including the legacy
    // T0b chai/thali fixture and the two internal sub-recipe carrier items),
    // inventory, recipes, modifier deltas and the shared tax config all come
    // from the committed catalogue now — never from a hand-written literal
    // here — so the edge and the cloud stay fed by the same bytes
    // (seed/README.md). `write_tax` is unconditional (not gated by
    // `HOLLER_SEED_BILLING`): every spec menu item's `tax_profile_id` points
    // at one of these three profiles, so the catalogue carries them
    // regardless of whether the legacy GST_5 billing fixture below is on.
    write_tax(conn, catalogue)?;
    write_inventory(conn, catalogue)?;
    write_menu(conn, catalogue)?;
    write_recipes(conn, catalogue)?;
    write_modifier_deltas(conn, catalogue)?;
    write_suppliers(conn, catalogue)?;
    write_goods_receipt(db, catalogue)?;
    write_opening_stock(db, catalogue)?;
    let conn = db.connection();

    if env::var("HOLLER_SEED_BILLING").is_ok_and(|v| v == "1") {
        seed_billing(conn)?;
    }

    // Without a sync_state row the outbox has no cursor to advance against
    // once the sync worker is eventually wired up.
    repo::init_sync_state(conn, OUTLET_ID)?;

    Ok(())
}

// ---- JSON extraction helpers for the committed catalogue ----
// Every seeding function below reads `Value` rows out of the catalogue
// loaded from `seed/demo-outlet.json` -- never the SEED_* consts directly
// (those feed [`build_shared_catalogue`] only). A missing/mistyped field is
// a typed `DbError::InvalidInput`, the same discipline this file already
// applies to a dangling sku/variant-name lookup.

fn jarr<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>, DbError> {
    v.get(key).and_then(|x| x.as_array()).ok_or_else(|| {
        DbError::InvalidInput(format!("devseed: catalogue missing array field {key}"))
    })
}

fn jstr(v: &Value, key: &str) -> Result<String, DbError> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            DbError::InvalidInput(format!(
                "devseed: catalogue row missing string field {key}: {v}"
            ))
        })
}

fn jstr_opt(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

fn ji64(v: &Value, key: &str) -> Result<i64, DbError> {
    v.get(key).and_then(|x| x.as_i64()).ok_or_else(|| {
        DbError::InvalidInput(format!(
            "devseed: catalogue row missing integer field {key}: {v}"
        ))
    })
}

fn ji64_opt(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(|x| x.as_i64())
}

fn jbool(v: &Value, key: &str) -> Result<bool, DbError> {
    v.get(key).and_then(|x| x.as_bool()).ok_or_else(|| {
        DbError::InvalidInput(format!(
            "devseed: catalogue row missing bool field {key}: {v}"
        ))
    })
}

/// The fixed station code -> station id map. Stations are EDGE ONLY
/// (seed/README.md) so `menu_item.station_code` -- the one field the shared
/// catalogue carries purely for this side -- is resolved against this fixed
/// map rather than anything in the JSON.
fn station_id_for_code(code: &str) -> Result<&'static str, DbError> {
    match code {
        STATION_CODE => Ok(STATION_ID),
        STATION_KITCHEN_CODE => Ok(STATION_KITCHEN_ID),
        STATION_WOK_CODE => Ok(STATION_WOK_ID),
        STATION_SUSHI_CODE => Ok(STATION_SUSHI_ID),
        STATION_BAR_CODE => Ok(STATION_BAR_ID),
        STATION_DESSERT_CODE => Ok(STATION_DESSERT_ID),
        STATION_DIMSUM_CODE => Ok(STATION_DIMSUM_ID),
        STATION_BEVERAGE_CODE => Ok(STATION_BEVERAGE_ID),
        other => Err(DbError::InvalidInput(format!(
            "devseed: menu_item.station_code {other} is not a known station"
        ))),
    }
}

/// Shared tax config: `compliance_versions`, `tax_profiles`, `tax_rules`.
/// Unconditional -- see `seed`'s own comment on why this is no longer
/// gated by `HOLLER_SEED_BILLING`.
fn write_tax(conn: &rusqlite::Connection, catalogue: &Value) -> Result<(), DbError> {
    for cv in jarr(catalogue, "compliance_versions")? {
        repo::upsert_compliance_version(
            conn,
            &ComplianceVersion {
                id: jstr(cv, "id")?,
                outlet_id: jstr(cv, "outlet_id")?,
                label: jstr(cv, "label")?,
                effective_from: jstr(cv, "effective_from")?,
                notes: jstr_opt(cv, "notes"),
                config_version: CONFIG_VERSION,
            },
        )?;
    }
    for tp in jarr(catalogue, "tax_profiles")? {
        repo::upsert_tax_profile(
            conn,
            &TaxProfile {
                id: jstr(tp, "id")?,
                outlet_id: jstr(tp, "outlet_id")?,
                code: jstr(tp, "code")?,
                name: jstr(tp, "name")?,
                pricing_mode: jstr(tp, "pricing_mode")?,
                is_default: jbool(tp, "is_default")?,
                is_active: jbool(tp, "is_active")?,
                config_version: CONFIG_VERSION,
            },
        )?;
    }
    for tr in jarr(catalogue, "tax_rules")? {
        repo::upsert_tax_rule(
            conn,
            &TaxRule {
                id: jstr(tr, "id")?,
                tax_profile_id: jstr(tr, "tax_profile_id")?,
                compliance_version_id: jstr(tr, "compliance_version_id")?,
                component: jstr(tr, "component")?,
                rate_bps: ji64(tr, "rate_bps")?,
                effective_from: jstr(tr, "effective_from")?,
                effective_to: jstr_opt(tr, "effective_to"),
                config_version: CONFIG_VERSION,
            },
        )?;
    }
    Ok(())
}

/// `inventory_items`, `item_unit_conversions`.
fn write_inventory(conn: &rusqlite::Connection, catalogue: &Value) -> Result<(), DbError> {
    let items = jarr(catalogue, "inventory_items")?;
    for item in items {
        repo::upsert_inventory_item(
            conn,
            &InventoryItem {
                id: jstr(item, "id")?,
                outlet_id: jstr(item, "outlet_id")?,
                sku: jstr(item, "sku")?,
                name: jstr(item, "name")?,
                category: jstr_opt(item, "category"),
                dimension: jstr(item, "dimension")?,
                reorder_level_micro: ji64_opt(item, "reorder_level_micro"),
                par_level_micro: None,
                storage_location: None,
                is_active: true,
                yield_factor_ppm: 1_000_000, // identity; DEFERRED to M5 (0015)
                config_version: CONFIG_VERSION,
            },
        )?;
    }
    for conv in jarr(catalogue, "item_unit_conversions")? {
        repo::upsert_item_unit_conversion(
            conn,
            &ItemUnitConversion {
                id: jstr(conv, "id")?,
                inventory_item_id: jstr(conv, "inventory_item_id")?,
                pack_unit_label: jstr(conv, "pack_unit_label")?,
                source_dimension: jstr(conv, "source_dimension")?,
                numerator: ji64(conv, "numerator")?,
                denominator: ji64(conv, "denominator")?,
                config_version: CONFIG_VERSION,
            },
        )?;
    }
    println!(
        "devseed: seed inventory — {} items, {} unit conversions",
        items.len(),
        jarr(catalogue, "item_unit_conversions")?.len()
    );
    Ok(())
}

/// `menu_categories`, `menu_items` (+ `menu_item_station` from
/// `station_code`, edge-only), `menu_item_variants`, `menu_item_modifiers`.
/// Covers the legacy T0b chai/thali fixture and the two internal sub-recipe
/// carrier items too -- both are rows in these same catalogue arrays now,
/// never a second hand-written path.
fn write_menu(conn: &rusqlite::Connection, catalogue: &Value) -> Result<(), DbError> {
    for cat in jarr(catalogue, "menu_categories")? {
        repo::upsert_menu_category(
            conn,
            &MenuCategory {
                id: jstr(cat, "id")?,
                outlet_id: jstr(cat, "outlet_id")?,
                name: jstr(cat, "name")?,
                sort_order: ji64(cat, "sort_order")?,
                config_version: CONFIG_VERSION,
            },
        )?;
    }

    let items = jarr(catalogue, "menu_items")?;
    for item in items {
        let id = jstr(item, "id")?;
        repo::upsert_menu_item(
            conn,
            &MenuItem {
                id: id.clone(),
                outlet_id: jstr(item, "outlet_id")?,
                category_id: jstr(item, "category_id")?,
                name: jstr(item, "name")?,
                base_price_paise: ji64(item, "base_price_paise")?,
                is_available: jbool(item, "is_available")?,
                config_version: CONFIG_VERSION,
                tax_profile_id: jstr_opt(item, "tax_profile_id"),
                hsn_sac: Some(jstr(item, "hsn_sac")?),
            },
        )?;
        let station_code = jstr(item, "station_code")?;
        let station_id = station_id_for_code(&station_code)?;
        repo::replace_menu_item_stations(conn, &id, &[station_id.to_string()], CONFIG_VERSION)?;
    }

    for variant in jarr(catalogue, "menu_item_variants")? {
        repo::upsert_menu_item_variant(
            conn,
            &MenuItemVariant {
                id: jstr(variant, "id")?,
                menu_item_id: jstr(variant, "menu_item_id")?,
                name: jstr(variant, "name")?,
                price_delta_paise: ji64(variant, "price_delta_paise")?,
                is_default: jbool(variant, "is_default")?,
                config_version: CONFIG_VERSION,
            },
        )?;
    }

    for modifier in jarr(catalogue, "menu_item_modifiers")? {
        repo::upsert_menu_item_modifier(
            conn,
            &MenuItemModifier {
                id: jstr(modifier, "id")?,
                menu_item_id: jstr(modifier, "menu_item_id")?,
                group_name: jstr(modifier, "group_name")?,
                option_name: jstr(modifier, "option_name")?,
                price_delta_paise: ji64(modifier, "price_delta_paise")?,
                min_selection: ji64(modifier, "min_selection")?,
                max_selection: ji64(modifier, "max_selection")?,
                config_version: CONFIG_VERSION,
            },
        )?;
    }

    println!(
        "devseed: seed menu — {} items across {} categories",
        items.len(),
        jarr(catalogue, "menu_categories")?.len()
    );
    Ok(())
}

/// `recipes`, then `recipe_ingredients` (recipes first: `recipe_ingredient.
/// recipe_id` is a real FK). `component_kind`/`yield_factor_ppm`/
/// `sort_order` are not wire fields -- derived/fixed here, exactly as
/// `backend/cmd/devseed/seedfile.go`'s comment on `seedRecipeIngredient`
/// describes for its own (mirrored) insert.
fn write_recipes(conn: &rusqlite::Connection, catalogue: &Value) -> Result<(), DbError> {
    let recipes = jarr(catalogue, "recipes")?;
    for r in recipes {
        repo::upsert_recipe(
            conn,
            &Recipe {
                id: jstr(r, "id")?,
                menu_item_variant_id: jstr(r, "menu_item_variant_id")?,
                name: jstr(r, "name")?,
                recipe_version: 1,
                output_dimension: jstr(r, "output_dimension")?,
                output_quantity_micro: ji64(r, "output_quantity_micro")?,
                config_version: CONFIG_VERSION,
            },
        )?;
    }

    let ingredients = jarr(catalogue, "recipe_ingredients")?;
    let mut sort_order_by_recipe: std::collections::HashMap<String, i64> =
        std::collections::HashMap::new();
    for ri in ingredients {
        let recipe_id_val = jstr(ri, "recipe_id")?;
        let inventory_item_id_val = jstr_opt(ri, "inventory_item_id");
        let sub_recipe_id_val = jstr_opt(ri, "sub_recipe_id");
        let component_kind = match (&inventory_item_id_val, &sub_recipe_id_val) {
            (Some(_), None) => "ITEM",
            (None, Some(_)) => "SUB_RECIPE",
            _ => {
                return Err(DbError::InvalidInput(format!(
                    "devseed: recipe_ingredient {} must set exactly one of inventory_item_id/sub_recipe_id",
                    jstr(ri, "id")?
                )))
            }
        };
        let sort_order = sort_order_by_recipe
            .entry(recipe_id_val.clone())
            .or_insert(0);
        repo::upsert_recipe_ingredient(
            conn,
            &RecipeIngredient {
                id: jstr(ri, "id")?,
                recipe_id: recipe_id_val,
                component_kind: component_kind.to_string(),
                inventory_item_id: inventory_item_id_val,
                sub_recipe_id: sub_recipe_id_val,
                quantity_micro: ji64(ri, "quantity_micro")?,
                quantity_dimension: jstr(ri, "quantity_dimension")?,
                yield_factor_ppm: 1_000_000, // identity; DEFERRED to M5 (0015)
                sort_order: *sort_order,
                config_version: CONFIG_VERSION,
            },
        )?;
        *sort_order += 1;
    }

    println!(
        "devseed: seed recipes — {} recipes, {} recipe_ingredient rows",
        recipes.len(),
        ingredients.len()
    );
    Ok(())
}

/// `modifier_ingredient_deltas`.
fn write_modifier_deltas(conn: &rusqlite::Connection, catalogue: &Value) -> Result<(), DbError> {
    let deltas = jarr(catalogue, "modifier_ingredient_deltas")?;
    for d in deltas {
        repo::upsert_modifier_ingredient_delta(
            conn,
            &ModifierIngredientDelta {
                id: jstr(d, "id")?,
                menu_item_modifier_id: jstr(d, "menu_item_modifier_id")?,
                inventory_item_id: jstr(d, "inventory_item_id")?,
                quantity_micro: ji64(d, "quantity_micro")?,
                config_version: CONFIG_VERSION,
            },
        )?;
    }
    println!(
        "devseed: seed modifier ingredient deltas — {} rows",
        deltas.len()
    );
    Ok(())
}

/// `suppliers`, `supplier_items` (demo build work item 2). `SupplierConfig`/
/// `SupplierItemConfig` are the CLOUD-OWNED config shapes (contracts 0.6.0)
/// -- exactly right here too, since a supplier and its pack sizes are
/// management config, cloud->edge, like every other row this function
/// writes (ADR-019).
fn write_suppliers(conn: &rusqlite::Connection, catalogue: &Value) -> Result<(), DbError> {
    for s in jarr(catalogue, "suppliers")? {
        repo::upsert_supplier(
            conn,
            &SupplierConfig {
                id: jstr(s, "id")?,
                outlet_id: jstr(s, "outlet_id")?,
                code: jstr(s, "code")?,
                name: jstr(s, "name")?,
                gstin: jstr_opt(s, "gstin"),
                phone: jstr_opt(s, "phone"),
                email: jstr_opt(s, "email"),
                address: jstr_opt(s, "address"),
                payment_terms_days: ji64(s, "payment_terms_days")?,
                is_active: jbool(s, "is_active")?,
                config_version: CONFIG_VERSION,
            },
        )?;
    }
    for si in jarr(catalogue, "supplier_items")? {
        repo::upsert_supplier_item(
            conn,
            &SupplierItemConfig {
                id: jstr(si, "id")?,
                supplier_id: jstr(si, "supplier_id")?,
                inventory_item_id: jstr(si, "inventory_item_id")?,
                purchase_unit: jstr(si, "purchase_unit")?,
                pack_size_micro: ji64(si, "pack_size_micro")?,
                quantity_dimension: jstr(si, "quantity_dimension")?,
                last_price_paise: ji64_opt(si, "last_price_paise"),
                is_preferred: jbool(si, "is_preferred")?,
            },
        )?;
    }
    Ok(())
}

/// The single received GRN (seed/README.md's deliberate exception).
/// **Does NOT read `goods_receipt.lines[]`'s precomputed conversion
/// fields.** Those exist for the cloud reader, which has no conversion
/// engine of its own; the edge re-derives the same numbers through
/// `Db::record_goods_receipt` -- the real, tested conversion path -- from
/// the entered quantities and the `supplier_item` rows [`write_suppliers`]
/// already wrote. `GRN_NUMBER`/`GRN_BUSINESS_DATE` are chosen (see their own
/// doc comments) to equal what that call mints on a clean bootstrap, so the
/// edge's own stored number still matches the JSON's without either side
/// reading the other.
fn write_goods_receipt(db: &mut Db, catalogue: &Value) -> Result<(), DbError> {
    let Some(receipt) = catalogue.get("goods_receipt").filter(|v| !v.is_null()) else {
        return Ok(());
    };

    // `Db::record_goods_receipt` is not an upsert: it mints a fresh GRN
    // number, appends `stock_ledger_entry` rows (insert-only, trigger-
    // guarded — see migrations.rs) and records `grn_gap` rows. None of that
    // can be safely re-run. A re-run of this seeder must not call it twice
    // for the same fixed `GRN_ID`, so check first and skip rather than let
    // the second run hit `goods_receipt_note`'s UNIQUE(id) and abort.
    let already_seeded: bool = db
        .connection()
        .query_row(
            "SELECT 1 FROM goods_receipt_note WHERE id = ?1",
            [GRN_ID],
            |_| Ok(()),
        )
        .optional()
        .map_err(|e| DbError::InvalidInput(format!("devseed: checking seeded GRN: {e}")))?
        .is_some();
    if already_seeded {
        println!("devseed: seed goods receipt — already present, skipping ({GRN_ID})");
        return Ok(());
    }

    let lines = jarr(receipt, "lines")?;
    let mut new_lines = Vec::with_capacity(lines.len());
    for l in lines {
        new_lines.push(NewGrnLine {
            inventory_item_id: jstr(l, "inventory_item_id")?,
            entered_purchase_unit: jstr(l, "entered_purchase_unit")?,
            entered_quantity_micro: ji64(l, "entered_quantity_micro")?,
            quantity_dimension: jstr(l, "quantity_dimension")?,
            purchase_price_paise: {
                // The JSON's `line_total_paise` / `entered_quantity_micro` at
                // full precision recovers the per-purchase-unit price this
                // struct wants -- `entered_quantity_micro` is always a whole
                // number of purchase units (x 1_000_000) in this fixture, so
                // the division is exact.
                let entered = ji64(l, "entered_quantity_micro")?;
                let total = ji64(l, "line_total_paise")?;
                if entered == 0 {
                    0
                } else {
                    i64::try_from(i128::from(total) * 1_000_000 / i128::from(entered)).unwrap_or(0)
                }
            },
            batch_code: jstr_opt(l, "batch_code"),
            expiry_date: jstr_opt(l, "expiry_date"),
            purchase_order_line_id: jstr_opt(l, "purchase_order_line_id"),
        });
    }

    let req = NewGoodsReceiptNote {
        id: jstr(receipt, "id")?,
        outlet_id: jstr(receipt, "outlet_id")?,
        purchase_order_id: jstr_opt(receipt, "purchase_order_id"),
        supplier_id: jstr_opt(receipt, "supplier_id"),
        delivery_note_ref: jstr_opt(receipt, "delivery_note_ref"),
        received_at: jstr(receipt, "received_at")?,
        received_by_user_id: jstr(receipt, "received_by_user_id")?,
        notes: jstr_opt(receipt, "notes"),
        lines: new_lines,
    };
    let stored = db
        .record_goods_receipt(req)
        .map_err(|e| DbError::InvalidInput(format!("devseed: recording seeded GRN: {e}")))?;
    println!(
        "devseed: seed goods receipt — {} ({} lines, {} gaps)",
        stored.grn_number,
        stored.lines.len(),
        stored.gaps.len()
    );
    Ok(())
}

/// Opening stock: one `COUNT_ADJUSTMENT` `stock_ledger_entry` per
/// inventory item, through a real stock count (`Db::open_stock_count` /
/// `add_or_update_stock_count_line` / `Db::complete_stock_count`) -- the
/// same sanctioned public entry point CLAUDE.md's "enumerate the sinks"
/// rule requires, never a raw ledger insert.
fn write_opening_stock(db: &mut Db, catalogue: &Value) -> Result<(), DbError> {
    let rows = jarr(catalogue, "opening_stock")?;
    if rows.is_empty() {
        return Ok(());
    }

    // Same shape as `write_goods_receipt`: `open_stock_count` /
    // `complete_stock_count` append `stock_ledger_entry` rows (insert-only,
    // trigger-guarded) and are not upserts. A re-run must not call this a
    // second time for the same fixed `OPENING_STOCK_ID`.
    let already_seeded: bool = db
        .connection()
        .query_row(
            "SELECT 1 FROM stock_count WHERE id = ?1",
            [OPENING_STOCK_ID],
            |_| Ok(()),
        )
        .optional()
        .map_err(|e| DbError::InvalidInput(format!("devseed: checking seeded opening stock: {e}")))?
        .is_some();
    if already_seeded {
        println!("devseed: seed opening stock — already present, skipping ({OPENING_STOCK_ID})");
        return Ok(());
    }
    let outlet_id = jstr(&rows[0], "outlet_id")?;
    db.open_stock_count(NewStockCount {
        id: OPENING_STOCK_ID.to_string(),
        outlet_id: outlet_id.clone(),
        started_at: OPENING_STOCK_STARTED_AT.to_string(),
        counted_by_user_id: Some(CASHIER_ID.to_string()),
        note: Some("Opening stock for the demo build".to_string()),
    })
    .map_err(|e| DbError::InvalidInput(format!("devseed: opening stock count: {e}")))?;

    for row in rows {
        db.add_or_update_stock_count_line(
            OPENING_STOCK_ID,
            &outlet_id,
            NewStockCountLine {
                inventory_item_id: jstr(row, "inventory_item_id")?,
                counted_quantity_micro: ji64(row, "quantity_micro")?,
                note: None,
            },
        )
        .map_err(|e| DbError::InvalidInput(format!("devseed: opening stock count line: {e}")))?;
    }

    db.complete_stock_count(OPENING_STOCK_ID, &outlet_id, OPENING_STOCK_COMPLETED_AT)
        .map_err(|e| {
            DbError::InvalidInput(format!("devseed: completing opening stock count: {e}"))
        })?;
    println!(
        "devseed: seed opening stock — {} inventory items",
        rows.len()
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
        (TAX_PROFILE_ALCOHOL_VAT_ID, 0i64, 0i64),
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
            legal_name: RESTAURANT_LEGAL_NAME.to_string(),
            trade_name: OUTLET_NAME.to_string(),
            address_line1: OUTLET_ADDRESS_LINE1.to_string(),
            address_line2: OUTLET_ADDRESS_LINE2.map(str::to_string),
            city: OUTLET_CITY.to_string(),
            state_code: OUTLET_STATE_CODE.to_string(),
            state_name: OUTLET_STATE_NAME.to_string(),
            pincode: OUTLET_PINCODE.to_string(),
            gstin: OUTLET_GSTIN.to_string(),
            fssai_number: OUTLET_FSSAI.map(str::to_string),
            invoice_footer_text: INVOICE_FOOTER_TEXT.map(str::to_string),
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
    println!(
        "devseed:   tax GST 5% (CGST 2.5 + SGST 2.5), series DEV/, GSTIN {}",
        OUTLET_GSTIN
    );
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
        let mut db = Db::open_in_memory_for_tests().expect("open in-memory db");
        let catalogue = build_shared_catalogue().expect("build shared catalogue");
        seed(&mut db, "unused-in-tests-hash", &catalogue).expect("seed");
        db
    }

    /// A Stone Bowl through the sauce sub-recipe at a genuinely FRACTIONAL
    /// share of its batch -- 60 ml of 480 ml, so the multiplier is 1/8 and
    /// never 1x. This is the case contracts 0.5.1 exists for: a sub-recipe
    /// reference carried as an exact rational to the leaf, so rescaling the
    /// batch cannot silently multiply every parent's deductions.
    ///
    /// It is also demo step 3's dish, which is why this one is pinned.
    #[test]
    fn kimchi_stone_bowl_resolves_through_the_sauce_sub_recipe() {
        let db = seeded_db();
        let variant_id = &db
            .connection()
            .query_row(
                "SELECT v.id FROM menu_item_variant v JOIN menu_item m ON m.id = v.menu_item_id \
                 WHERE m.name = 'Kimchi' AND v.name = 'Chicken'",
                [],
                |r| r.get::<_, String>(0),
            )
            .expect("Kimchi / Chicken variant exists");

        let outcome =
            resolve_recipe_for_variant(db.connection(), Some(variant_id), 1).expect("no DbError");
        let ResolveOutcome::Resolved(resolution) = outcome else {
            panic!("expected the Kimchi stone bowl to resolve, got {outcome:?}");
        };
        assert_eq!(resolution.recipe_name, "Kimchi");

        // Chicken: a plain ITEM row, 150 g direct.
        let chicken = resolution
            .leaves
            .iter()
            .find(|l| l.inventory_item_name == "Chicken (Boneless, Diced)")
            .expect("chicken leaf present");
        assert_eq!(chicken.applied_micro, grams(150));

        // Soy sauce: ONLY reachable through the sauce sub-recipe. 60/480 of
        // the batch's 180 ml is 22.5 ml exactly -- a fractional millilitre
        // figure that survives because the multiplier is carried as a
        // rational and rounded once, at the leaf.
        let soy = resolution
            .leaves
            .iter()
            .find(|l| l.inventory_item_name == "Light Soy Sauce")
            .expect("soy sauce leaf present (via the sub-recipe)");
        assert_eq!(
            soy.applied_micro,
            millilitres(180) / 8,
            "60/480 of the sauce batch's 180 ml must be exactly 22.5 ml, not a rounded approximation"
        );

        // Spring onion: BOTH a direct ingredient (15 g) AND inside the sauce
        // (40 g * 60/480 = 5 g) -- the two must SUM, not overwrite.
        let spring_onion = resolution
            .leaves
            .iter()
            .find(|l| l.inventory_item_name == "Spring Onions")
            .expect("spring onion leaf present");
        assert_eq!(spring_onion.applied_micro, grams(15) + grams(5));
    }

    /// A 2x order quantity scales every leaf by 2, including through the
    /// sub-recipe.
    #[test]
    fn stone_bowl_scales_by_order_quantity_through_the_sub_recipe() {
        let db = seeded_db();
        let variant_id: String = db
            .connection()
            .query_row(
                "SELECT v.id FROM menu_item_variant v JOIN menu_item m ON m.id = v.menu_item_id \
                 WHERE m.name = 'Kimchi' AND v.name = 'Chicken'",
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
            .find(|l| l.inventory_item_name == "Chicken (Boneless, Diced)")
            .unwrap();
        assert_eq!(chicken.applied_micro, grams(300));
        let soy = resolution
            .leaves
            .iter()
            .find(|l| l.inventory_item_name == "Light Soy Sauce")
            .unwrap();
        assert_eq!(soy.applied_micro, millilitres(180) / 4);
    }

    /// The simplest recipe in the seed: a straight COUNT -> COUNT
    /// passthrough with no sub-recipe and no other ingredient. One can sold
    /// is one can gone.
    #[test]
    fn a_canned_drink_resolves_as_a_single_count_passthrough() {
        let db = seeded_db();
        let variant_id: String = db
            .connection()
            .query_row(
                "SELECT v.id FROM menu_item_variant v JOIN menu_item m ON m.id = v.menu_item_id \
                 WHERE m.name = 'Aerated Water' AND v.name = 'Coke'",
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
        assert_eq!(resolution.leaves[0].inventory_item_name, "Coca-Cola Can");
        assert_eq!(resolution.leaves[0].applied_micro, pieces(3));
    }

    /// A dish with a variant but deliberately no recipe row -- `NoRecipe`,
    /// structurally different from the `NoVariant` gap below. This is the
    /// realistic gap: 277 items are on the card and 16 are costed, because a
    /// real kitchen costs its signature dishes first and gets to the rest
    /// later. "Items sold with no recipe" is a visible report, not an error
    /// (ADR-018 Rule 2: a missing recipe NEVER fails a confirm).
    #[test]
    fn an_uncosted_dish_is_deliberately_a_no_recipe_gap() {
        let db = seeded_db();
        let variant_id: String = db
            .connection()
            .query_row(
                "SELECT v.id FROM menu_item_variant v JOIN menu_item m ON m.id = v.menu_item_id \
                 WHERE m.name = 'Aona Gomae' AND v.name = 'Regular'",
                [],
                |r| r.get(0),
            )
            .expect("Aona Gomae DOES have a variant -- the seed must not remove it");
        let outcome =
            resolve_recipe_for_variant(db.connection(), Some(&variant_id), 1).expect("no DbError");
        assert_eq!(outcome, ResolveOutcome::Gap(GapReason::NoRecipe));
    }

    /// A line carrying no `menu_item_variant_id` is a `NoVariant` gap,
    /// structurally different from `Chana Masala`'s `NoRecipe` gap above.
    /// That path stays reachable in production -- any caller that sends a
    /// null variant lands here -- so it is asserted directly against the
    /// resolver rather than through a seeded item that happens to have none.
    ///
    /// The second half is the guard the demo build added: NO seeded item may
    /// be variant-less. Samosa used to be this test's fixture, and a
    /// variant-less item is two silent failures, not one -- it can carry no
    /// recipe (ADR-018 2.1 binds a recipe to a variant, so selling it deducts
    /// nothing at all) and `apps/captain` refuses to order it, treating it as
    /// a seed defect rather than sending a null (`MenuCartScreen.tsx`).
    #[test]
    fn a_null_variant_is_a_novariant_gap_and_no_seeded_item_is_variant_less() {
        let db = seeded_db();
        let outcome = resolve_recipe_for_variant(db.connection(), None, 1).expect("no DbError");
        assert_eq!(outcome, ResolveOutcome::Gap(GapReason::NoVariant));

        let variant_less: Vec<String> = db
            .connection()
            .prepare(
                "SELECT m.name FROM menu_item m                  WHERE NOT EXISTS                  (SELECT 1 FROM menu_item_variant v WHERE v.menu_item_id = m.id)                  ORDER BY m.name",
            )
            .expect("prepare succeeds")
            .query_map([], |r| r.get::<_, String>(0))
            .expect("query succeeds")
            .collect::<Result<Vec<String>, _>>()
            .expect("rows read");
        assert!(
            variant_less.is_empty(),
            "every seeded menu_item needs at least one variant; these have none: {variant_less:?}"
        );
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

    /// SEED PARITY, the edge half, ROW FOR ROW rather than by count.
    ///
    /// `backend/cmd/devseed` and this binary are two independent readers of
    /// one committed file, and a matching COUNT with mismatched CONTENTS is
    /// exactly what the old hand-mirrored constants achieved -- so this
    /// compares every field of every row it can reach, by id. The cloud side
    /// of the same comparison is run against a clean Postgres during a demo
    /// reset (docs/demo-status.md); this half runs in CI on every commit.
    #[test]
    fn edge_rows_match_the_shared_catalogue_row_for_row() {
        let catalogue = build_shared_catalogue().expect("build shared catalogue");
        let mut db = Db::open_in_memory_for_tests().expect("open in-memory db");
        seed(&mut db, "unused-in-tests-hash", &catalogue).expect("seed");
        let conn = db.connection();

        // (catalogue key, table, columns as they appear in BOTH sides)
        let specs: &[(&str, &str, &[&str])] = &[
            (
                "menu_categories",
                "menu_category",
                &["id", "name", "sort_order"],
            ),
            (
                "menu_items",
                "menu_item",
                &["id", "category_id", "name", "base_price_paise", "hsn_sac"],
            ),
            (
                "menu_item_variants",
                "menu_item_variant",
                &["id", "menu_item_id", "name", "price_delta_paise"],
            ),
            (
                "menu_item_modifiers",
                "menu_item_modifier",
                &[
                    "id",
                    "menu_item_id",
                    "group_name",
                    "option_name",
                    "price_delta_paise",
                ],
            ),
            (
                "inventory_items",
                "inventory_item",
                &["id", "sku", "name", "dimension"],
            ),
            (
                "recipes",
                "recipe",
                &[
                    "id",
                    "menu_item_variant_id",
                    "name",
                    "output_dimension",
                    "output_quantity_micro",
                ],
            ),
            (
                "recipe_ingredients",
                "recipe_ingredient",
                &["id", "recipe_id", "quantity_micro", "quantity_dimension"],
            ),
            (
                "supplier_items",
                "supplier_item",
                &[
                    "id",
                    "supplier_id",
                    "inventory_item_id",
                    "purchase_unit",
                    "pack_size_micro",
                ],
            ),
            (
                "tax_profiles",
                "tax_profile",
                &["id", "code", "name", "pricing_mode"],
            ),
            (
                "tax_rules",
                "tax_rule",
                &["id", "tax_profile_id", "component", "rate_bps"],
            ),
        ];

        for (key, table, columns) in specs {
            let rows = catalogue[*key].as_array().expect("catalogue array");
            assert!(!rows.is_empty(), "{key}: catalogue side is empty");
            for row in rows {
                let id = row["id"].as_str().expect("row id");
                for column in *columns {
                    // Read every column as TEXT so one comparison covers
                    // integers and strings alike; SQLite coerces on read and
                    // both sides are rendered the same way.
                    let stored: Option<String> = conn
                        .query_row(
                            &format!("SELECT CAST({column} AS TEXT) FROM {table} WHERE id = ?1"),
                            [id],
                            |r| r.get(0),
                        )
                        .unwrap_or_else(|e| panic!("{table} row {id} missing from SQLite: {e}"));
                    let expected = match &row[*column] {
                        Value::Null => None,
                        Value::String(text) => Some(text.clone()),
                        other => Some(other.to_string()),
                    };
                    assert_eq!(
                        stored, expected,
                        "{table}.{column} differs from the shared catalogue on row {id}"
                    );
                }
            }
        }
    }

    /// FNV-1a, 64-bit. Reproduces `scripts/menu-to-seed.py`'s own
    /// `fnv1a64` exactly, byte for byte, over the same canonical projection.
    /// Not a cryptographic digest and not trying to be: what it guards
    /// against is an accidental hand-edit of a generated file, and this
    /// crate carries no hashing dependency for a dev-only binary to borrow.
    fn fnv1a64(text: &str) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in text.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
        hash
    }

    /// The client menu catalogue is GENERATED from the client's workbook
    /// (`scripts/menu-to-seed.py`), so the drift this guards against is
    /// not the old two-hand-maintained-copies kind that
    /// `HOLLER_DEV_MENU_SPEC.md` needed -- it is a hand-edit of the generated
    /// file, or a regenerated file committed without its manifest.
    ///
    /// Both sides are rebuilt here: the counts, and a canonical projection of
    /// every item, price, station, tax class, variant delta and modifier
    /// delta. Editing `devseed/client_menu.rs` by hand fails this; editing the
    /// workbook without regenerating fails it too, because the manifest
    /// carries the workbook's own digest and the generator is the only thing
    /// that writes either.
    #[test]
    fn client_menu_matches_the_generated_manifest() {
        let manifest_path = format!(
            "{}/../../seed/menu-manifest.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&manifest_path)
            .unwrap_or_else(|e| panic!("could not read {manifest_path}: {e}"));
        let manifest: Value = serde_json::from_str(&text).expect("manifest is valid JSON");

        let mut rows: Vec<String> = Vec::new();
        let mut variants = 0usize;
        let mut modifier_options = 0usize;
        for (category, _sort_order, items) in client_menu::CLIENT_CATEGORIES {
            for item in *items {
                variants += item.variants.len();
                modifier_options += item
                    .modifier_groups
                    .iter()
                    .map(|(_, options)| options.len())
                    .sum::<usize>();
                let tax = if item.tax_profile_id == TAX_PROFILE_ALCOHOL_VAT_ID {
                    "ALCOHOL"
                } else {
                    "FOOD5"
                };
                let variant_field = item
                    .variants
                    .iter()
                    .map(|(name, delta)| format!("{name}={delta}"))
                    .collect::<Vec<_>>()
                    .join(";");
                let modifier_field = item
                    .modifier_groups
                    .iter()
                    .flat_map(|(group, options)| {
                        options
                            .iter()
                            .map(move |(option, delta)| format!("{group}/{option}={delta}"))
                    })
                    .collect::<Vec<_>>()
                    .join(";");
                rows.push(format!(
                    "{category}|{}|{}|{}|{tax}|{variant_field}|{modifier_field}",
                    item.name, item.price_paise, item.station_code
                ));
            }
        }

        let items = rows.len();
        assert_eq!(
            manifest["categories"].as_u64().expect("categories"),
            client_menu::CLIENT_CATEGORIES.len() as u64,
            "category count differs from the manifest -- re-run scripts/menu-to-seed.py"
        );
        assert_eq!(
            manifest["items"].as_u64().expect("items"),
            items as u64,
            "item count differs from the manifest -- re-run scripts/menu-to-seed.py"
        );
        assert_eq!(
            manifest["variants"].as_u64().expect("variants"),
            variants as u64,
            "variant count differs from the manifest -- re-run scripts/menu-to-seed.py"
        );
        assert_eq!(
            manifest["modifier_options"]
                .as_u64()
                .expect("modifier_options"),
            modifier_options as u64,
            "modifier count differs from the manifest -- re-run scripts/menu-to-seed.py"
        );
        assert_eq!(
            manifest["projection_fnv1a64"].as_str().expect("projection"),
            format!("{:016x}", fnv1a64(&rows.join("\n"))),
            "the generated catalogue no longer matches the manifest. Either \
             edge/database/src/bin/devseed/client_menu.rs was hand-edited (it is \
             GENERATED -- edit menu_imgs_gong/gong_menu.xlsx instead), or the \
             workbook changed and scripts/menu-to-seed.py was not re-run."
        );
    }
}
