//! e2e-scenario-harness bridge (see Cargo.toml). Owns the real, shipped
//! layers a scenario needs on the Rust side of the process boundary:
//!
//!  - seeds a fresh scratch edge database by invoking the REAL `devseed`
//!    binary (never `%APPDATA%`, never a real `edge.db.enc`), then augments
//!    it with `holler_edge_database::repo::*` calls for the extra fixtures
//!    the spec requires that `devseed` does not provide (a second station,
//!    a multi-station item, a deliberately unrouted item);
//!  - executes POS actions through the exact `holler_pos_lib::commands::*::*_impl`
//!    functions the Tauri IPC layer calls — no reimplementation;
//!  - starts the real `holler_edge_device::server` LAN listener, sharing one
//!    `Arc<Mutex<Db>>`/`Hub` with the command layer exactly as
//!    `apps/pos/src-tauri/src/state.rs` wires it in production;
//!  - answers DB-introspection and crash/recovery requests so the
//!    TypeScript orchestrator (tests/e2e-scenario/orchestrator) can assert
//!    the durability/outbox/money invariants without touching SQLite
//!    itself.
//!
//! Protocol: line-delimited JSON on stdin/stdout, one request per line, one
//! response per line, in order (fully synchronous — the orchestrator never
//! pipelines). See `Request`/`respond` below for the message shapes. Any
//! line on stdin that is not valid JSON, or EOF, ends the process after
//! flushing stdout.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;

use holler_edge_database::crypto::EncryptionKey;
use holler_edge_database::{model, repo, Db};
use holler_edge_device::{server, CachedCredentialVerifier, DeviceTokenVerifier};
use holler_pos_lib::commands::billing::{
    issue_invoice_impl, list_invoices_for_order_impl, list_payments_for_order_impl,
    record_payment_impl,
};
use holler_pos_lib::commands::kitchen::{
    list_kots_for_order_impl, send_order_to_kitchen_impl, transition_kot_status_impl,
};
use holler_pos_lib::commands::orders::{
    add_order_item_impl, confirm_order_impl, create_order_impl, get_order_impl,
    remove_order_item_impl, update_order_shape_impl, NewOrderItemModifierRequest,
    NewOrderItemRequest,
};
use holler_pos_lib::error::AppError;
use holler_pos_lib::state::AppState;

// ---- Fixed devseed ids (edge/database/src/bin/devseed.rs). Read-only
// reference to that binary's own documented, fixed development ids — this
// file does not edit devseed, it reuses the ids the real seeded rows carry
// so the harness can address them. Kept in sync by devseed's own comment
// promising these never change without updating its callers. ----
mod devseed_ids {
    pub const TENANT_ID: &str = "0191a000-0000-7000-8000-000000000001";
    pub const OUTLET_ID: &str = "0191a000-0000-7000-8000-00000000000a";
    pub const POS_DEVICE_ID: &str = "0191a000-0000-7000-8000-00000000000b";
    pub const CASHIER_ID: &str = "0191a000-0000-7000-8000-00000000000c";
    pub const KDS_DEVICE_ID: &str = "0191a000-0000-7000-8000-00000000000d";
    pub const CATEGORY_ID: &str = "0191a000-0000-7000-8000-000000000010";
    pub const ITEM_CHAI_ID: &str = "0191a000-0000-7000-8000-000000000011"; // single-station, has variant+modifiers
    pub const ITEM_THALI_ID: &str = "0191a000-0000-7000-8000-000000000012"; // single-station
    pub const VARIANT_ID: &str = "0191a000-0000-7000-8000-000000000013";
    pub const MOD_LESS_SUGAR_ID: &str = "0191a000-0000-7000-8000-000000000014";
    pub const MOD_EXTRA_SUGAR_ID: &str = "0191a000-0000-7000-8000-000000000015";
    pub const TABLE_1_ID: &str = "0191a000-0000-7000-8000-000000000020";
    pub const TABLE_2_ID: &str = "0191a000-0000-7000-8000-000000000021";
    pub const STATION_1_ID: &str = "0191a000-0000-7000-8000-000000000030";
    pub const STATION_1_CODE: &str = "MAIN_KITCHEN";
}

// ---- Harness-minted billing config fixtures (contracts 0.4.2/ADR-016):
// devseed itself seeds no tax_profile/outlet_fiscal_profile/invoice_series
// row, so `issue_invoice_impl` cannot resolve a GSTIN or an active series
// against a bare devseed template — mirrors apps/pos/src-tauri/tests/
// billing_flow.rs's own fixture set, the only place this shape was already
// proven correct. ----
const COMPLIANCE_VERSION_ID: &str = "0191c000-0000-7000-8000-000000000001";
const TAX_PROFILE_ID: &str = "0191c000-0000-7000-8000-000000000002";
const FISCAL_PROFILE_ID: &str = "0191c000-0000-7000-8000-000000000003";
const INVOICE_SERIES_ID: &str = "0191c000-0000-7000-8000-000000000004";

// ---- Harness-minted KDS device credential (ADR-017 amendment): edge/device
// now requires a verifiable device_token as the connection's first frame on
// every LAN handshake (apps/kds/src/lib/lanConfig.ts), so the KDS driver
// needs one enrolled credential to present. Plaintext secret is fixed and
// scoped to this throwaway scratch database only. ----
const KDS_CREDENTIAL_ID: &str = "0191c000-0000-7000-8000-000000000005";
const KDS_CREDENTIAL_SECRET: &str = "e2e-harness-kds-secret";

// ---- Harness-minted extra fixture ids (augmented onto the template after
// devseed runs) — a second station, a multi-station item, and a
// deliberately no-station item, per the spec's seed requirements. ----
const STATION_2_ID: &str = "0191b000-0000-7000-8000-000000000031";
const STATION_2_CODE: &str = "BAR";
const ITEM_MULTI_ID: &str = "0191b000-0000-7000-8000-000000000040"; // routes to BOTH stations
const ITEM_NO_STATION_ID: &str = "0191b000-0000-7000-8000-000000000041"; // routes nowhere, deliberately

/// Fixed 32-byte key (as hex) for every scratch database this harness ever
/// opens. Never used for anything but throwaway scratch dirs created below.
const FIXED_KEY_HEX: &str = "e2e0e2e0e2e0e2e0e2e0e2e0e2e0e2e0e2e0e2e0e2e0e2e0e2e0e2e0e2e0e2e0";
/// devseed requires a password hash env var but this harness never logs in
/// (every `*_impl` function it calls takes `&AppState` directly, with no
/// authentication check — the same surface the Tauri IPC layer exposes).
/// This is a syntactically well-formed but unverifiable placeholder.
const DUMMY_PASSWORD_HASH: &str = "$argon2id$v=19$m=65536,t=2,p=4$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

fn parse_key_hex(hex: &str) -> EncryptionKey {
    let mut bytes = [0u8; 32];
    for i in 0..32 {
        bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("fixed hex key is valid");
    }
    EncryptionKey::new(bytes)
}

/// Hashes `plaintext` into the same `$argon2id$v=..$m=..,t=..,p=..$salt$hash`
/// PHC encoding `holler_edge_database::auth::verify_password` parses — so
/// the one `device_credential_cache` row this harness seeds (below) verifies
/// against a real Argon2id check, not a stub. Params/salt are fixed
/// (throwaway scratch database only, never a real credential).
fn hash_device_secret(plaintext: &str) -> String {
    const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD_NO_PAD;
    let salt: [u8; 16] = *b"e2e-harness-salt";
    let params = Params::new(19_456, 2, 1, Some(32)).expect("valid argon2 params");
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut hash = vec![0u8; 32];
    argon2
        .hash_password_into(plaintext.as_bytes(), &salt, &mut hash)
        .expect("argon2 hash");
    format!(
        "$argon2id$v=19$m=19456,t=2,p=1${}${}",
        B64.encode(salt),
        B64.encode(hash)
    )
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Ping,
    NewScenario,
    CloseScenario,
    CreateDraft {
        order_type: String,
        table_id: Option<String>,
        menu_item_id: String,
        variant_id: Option<String>,
        unit_price_paise: i64,
        quantity: i64,
        notes: Option<String>,
        #[serde(default)]
        modifiers: Vec<ModifierSelection>,
    },
    AddItem {
        order_id: String,
        menu_item_id: String,
        variant_id: Option<String>,
        unit_price_paise: i64,
        quantity: i64,
        notes: Option<String>,
        #[serde(default)]
        modifiers: Vec<ModifierSelection>,
    },
    RemoveItem {
        order_id: String,
        order_item_id: String,
    },
    UpdateShape {
        order_id: String,
        order_type: String,
        table_id: Option<String>,
    },
    Confirm {
        order_id: String,
    },
    SendToKitchen {
        order_id: String,
    },
    TransitionKot {
        order_id: String,
        kot_id: String,
        status: String,
    },
    GetOrder {
        order_id: String,
    },
    ListKots {
        order_id: String,
    },
    Introspect,
    ResumeScenario { dir: String },
    // ---------------------------------------------------- billing (T11b) --
    IssueInvoice {
        order_id: String,
        created_by_user_id: String,
    },
    ListInvoicesForOrder {
        order_id: String,
    },
    RecordPayment {
        order_id: String,
        method: String,
        amount_paise: i64,
        tendered_paise: Option<i64>,
        change_paise: Option<i64>,
        reference: Option<String>,
        cash_shift_id: Option<String>,
        reverses_payment_id: Option<String>,
        // Which invoice a FORWARD tender settles (billing.rs's own doc
        // comment on `record_payment_impl`) — required for the edge-side
        // over-settlement guard (`FORWARD_PAYMENT_EXCEEDS_REMAINING_DUE`,
        // T9 retry) to have anything to check against. Ignored by
        // `record_payment_impl` for a reversal.
        #[serde(default)]
        invoice_id: Option<String>,
        created_by_user_id: String,
    },
    ListPaymentsForOrder {
        order_id: String,
    },
}

/// One modifier selection submitted alongside a cart line over the bridge —
/// mirrors `holler_pos_lib::commands::orders::NewOrderItemModifierRequest`'s
/// wire shape field-for-field.
#[derive(Debug, Clone, Deserialize)]
struct ModifierSelection {
    modifier_id: String,
    group_name: String,
    option_name: String,
    price_delta_paise: i64,
}

impl From<ModifierSelection> for NewOrderItemModifierRequest {
    fn from(m: ModifierSelection) -> Self {
        NewOrderItemModifierRequest {
            modifier_id: m.modifier_id,
            group_name: m.group_name,
            option_name: m.option_name,
            price_delta_paise: m.price_delta_paise,
        }
    }
}

struct Scenario {
    state: AppState,
    lan_handle: Option<server::LanServerHandle>,
    scratch_dir: PathBuf,
}

struct Harness {
    template_sealed: PathBuf,
    root: PathBuf,
    scenario: Option<Scenario>,
    scenario_seq: u64,
}

fn order_to_json(o: &model::Order) -> Value {
    json!({
        "id": o.id, "outlet_id": o.outlet_id, "device_id": o.device_id,
        "order_type": o.order_type, "status": o.status, "table_id": o.table_id,
        "subtotal_paise": o.subtotal_paise, "discount_paise": o.discount_paise,
        "taxes_paise": o.taxes_paise, "total_paise": o.total_paise,
        "version": o.version, "sync_status": o.sync_status,
        "created_at": o.created_at, "updated_at": o.updated_at,
    })
}

fn order_item_to_json(i: &model::OrderItem) -> Value {
    json!({
        "id": i.id, "order_id": i.order_id, "menu_item_id": i.menu_item_id,
        "variant_id": i.variant_id, "quantity": i.quantity,
        "unit_price_paise": i.unit_price_paise, "line_total_paise": i.line_total_paise,
        "notes": i.notes,
    })
}

fn kot_to_json(k: &model::Kot) -> Value {
    json!({
        "id": k.id, "order_id": k.order_id, "station": k.station,
        "sequence": k.sequence, "status": k.status,
        "items": serde_json::from_str::<Value>(&k.items_json).unwrap_or(Value::Null),
        "created_by_device_id": k.created_by_device_id,
        "created_at": k.created_at, "updated_at": k.updated_at,
    })
}

fn outbox_to_json(e: &model::OutboxEntry) -> Value {
    json!({
        "id": e.id, "aggregate_type": e.aggregate_type, "aggregate_id": e.aggregate_id,
        "event_type": e.event_type, "created_at": e.created_at,
        "published_at": e.published_at, "attempt_count": e.attempt_count,
    })
}

fn app_error_to_json(e: &AppError) -> Value {
    json!({ "code": e.code, "message": e.message })
}

/// Builds (once) the template sealed database this run's scenarios are all
/// cheap copies of: invokes the real `devseed` binary against a scratch
/// dir, then augments it with a second station, a multi-station item and a
/// deliberately unrouted item via `holler_edge_database::repo::*` — the
/// fixtures the spec requires that devseed does not seed.
fn build_template(root: &Path) -> PathBuf {
    let template_dir = root.join("template");
    std::fs::create_dir_all(&template_dir).expect("create template dir");
    let sealed_existing = template_dir.join("edge.db.enc");
    if sealed_existing.exists() {
        // A crash-simulation respawn (see `ResumeScenario`) restarts this
        // binary against the SAME `HOLLER_E2E_DATA_DIR`. The template is
        // immutable for the run's lifetime, so re-running devseed here would
        // only cost real seconds for a no-op — skip straight to reuse.
        return sealed_existing;
    }

    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../edge/database/Cargo.toml");
    let status = std::process::Command::new("cargo")
        .args(["run", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .arg("--bin")
        .arg("devseed")
        .env("HOLLER_EDGE_DATA_DIR", &template_dir)
        .env("HOLLER_DB_KEY_HEX", FIXED_KEY_HEX)
        .env("HOLLER_SEED_PASSWORD_HASH", DUMMY_PASSWORD_HASH)
        .status()
        .expect("spawn devseed");
    assert!(status.success(), "devseed failed with status {status:?}");

    let sealed = template_dir.join("edge.db.enc");
    let plaintext = template_dir.join("edge.db");
    let key = parse_key_hex(FIXED_KEY_HEX);

    // Augment: open the real devseed output and add the fixtures that
    // devseed itself does not provide (ADR-003: through this crate's public
    // repo functions only, never raw SQL, never editing devseed).
    let db = Db::open(&sealed, &plaintext, key).expect("open template for augmentation");
    let conn = db.connection();

    repo::upsert_station(
        conn,
        &model::Station {
            id: STATION_2_ID.to_string(),
            outlet_id: devseed_ids::OUTLET_ID.to_string(),
            code: STATION_2_CODE.to_string(),
            name: "Bar".to_string(),
            sort_order: 2,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed second station");

    repo::upsert_menu_item(
        conn,
        &model::MenuItem {
            id: ITEM_MULTI_ID.to_string(),
            outlet_id: devseed_ids::OUTLET_ID.to_string(),
            category_id: devseed_ids::CATEGORY_ID.to_string(),
            name: "Combo Platter (multi-station)".to_string(),
            base_price_paise: 35000,
            is_available: true,
            config_version: 1,
            tax_profile_id: None,
        },
    )
    .expect("seed multi-station item");
    repo::replace_menu_item_stations(
        conn,
        ITEM_MULTI_ID,
        &[devseed_ids::STATION_1_ID.to_string(), STATION_2_ID.to_string()],
        1,
    )
    .expect("route multi-station item to both stations");

    repo::upsert_menu_item(
        conn,
        &model::MenuItem {
            id: ITEM_NO_STATION_ID.to_string(),
            outlet_id: devseed_ids::OUTLET_ID.to_string(),
            category_id: devseed_ids::CATEGORY_ID.to_string(),
            name: "Packaging Charge (no-station)".to_string(),
            base_price_paise: 1000,
            is_available: true,
            config_version: 1,
            tax_profile_id: None,
        },
    )
    .expect("seed no-station item");
    // Deliberately no replace_menu_item_stations call for this item.

    // ---- billing config (T11b): devseed itself seeds no tax_profile /
    // outlet_fiscal_profile / invoice_series row, so issue_invoice_impl
    // would fail NO_FISCAL_PROFILE_CONFIGURED / NO_ACTIVE_INVOICE_SERIES on
    // every scenario without this. Mirrors apps/pos/src-tauri/tests/
    // billing_flow.rs's fixture set — the one place this exact shape was
    // already proven to compute a correct GST invoice. Every seeded item
    // above and in devseed itself leaves tax_profile_id = None, so all of
    // them resolve to this one default profile (GST 5%, CGST 2.5% + SGST
    // 2.5%) via holler_edge_database::tax::resolve_tax_profile's fallback. ----
    repo::upsert_compliance_version(
        conn,
        &model::ComplianceVersion {
            id: COMPLIANCE_VERSION_ID.to_string(),
            outlet_id: devseed_ids::OUTLET_ID.to_string(),
            label: "GST e2e-harness".to_string(),
            effective_from: "2020-01-01T00:00:00Z".to_string(),
            notes: None,
            config_version: 1,
        },
    )
    .expect("seed compliance version");

    repo::upsert_tax_profile(
        conn,
        &model::TaxProfile {
            id: TAX_PROFILE_ID.to_string(),
            outlet_id: devseed_ids::OUTLET_ID.to_string(),
            code: "GST_5".to_string(),
            name: "GST 5%".to_string(),
            pricing_mode: "EXCLUSIVE".to_string(),
            is_default: true,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed tax profile");

    for (component, rate_bps) in [("CGST", 250i64), ("SGST", 250i64)] {
        repo::upsert_tax_rule(
            conn,
            &model::TaxRule {
                id: format!("{TAX_PROFILE_ID}-{component}"),
                tax_profile_id: TAX_PROFILE_ID.to_string(),
                compliance_version_id: COMPLIANCE_VERSION_ID.to_string(),
                component: component.to_string(),
                rate_bps,
                effective_from: "2020-01-01T00:00:00Z".to_string(),
                effective_to: None,
                config_version: 1,
            },
        )
        .expect("seed tax rule");
    }

    repo::upsert_outlet_fiscal_profile(
        conn,
        &model::OutletFiscalProfile {
            id: FISCAL_PROFILE_ID.to_string(),
            outlet_id: devseed_ids::OUTLET_ID.to_string(),
            legal_name: "e2e Harness Restaurant Pvt Ltd".to_string(),
            trade_name: "e2e Harness Outlet".to_string(),
            address_line1: "123 MG Road".to_string(),
            address_line2: None,
            city: "Pune".to_string(),
            state_code: "27".to_string(),
            state_name: "Maharashtra".to_string(),
            pincode: "411001".to_string(),
            gstin: "27AAAAA0000A1Z5".to_string(),
            fssai_number: None,
            invoice_footer_text: None,
            effective_from: "2020-01-01T00:00:00Z".to_string(),
            config_version: 1,
        },
    )
    .expect("seed fiscal profile");

    repo::upsert_invoice_series(
        conn,
        &model::InvoiceSeries {
            id: INVOICE_SERIES_ID.to_string(),
            outlet_id: devseed_ids::OUTLET_ID.to_string(),
            code: "SALES".to_string(),
            prefix_template: "INV-".to_string(),
            reset_policy: "NEVER".to_string(),
            padding_width: 6,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed invoice series");

    // ---- KDS device credential (ADR-017 amendment): edge/device's server
    // now requires a verifiable device_token as the connection's first frame
    // (apps/kds/src/lib/lanConfig.ts) — without this row every KDS
    // connection in every scenario would be rejected before any frame moved. ----
    repo::replace_device_credential_cache(
        conn,
        &model::DeviceCredentialCache {
            credential_id: KDS_CREDENTIAL_ID.to_string(),
            device_id: devseed_ids::KDS_DEVICE_ID.to_string(),
            tenant_id: devseed_ids::TENANT_ID.to_string(),
            outlet_id: devseed_ids::OUTLET_ID.to_string(),
            credential_hash: hash_device_secret(KDS_CREDENTIAL_SECRET),
            device_kind: "KDS".to_string(),
            revoked_at: None,
            expires_at: None,
            config_version: 1,
        },
    )
    .expect("seed KDS device credential");

    db.close().expect("reseal augmented template");
    sealed
}

fn scenario_response(dir_name: &str, port: u16) -> Value {
    json!({
        "port": port,
        "scenario_dir": dir_name,
        "outlet_id": devseed_ids::OUTLET_ID,
        "pos_device_id": devseed_ids::POS_DEVICE_ID,
        "kds_device_id": devseed_ids::KDS_DEVICE_ID,
        // <credential_id>.<secret> — the connection's first frame
        // (apps/kds/src/lib/lanConfig.ts's AUTHENTICATION note).
        "kds_device_token": format!("{KDS_CREDENTIAL_ID}.{KDS_CREDENTIAL_SECRET}"),
        "cashier_user_id": devseed_ids::CASHIER_ID,
        "stations": { "single": devseed_ids::STATION_1_CODE, "multi_extra": STATION_2_CODE },
        "tables": [devseed_ids::TABLE_1_ID, devseed_ids::TABLE_2_ID],
        "items": {
            "single_station": { "id": devseed_ids::ITEM_CHAI_ID, "unit_price_paise": 4000,
                "variant_id": devseed_ids::VARIANT_ID,
                "modifier_ids": [devseed_ids::MOD_LESS_SUGAR_ID, devseed_ids::MOD_EXTRA_SUGAR_ID],
                // Real seeded (group_name, option_name, price_delta_paise) per
                // modifier, so the orchestrator can attach a genuine modifier
                // price delta to an order line (M3 Track B's own fixture,
                // devseed.rs's "Sugar" group) rather than fabricate one.
                "modifiers": [
                    { "id": devseed_ids::MOD_LESS_SUGAR_ID, "group_name": "Sugar", "option_name": "Less Sugar", "price_delta_paise": 0 },
                    { "id": devseed_ids::MOD_EXTRA_SUGAR_ID, "group_name": "Sugar", "option_name": "Extra Sugar", "price_delta_paise": 500 },
                ] },
            "single_station_2": { "id": devseed_ids::ITEM_THALI_ID, "unit_price_paise": 22000 },
            "multi_station": { "id": ITEM_MULTI_ID, "unit_price_paise": 35000 },
            "no_station": { "id": ITEM_NO_STATION_ID, "unit_price_paise": 1000 },
        },
    })
}

/// Opens (or, when `sealed`/`plaintext` already exist and carry an unclean
/// marker, recovers) a scenario database at the given paths, wires the LAN
/// server exactly as `apps/pos/src-tauri/src/state.rs::AppState::open` does
/// in production (shared `Arc<Mutex<Db>>`, `state.hub` populated from the
/// same handle), and installs it as the harness's current scenario.
fn open_scenario_at(h: &mut Harness, dir: PathBuf, sealed: PathBuf, plaintext: PathBuf) -> Value {
    let key = parse_key_hex(FIXED_KEY_HEX);
    let db = match Db::open(&sealed, &plaintext, key) {
        Ok(db) => db,
        Err(e) => return json!({ "ok": false, "error": e.to_string() }),
    };
    let mut state = AppState::new(
        db,
        devseed_ids::OUTLET_ID.to_string(),
        devseed_ids::POS_DEVICE_ID.to_string(),
    );
    // ADR-017 amendment: the LAN server now requires a device_token verifier
    // — mirrors apps/pos/src-tauri/src/state.rs::start_lan_server exactly
    // (local-cache verifier, no cloud fallback: this harness never syncs).
    let verifier: Arc<dyn DeviceTokenVerifier> =
        Arc::new(CachedCredentialVerifier::new(state.db.clone(), "KDS", None));
    let handle = server::start(
        "127.0.0.1:0".parse().unwrap(),
        state.db.clone(),
        Duration::from_millis(500),
        verifier,
    )
    .expect("lan server binds");
    state.hub = Some(handle.hub.clone());
    let port = handle.local_addr().port();
    let dir_name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();

    h.scenario = Some(Scenario {
        state,
        lan_handle: Some(handle),
        scratch_dir: dir,
    });

    scenario_response(&dir_name, port)
}

/// Copies the sealed template into a fresh scratch dir and opens it — a
/// brand-new scenario.
fn new_scenario(h: &mut Harness) -> Value {
    h.scenario_seq += 1;
    let dir = h.root.join(format!("scenario-{}", h.scenario_seq));
    std::fs::create_dir_all(&dir).expect("create scenario dir");
    let sealed = dir.join("edge.db.enc");
    let plaintext = dir.join("edge.db");
    std::fs::copy(&h.template_sealed, &sealed).expect("copy template into scenario dir");
    open_scenario_at(h, dir, sealed, plaintext)
}

/// Reopens an EXISTING scenario's scratch dir by name — the crash-
/// simulation recovery path. The orchestrator (tests/e2e-scenario/
/// orchestrator/src/bridge.ts) simulates a crash by force-killing this
/// whole OS process (the only way to truly release every SQLite file
/// handle the way a real power cut does — an in-process "forget the Db"
/// trick left a leaked handle that Windows kept the plaintext file locked
/// under, which is a Windows file-locking artifact of the SAME PROCESS
/// still existing, not a property of a real crash) and starting a fresh
/// one against the same `HOLLER_E2E_DATA_DIR`. This call is what that fresh
/// process uses to pick the scenario back up: `Db::open`'s own crash-
/// leftover recovery runs naturally here, because the plaintext file and
/// its "unclean" marker are exactly what the killed process left behind.
fn resume_scenario(h: &mut Harness, dir_name: &str) -> Value {
    let dir = h.root.join(dir_name);
    let sealed = dir.join("edge.db.enc");
    let plaintext = dir.join("edge.db");
    open_scenario_at(h, dir, sealed, plaintext)
}

fn close_scenario(h: &mut Harness) -> Value {
    if let Some(mut sc) = h.scenario.take() {
        if let Some(handle) = sc.lan_handle.take() {
            handle.shutdown();
        }
        // Best-effort clean close (reseal); a "crashed" scenario's Db was
        // already swapped for a throwaway in-memory one (see `do_crash`),
        // so this always closes cleanly regardless of crash-sim history.
        if let Ok(db) = Arc::try_unwrap(sc.state.db).map_err(|_| ()) {
            if let Ok(db) = db.into_inner() {
                let _ = db.close();
            }
        }
        let _ = std::fs::remove_dir_all(&sc.scratch_dir);
    }
    json!({})
}

fn introspect(sc: &Scenario) -> Value {
    let db = sc.state.db.lock().unwrap_or_else(|e| e.into_inner());
    let conn = db.connection();
    let orders = repo::list_orders_for_outlet(conn, &sc.state.outlet_id).unwrap_or_default();
    let mut orders_json = Vec::new();
    for o in &orders {
        let items = repo::list_order_items(conn, &o.id).unwrap_or_default();
        let mut oj = order_to_json(o);
        oj["items"] = Value::Array(items.iter().map(order_item_to_json).collect());
        orders_json.push(oj);
    }
    let kots = repo::list_kots_for_outlet(conn, &sc.state.outlet_id, None).unwrap_or_default();
    let outbox = repo::list_unpublished_outbox(conn, 10_000).unwrap_or_default();
    json!({
        "orders": orders_json,
        "kots": kots.iter().map(kot_to_json).collect::<Vec<_>>(),
        "outbox_unpublished": outbox.iter().map(outbox_to_json).collect::<Vec<_>>(),
    })
}

fn dispatch(h: &mut Harness, req: Request) -> Value {
    match req {
        Request::Ping => json!({}),
        Request::NewScenario => new_scenario(h),
        Request::CloseScenario => close_scenario(h),
        Request::ResumeScenario { dir } => resume_scenario(h, &dir),
        Request::Introspect => {
            let sc = h.scenario.as_ref().expect("scenario active");
            introspect(sc)
        }
        Request::CreateDraft {
            order_type,
            table_id,
            menu_item_id,
            variant_id,
            unit_price_paise,
            quantity,
            notes,
            modifiers,
        } => {
            let sc = h.scenario.as_ref().expect("scenario active");
            match create_order_impl(
                &sc.state,
                order_type,
                table_id,
                vec![NewOrderItemRequest {
                    menu_item_id,
                    variant_id,
                    quantity,
                    unit_price_paise,
                    notes,
                    modifiers: modifiers.into_iter().map(Into::into).collect(),
                }],
            ) {
                Ok(order) => json!({ "ok": true, "order": order }),
                Err(e) => json!({ "ok": false, "error": app_error_to_json(&e) }),
            }
        }
        Request::AddItem {
            order_id,
            menu_item_id,
            variant_id,
            unit_price_paise,
            quantity,
            notes,
            modifiers,
        } => {
            let sc = h.scenario.as_ref().expect("scenario active");
            match add_order_item_impl(
                &sc.state,
                &order_id,
                NewOrderItemRequest {
                    menu_item_id,
                    variant_id,
                    quantity,
                    unit_price_paise,
                    notes,
                    modifiers: modifiers.into_iter().map(Into::into).collect(),
                },
            ) {
                Ok(order) => json!({ "ok": true, "order": order }),
                Err(e) => json!({ "ok": false, "error": app_error_to_json(&e) }),
            }
        }
        Request::RemoveItem {
            order_id,
            order_item_id,
        } => {
            let sc = h.scenario.as_ref().expect("scenario active");
            match remove_order_item_impl(&sc.state, &order_id, &order_item_id) {
                Ok(order) => json!({ "ok": true, "order": order }),
                Err(e) => json!({ "ok": false, "error": app_error_to_json(&e) }),
            }
        }
        Request::UpdateShape {
            order_id,
            order_type,
            table_id,
        } => {
            let sc = h.scenario.as_ref().expect("scenario active");
            match update_order_shape_impl(&sc.state, &order_id, order_type, table_id) {
                Ok(order) => json!({ "ok": true, "order": order }),
                Err(e) => json!({ "ok": false, "error": app_error_to_json(&e) }),
            }
        }
        Request::Confirm { order_id } => {
            let sc = h.scenario.as_ref().expect("scenario active");
            match confirm_order_impl(&sc.state, &order_id) {
                Ok(order) => json!({ "ok": true, "order": order }),
                Err(e) => json!({ "ok": false, "error": app_error_to_json(&e) }),
            }
        }
        Request::SendToKitchen { order_id } => {
            let sc = h.scenario.as_ref().expect("scenario active");
            match send_order_to_kitchen_impl(&sc.state, &order_id) {
                Ok(kots) => json!({ "ok": true, "kots": kots }),
                Err(e) => json!({ "ok": false, "error": app_error_to_json(&e) }),
            }
        }
        Request::TransitionKot {
            order_id,
            kot_id,
            status,
        } => {
            let sc = h.scenario.as_ref().expect("scenario active");
            match transition_kot_status_impl(&sc.state, &order_id, &kot_id, &status) {
                Ok(kots) => json!({ "ok": true, "kots": kots }),
                Err(e) => json!({ "ok": false, "error": app_error_to_json(&e) }),
            }
        }
        Request::GetOrder { order_id } => {
            let sc = h.scenario.as_ref().expect("scenario active");
            match get_order_impl(&sc.state, &order_id) {
                Ok(order) => json!({ "ok": true, "order": order }),
                Err(e) => json!({ "ok": false, "error": app_error_to_json(&e) }),
            }
        }
        Request::ListKots { order_id } => {
            let sc = h.scenario.as_ref().expect("scenario active");
            match list_kots_for_order_impl(&sc.state, &order_id) {
                Ok(kots) => json!({ "ok": true, "kots": kots }),
                Err(e) => json!({ "ok": false, "error": app_error_to_json(&e) }),
            }
        }
        Request::IssueInvoice {
            order_id,
            created_by_user_id,
        } => {
            let sc = h.scenario.as_ref().expect("scenario active");
            match issue_invoice_impl(&sc.state, &order_id, &created_by_user_id) {
                Ok(invoice) => json!({ "ok": true, "invoice": invoice }),
                Err(e) => json!({ "ok": false, "error": app_error_to_json(&e) }),
            }
        }
        Request::ListInvoicesForOrder { order_id } => {
            let sc = h.scenario.as_ref().expect("scenario active");
            match list_invoices_for_order_impl(&sc.state, &order_id) {
                Ok(invoices) => json!({ "ok": true, "invoices": invoices }),
                Err(e) => json!({ "ok": false, "error": app_error_to_json(&e) }),
            }
        }
        Request::RecordPayment {
            order_id,
            method,
            amount_paise,
            tendered_paise,
            change_paise,
            reference,
            cash_shift_id,
            reverses_payment_id,
            invoice_id,
            created_by_user_id,
        } => {
            let sc = h.scenario.as_ref().expect("scenario active");
            match record_payment_impl(
                &sc.state,
                &order_id,
                &method,
                amount_paise,
                tendered_paise,
                change_paise,
                reference,
                cash_shift_id,
                reverses_payment_id,
                invoice_id,
                &created_by_user_id,
            ) {
                Ok(payment) => json!({ "ok": true, "payment": payment }),
                Err(e) => json!({ "ok": false, "error": app_error_to_json(&e) }),
            }
        }
        Request::ListPaymentsForOrder { order_id } => {
            let sc = h.scenario.as_ref().expect("scenario active");
            match list_payments_for_order_impl(&sc.state, &order_id) {
                Ok(payments) => json!({ "ok": true, "payments": payments }),
                Err(e) => json!({ "ok": false, "error": app_error_to_json(&e) }),
            }
        }
    }
}

fn main() {
    let root: PathBuf = std::env::var("HOLLER_E2E_DATA_DIR")
        .map(PathBuf::from)
        .expect("HOLLER_E2E_DATA_DIR must be set to a scratch directory (never %APPDATA%)");
    std::fs::create_dir_all(&root).expect("create scratch root");

    let template_sealed = build_template(&root);

    let mut h = Harness {
        template_sealed,
        root,
        scenario: None,
        scenario_seq: 0,
    };

    println!("{}", json!({ "ready": true }));
    std::io::stdout().flush().ok();

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "stop" {
            break;
        }
        let req: Request = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                writeln!(stdout, "{}", json!({ "ok": false, "error": { "code": "BAD_REQUEST", "message": e.to_string() } })).ok();
                stdout.flush().ok();
                continue;
            }
        };
        let resp = dispatch(&mut h, req);
        writeln!(stdout, "{resp}").ok();
        stdout.flush().ok();
    }

    close_scenario(&mut h);
}
