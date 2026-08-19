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

    if env::var("HOLLER_SEED_BILLING").is_ok_and(|v| v == "1") {
        seed_billing(conn)?;
    }

    // Without a sync_state row the outbox has no cursor to advance against
    // once the sync worker is eventually wired up.
    repo::init_sync_state(conn, OUTLET_ID)?;

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
