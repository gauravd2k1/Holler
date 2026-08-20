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
use holler_edge_database::model::{
    AppUser, ComplianceVersion, Device, DiscountDefinition, InvoiceSeries, MenuCategory, MenuItem,
    MenuItemModifier, MenuItemVariant, Outlet, OutletFiscalProfile, Printer, RestaurantTable,
    Station, TaxProfile, TaxRule,
};
use holler_edge_database::{repo, Db};

// Fixed development ids. MUST match the constants in
// backend/cmd/devseed/main.go — the two seeders describe the same outlet.
const TENANT_ID: &str = "0191a000-0000-7000-8000-000000000001";
const BRAND_ID: &str = "0191a000-0000-7000-8000-000000000002";
const OUTLET_ID: &str = "0191a000-0000-7000-8000-00000000000a";
const DEVICE_ID: &str = "0191a000-0000-7000-8000-00000000000b";
const CASHIER_ID: &str = "0191a000-0000-7000-8000-00000000000c";
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

const SPICE_GROUP: (&str, &[(&str, i64)]) =
    ("Spice", &[("Mild", 0), ("Med", 0), ("Hot", 0)]);

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
                variants: &[],
                modifier_groups: &[("Extras", &[("Extra butter", 1500)])],
            },
            SeedItem {
                name: "Garlic Naan",
                price_paise: 7000,
                tax_profile_id: TAX_PROFILE_FOOD5_ID,
                hsn_sac: "9963",
                station_code: STATION_TANDOOR_CODE,
                variants: &[],
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
                variants: &[],
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
                variants: &[],
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
                variants: &[],
                modifier_groups: &[],
            },
            SeedItem {
                name: "Thums Up (can)",
                price_paise: 5000,
                tax_profile_id: TAX_PROFILE_AERATED40_ID,
                hsn_sac: "2202",
                station_code: STATION_BAR_CODE,
                variants: &[],
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
const CASHIER_PERMISSIONS: &str = r#"["order.create","order.modify","table.manage"]"#;

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

    seed_menu(conn)?;

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
fn seed_menu(conn: &rusqlite::Connection) -> Result<(), holler_edge_database::DbError> {
    for (id, code, name, sort_order) in [
        (STATION_TANDOOR_ID, STATION_TANDOOR_CODE, "Tandoor", 2),
        (STATION_MAIN_ID, STATION_MAIN_CODE, "Main Kitchen (Curries)", 3),
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

            for variant_name in item.variants {
                variant_seq += 1;
                repo::upsert_menu_item_variant(
                    conn,
                    &MenuItemVariant {
                        id: menu_variant_id(variant_seq),
                        menu_item_id: item_id.clone(),
                        name: variant_name.to_string(),
                        // The spec names variant options but gives no price
                        // deltas for any of them (unlike modifiers, which
                        // always carry an explicit figure) — 0 is the
                        // representative dev value, not a claim that e.g.
                        // Half and Full cost the same at a real outlet.
                        price_delta_paise: 0,
                        config_version: CONFIG_VERSION,
                    },
                )?;
            }

            for (group_name, options) in item.modifier_groups {
                for (option_name, delta) in *options {
                    modifier_seq += 1;
                    repo::upsert_menu_item_modifier(
                        conn,
                        &MenuItemModifier {
                            id: menu_modifier_id(modifier_seq),
                            menu_item_id: item_id.clone(),
                            group_name: group_name.to_string(),
                            option_name: option_name.to_string(),
                            price_delta_paise: *delta,
                            min_selection: 0,
                            max_selection: 1,
                            config_version: CONFIG_VERSION,
                        },
                    )?;
                }
            }
        }
    }

    println!("devseed: seed menu — {item_seq} items across {category_seq} categories, 5 stations, 3 tax profiles (GST_FOOD_5/GST_PACKAGED_18/GST_AERATED_40)");
    Ok(())
}

/// The config a bill needs before one can be issued: a compliance version, a
/// GST 5% default tax profile (CGST 2.5% + SGST 2.5%), the outlet's fiscal
/// identity as printed on the invoice, an active SALES numbering series,
/// three discount definitions, and two printers with roles.
///
/// Opt-in — see the const block above for why the e2e harness must not get
/// these rows.
fn seed_billing(
    conn: &rusqlite::Connection,
) -> Result<(), holler_edge_database::DbError> {
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
