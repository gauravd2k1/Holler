//! T10 test harness binary. See Cargo.toml and
//! tests/integration/kds-lan/README.md.
//!
//! Seeds one outlet, one POS device, one KDS device, one menu item/station
//! and sends one order to the kitchen — producing exactly one active `kot`
//! row in status NEW, mirroring the seeding pattern in
//! `edge/device/src/tests.rs::seed_one_active_kot`, but with real UUIDv7 ids
//! throughout (not the plain test strings `tests.rs` uses), because every id
//! that lands on the wire here is validated by the REAL `apps/kds` Zod
//! schemas (`KotSchema`, `KdsLanMessageSchema`), which require `z.string()
//! .uuid()`. Using non-UUID ids here would make the client-side schema
//! validation fail before this test could prove anything about the
//! handshake or the transition round-trip.
//!
//! Starts the real `holler_edge_device::server` on an ephemeral port, prints
//! one JSON line to stdout with the port and every id a driver needs, then
//! blocks reading stdin: any line (or EOF) triggers a clean shutdown and
//! exit. The parent test process kills this process either way once it is
//! done, so the read loop is a courtesy, not a requirement.

use std::io::BufRead;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;

use holler_edge_database::{model, repo, Db};
use holler_edge_device::{server, CachedCredentialVerifier, DeviceTokenVerifier};

fn uuid7() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Plaintext half of the one `device_credential_cache` row this bridge seeds
/// (ADR-017 amendment: `edge/device` now rejects any LAN connection whose
/// first frame is not a verifiable `device_token`). Fixed, and scoped to an
/// in-memory database that exists for the lifetime of one test process.
const KDS_CREDENTIAL_SECRET: &str = "kds-lan-bridge-secret";

/// Hashes `plaintext` into the PHC encoding
/// `holler_edge_database::auth::verify_password` parses, so the seeded
/// credential verifies against a real Argon2id check rather than a stub.
/// Mirrors `tests/e2e-scenario/harness`'s `hash_device_secret` — the same
/// need arrived at both bridges from the same contract change.
fn hash_device_secret(plaintext: &str) -> String {
    const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD_NO_PAD;
    let salt: [u8; 16] = *b"kds-lan-brdg-slt";
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

fn main() {
    let outlet_id = uuid7();
    let brand_id = uuid7();
    let pos_device_id = uuid7();
    let kds_device_id = uuid7();
    let category_id = uuid7();
    let item_id = uuid7();
    let station_id = uuid7();
    let order_id = uuid7();
    let order_item_id = uuid7();
    let outbox_order_id = uuid7();
    let outbox_confirm_id = uuid7();
    let tenant_id = uuid7();
    let kds_credential_id = uuid7();

    let mut db = Db::open_in_memory_for_tests().expect("open in-memory db");

    repo::upsert_outlet(
        db.connection(),
        &model::Outlet {
            id: outlet_id.clone(),
            brand_id,
            name: "T10 Bridge Outlet".to_string(),
            timezone: "Asia/Kolkata".to_string(),
            config_version: 1,
            created_at: "2026-08-10T00:00:00Z".to_string(),
            updated_at: "2026-08-10T00:00:00Z".to_string(),
        },
    )
    .expect("seed outlet");

    repo::upsert_device(
        db.connection(),
        &model::Device {
            id: pos_device_id.clone(),
            outlet_id: outlet_id.clone(),
            kind: "POS".to_string(),
            name: "Till 1".to_string(),
            last_seen_at: None,
            created_at: "2026-08-10T00:00:00Z".to_string(),
        },
    )
    .expect("seed pos device");

    repo::upsert_device(
        db.connection(),
        &model::Device {
            id: kds_device_id.clone(),
            outlet_id: outlet_id.clone(),
            kind: "KDS".to_string(),
            name: "Kitchen Screen 1".to_string(),
            last_seen_at: None,
            created_at: "2026-08-10T00:00:00Z".to_string(),
        },
    )
    .expect("seed kds device");

    // ADR-017 amendment: the LAN server verifies a `device_token` as the
    // connection's first frame, so the one KDS screen this bridge exists to
    // let a test drive needs one enrolled credential to present.
    repo::replace_device_credential_cache(
        db.connection(),
        &model::DeviceCredentialCache {
            credential_id: kds_credential_id.clone(),
            device_id: kds_device_id.clone(),
            tenant_id,
            outlet_id: outlet_id.clone(),
            credential_hash: hash_device_secret(KDS_CREDENTIAL_SECRET),
            device_kind: "KDS".to_string(),
            revoked_at: None,
            expires_at: None,
            config_version: 1,
        },
    )
    .expect("seed kds device credential");

    repo::upsert_menu_category(
        db.connection(),
        &model::MenuCategory {
            id: category_id.clone(),
            outlet_id: outlet_id.clone(),
            name: "Mains".to_string(),
            sort_order: 1,
            config_version: 1,
        },
    )
    .expect("seed category");

    repo::upsert_menu_item(
        db.connection(),
        &model::MenuItem {
            id: item_id.clone(),
            outlet_id: outlet_id.clone(),
            category_id: category_id.clone(),
            name: "Burger".to_string(),
            base_price_paise: 25000,
            is_available: true,
            config_version: 1,
            // Contracts 0.4.2 added per-item tax profile selection; `None`
            // falls back to the outlet's default profile, which is the
            // shape this bridge wants (it never issues an invoice).
            tax_profile_id: None,
            // Contracts 0.4.5 §3: an invoice cannot issue with a NULL
            // HSN/SAC. This bridge only sends to the kitchen, but the field
            // is required to construct the row — SAC 9963, restaurant
            // service, the same code the e2e harness's fixtures carry.
            hsn_sac: Some("9963".to_string()),
        },
    )
    .expect("seed menu item");

    repo::upsert_station(
        db.connection(),
        &model::Station {
            id: station_id.clone(),
            outlet_id: outlet_id.clone(),
            code: "MAIN_KITCHEN".to_string(),
            name: "MAIN_KITCHEN".to_string(),
            sort_order: 0,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed station");

    repo::replace_menu_item_stations(db.connection(), &item_id, &[station_id.clone()], 1)
        .expect("route item to station");

    let order = model::NewOrder {
        id: order_id.clone(),
        outlet_id: outlet_id.clone(),
        device_id: pos_device_id.clone(),
        order_type: "DINE_IN".to_string(),
        status: "DRAFT".to_string(),
        table_id: None,
        subtotal_paise: 25000,
        discount_paise: 0,
        taxes_paise: 0,
        total_paise: 25000,
        source: "POS".to_string(),
        external_order_id: None,
        payment_status: "UNPAID".to_string(),
        payment_source: None,
        confirmed_at: None,
        source_payload_json: None,
        schema_version: 1,
        created_at: "2026-08-10T10:00:00Z".to_string(),
        updated_at: "2026-08-10T10:00:00Z".to_string(),
    };
    let item = model::NewOrderItem {
        id: order_item_id,
        order_id: order_id.clone(),
        menu_item_id: item_id,
        variant_id: None,
        quantity: 1,
        unit_price_paise: 25000,
        line_total_paise: 25000,
        notes: None,
        created_at: "2026-08-10T10:00:00Z".to_string(),
    };
    db.create_order_with_outbox(
        &order,
        &[item],
        &model::NewOutboxEntry {
            id: outbox_order_id,
            aggregate_type: "order".to_string(),
            aggregate_id: order_id.clone(),
            event_type: "OrderCreated".to_string(),
            payload_json: "{}".to_string(),
            created_at: "2026-08-10T10:00:00Z".to_string(),
        },
    )
    .expect("create draft order");

    db.confirm_order_with_outbox(
        &order_id,
        &model::OrderConfirmedMeta {
            outbox_id: outbox_confirm_id,
            occurred_at: "2026-08-10T10:01:00Z".to_string(),
            confirmed_at: "2026-08-10T10:01:00Z".to_string(),
        },
    )
    .expect("confirm order");

    let created = db
        .send_order_to_kitchen_with_outbox(
            &order_id,
            &model::SendToKitchenMeta {
                device_id: pos_device_id,
                occurred_at: "2026-08-10T10:02:00Z".to_string(),
            },
        )
        .expect("send to kitchen");
    assert_eq!(created.len(), 1, "expected exactly one KOT from one item/one station");
    let kot_id = created[0].id.clone();

    let db = Arc::new(Mutex::new(db));
    // Local-cache verifier, no cloud fallback — this bridge never syncs,
    // exactly as `apps/pos/src-tauri/src/state.rs::start_lan_server` wires it
    // for an outlet that is offline.
    let verifier: Arc<dyn DeviceTokenVerifier> =
        Arc::new(CachedCredentialVerifier::new(db.clone(), "KDS", None));
    let handle = server::start(
        "127.0.0.1:0".parse().expect("valid loopback addr"),
        db,
        Duration::from_millis(500),
        verifier,
    )
    .expect("server starts");
    let addr = handle.local_addr();

    let ready = serde_json::json!({
        "port": addr.port(),
        "outlet_id": outlet_id,
        "kds_device_id": kds_device_id,
        // `<credential_id>.<secret>` — the exact string a KDS screen puts in
        // its first frame (apps/kds/src/lib/lanConfig.ts's AUTHENTICATION
        // note), so the driving test presents a real token rather than
        // bypassing the check.
        "kds_device_token": format!("{kds_credential_id}.{KDS_CREDENTIAL_SECRET}"),
        "kot_id": kot_id,
        "order_id": order_id,
    });
    println!("{ready}");
    use std::io::Write;
    std::io::stdout().flush().ok();

    // Block until the parent test says to stop, or stdin closes (parent
    // process exited/killed us). Either way, shut the server down cleanly
    // before exiting rather than relying solely on being killed.
    let stdin = std::io::stdin();
    let mut line = String::new();
    let _ = stdin.lock().read_line(&mut line);
    handle.shutdown();
}
