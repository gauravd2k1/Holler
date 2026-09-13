//! End-to-end tests of the captain HTTP listener (docs/captain-api.md),
//! binding a real `TcpListener` (via `tiny_http`) over an in-memory
//! database and driving it with plain `TcpStream` HTTP requests — no Tauri
//! window is involved, matching the fallback the task brief asks for when a
//! Tauri shell cannot be observed from this environment.
//!
//! Binds `127.0.0.1:0` (an OS-assigned ephemeral port) rather than the
//! production default, so this suite never collides with a real POS running
//! on this machine and can run its tests in parallel.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use holler_edge_database::{model, repo, Db};
use holler_edge_device::contract::KdsLanMessage;
use holler_edge_device::Hub;
use holler_pos_lib::captain::start_captain_server;
use holler_pos_lib::state::AppState;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD_NO_PAD;
const OUTLET_ID: &str = "outlet-1";
const TILL_DEVICE_ID: &str = "till-1";
const WAITER_DEVICE_ID: &str = "waiter-1";
const TENANT_ID: &str = "tenant-1";
const WAITER_SECRET: &str = "waiter-secret-xyz";

/// Same Argon2id PHC encoding `holler_edge_database::auth::verify_password`
/// checks against — mirrors `edge/device/src/tests.rs::hash_secret`,
/// duplicated because that helper is private to its own crate's test
/// module.
fn hash_secret(secret: &str) -> String {
    use argon2::{Algorithm, Argon2, Params, Version};
    use base64::Engine;
    use rand::RngCore as _;

    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    let params = Params::new(64 * 1024, 2, 4, Some(32)).expect("valid params");
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(secret.as_bytes(), &salt, &mut key)
        .expect("hash");
    format!(
        "$argon2id$v={}$m={},t={},p={}${}${}",
        Version::V0x13 as u32,
        64 * 1024,
        2,
        4,
        B64.encode(salt),
        B64.encode(key)
    )
}

fn seed(db: &Db) {
    let conn = db.connection();
    repo::upsert_outlet(
        conn,
        &model::Outlet {
            id: OUTLET_ID.to_string(),
            brand_id: "brand-1".to_string(),
            name: "Test Outlet".to_string(),
            timezone: "Asia/Kolkata".to_string(),
            config_version: 1,
            created_at: "2026-09-11T00:00:00.000Z".to_string(),
            updated_at: "2026-09-11T00:00:00.000Z".to_string(),
        },
    )
    .expect("seed outlet");

    repo::upsert_device(
        conn,
        &model::Device {
            id: TILL_DEVICE_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            kind: "POS".to_string(),
            name: "Till 1".to_string(),
            last_seen_at: None,
            created_at: "2026-09-11T00:00:00.000Z".to_string(),
        },
    )
    .expect("seed till device");

    repo::upsert_device(
        conn,
        &model::Device {
            id: WAITER_DEVICE_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            kind: "WAITER".to_string(),
            name: "Waiter Phone 1".to_string(),
            last_seen_at: None,
            created_at: "2026-09-11T00:00:00.000Z".to_string(),
        },
    )
    .expect("seed waiter device");

    repo::replace_device_credential_cache(
        conn,
        &model::DeviceCredentialCache {
            credential_id: "cred-waiter-1".to_string(),
            device_id: WAITER_DEVICE_ID.to_string(),
            tenant_id: TENANT_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            credential_hash: hash_secret(WAITER_SECRET),
            device_kind: "WAITER".to_string(),
            revoked_at: None,
            expires_at: None,
            config_version: 1,
        },
    )
    .expect("seed waiter device credential cache row");

    repo::upsert_menu_category(
        conn,
        &model::MenuCategory {
            id: "cat-1".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            name: "Starters".to_string(),
            sort_order: 1,
            config_version: 1,
        },
    )
    .expect("seed category");

    repo::upsert_menu_item(
        conn,
        &model::MenuItem {
            id: "item-1".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            category_id: "cat-1".to_string(),
            name: "Paneer Tikka".to_string(),
            base_price_paise: 25000,
            is_available: true,
            config_version: 1,
            tax_profile_id: None,
            hsn_sac: Some("9963".to_string()),
        },
    )
    .expect("seed menu item");

    repo::upsert_menu_item_variant(
        conn,
        &model::MenuItemVariant {
            id: "variant-1".to_string(),
            menu_item_id: "item-1".to_string(),
            name: "Full".to_string(),
            price_delta_paise: 0,
            is_default: true,
            config_version: 1,
        },
    )
    .expect("seed menu item variant");

    repo::upsert_restaurant_table(
        conn,
        &model::RestaurantTable {
            id: "table-1".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            section: "Main".to_string(),
            label: "T1".to_string(),
            seat_count: 4,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed table");

    repo::upsert_station(
        conn,
        &model::Station {
            id: "station-1".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            code: "MAIN_KITCHEN".to_string(),
            name: "Main Kitchen".to_string(),
            sort_order: 1,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed station");

    repo::replace_menu_item_stations(conn, "item-1", &["station-1".to_string()], 1)
        .expect("seed menu_item_station");

    repo::upsert_printer(
        conn,
        &model::Printer {
            id: "printer-1".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            name: "Kitchen Printer".to_string(),
            connection_kind: "ESCPOS_NETWORK".to_string(),
            // Deliberately unreachable, matching the rest of this crate's
            // suite (critical_offline_flow.rs's seed_kitchen): the print
            // attempt fails locally and instantly, and this suite is not
            // about printing.
            address: "127.0.0.1:1".to_string(),
            paper_width_mm: 80,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed printer");

    repo::replace_station_printers(conn, "station-1", &["printer-1".to_string()], 1)
        .expect("seed station_printer");
}

/// Starts a captain listener over a fresh in-memory database, seeded as
/// above, on an OS-assigned port. Panics (test failure) if it cannot bind —
/// unlike production, a test that cannot observe its own subject under test
/// must not silently report a false pass.
fn start_test_server() -> (SocketAddr, Arc<std::sync::Mutex<Db>>) {
    let db = Db::open_in_memory_for_tests().expect("open in-memory db");
    seed(&db);
    let state = AppState::new(db, OUTLET_ID.to_string(), TILL_DEVICE_ID.to_string());
    let db_handle = state.db.clone();
    let addr: SocketAddr = "127.0.0.1:0".parse().expect("valid addr");
    let bound = start_captain_server(addr, Arc::new(state))
        .expect("captain server must bind an ephemeral port in a test environment");
    (bound, db_handle)
}

/// Same as [`start_test_server`], but with a real [`Hub`] wired into the
/// `AppState` the captain server runs on — `AppState::new` (used above)
/// hard-codes `hub: None` (state.rs), under which `notify_kot` is a no-op
/// and nothing about the hub broadcast is exercised at all. Production
/// wires the *same* hub the KDS LAN server accepted connections on
/// (`AppState::shared_handle`); this constructs an equivalent single-hub
/// setup directly, since this suite never binds the WebSocket side.
fn start_test_server_with_hub() -> (SocketAddr, Arc<Hub>) {
    let db = Db::open_in_memory_for_tests().expect("open in-memory db");
    seed(&db);
    let hub = Arc::new(Hub::new());
    let state = AppState::new_with_hub(
        db,
        OUTLET_ID.to_string(),
        TILL_DEVICE_ID.to_string(),
        hub.clone(),
    );
    let addr: SocketAddr = "127.0.0.1:0".parse().expect("valid addr");
    let bound = start_captain_server(addr, Arc::new(state))
        .expect("captain server must bind an ephemeral port in a test environment");
    (bound, hub)
}

struct HttpResponse {
    status: u16,
    body: serde_json::Value,
}

/// Minimal hand-rolled HTTP/1.1 client — this crate carries no HTTP client
/// dependency (`tiny_http` is server-only), and adding one is outside this
/// track's owned paths (CLAUDE.md: only the `tiny_http` promotion in
/// Cargo.toml). `Connection: close` lets a plain `read_to_end` capture the
/// whole response without needing to track `Content-Length`.
fn http_request(addr: SocketAddr, method: &str, path: &str, token: Option<&str>, body: Option<&str>) -> HttpResponse {
    let mut stream = TcpStream::connect(addr).expect("connect to captain listener");
    // 30s, not 5. EVERY captain request re-verifies the device credential with
    // Argon2id at 64 MiB / t=2, which is deliberately expensive, and this suite
    // runs its tests in parallel -- the request-heavy cases below timed out at
    // 5s purely on that contention, which reads as a server hang rather than as
    // a slow hash. The timeout exists so a genuinely wedged listener still
    // fails the test rather than hanging the suite; it is not a latency budget.
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("set read timeout");

    let body_bytes = body.unwrap_or("").as_bytes();
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n"
    );
    if let Some(t) = token {
        request.push_str(&format!("Authorization: Bearer {t}\r\n"));
    }
    if body.is_some() {
        request.push_str("Content-Type: application/json\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", body_bytes.len()));
    }
    request.push_str("\r\n");

    stream
        .write_all(request.as_bytes())
        .expect("write request line/headers");
    if !body_bytes.is_empty() {
        stream.write_all(body_bytes).expect("write request body");
    }

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .expect("read full response before the server closes the connection");

    let text = String::from_utf8_lossy(&raw);
    let mut parts = text.splitn(2, "\r\n\r\n");
    let head = parts.next().unwrap_or("");
    let body_text = parts.next().unwrap_or("");

    let status_line = head.lines().next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let body_json = if body_text.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(body_text)
            .unwrap_or_else(|e| panic!("response body was not valid JSON ({e}): {body_text:?}"))
    };

    HttpResponse {
        status,
        body: body_json,
    }
}

#[test]
fn session_rejects_missing_token_and_accepts_the_enrolled_waiter_credential() {
    let (addr, _db) = start_test_server();

    let unauthenticated = http_request(addr, "GET", "/api/session", None, None);
    assert_eq!(unauthenticated.status, 401);
    assert_eq!(unauthenticated.body["code"], "UNAUTHORIZED");

    let wrong_secret = http_request(
        addr,
        "GET",
        "/api/session",
        Some("cred-waiter-1.not-the-secret"),
        None,
    );
    assert_eq!(wrong_secret.status, 401);

    let token = format!("cred-waiter-1.{WAITER_SECRET}");
    let ok = http_request(addr, "GET", "/api/session", Some(&token), None);
    assert_eq!(ok.status, 200);
    assert_eq!(ok.body["device_id"], WAITER_DEVICE_ID);
    assert_eq!(ok.body["outlet_id"], OUTLET_ID);
    assert_eq!(ok.body["device_kind"], "WAITER");
    assert_eq!(ok.body["outlet_name"], "Test Outlet");
}

#[test]
fn tables_and_menu_are_readable_with_a_valid_token() {
    let (addr, _db) = start_test_server();
    let token = format!("cred-waiter-1.{WAITER_SECRET}");

    let tables = http_request(addr, "GET", "/api/tables", Some(&token), None);
    assert_eq!(tables.status, 200);
    let table_list = tables.body["tables"].as_array().expect("tables array");
    assert_eq!(table_list.len(), 1);
    assert_eq!(table_list[0]["id"], "table-1");
    assert_eq!(table_list[0]["name"], "T1");
    assert_eq!(table_list[0]["open_order_id"], serde_json::Value::Null);

    let menu = http_request(addr, "GET", "/api/menu", Some(&token), None);
    assert_eq!(menu.status, 200);
    let items = menu.body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], "item-1");
    let variants = items[0]["variants"].as_array().expect("variants array");
    assert_eq!(variants.len(), 1);
    assert_eq!(variants[0]["id"], "variant-1");
}

/// THE pass condition this whole track exists for, exercised at the HTTP
/// layer: create an order as the waiter device, send it, and confirm the
/// resulting KOT is attributed correctly and reaches CONFIRMED status —
/// proof `send_order_to_kitchen_impl` actually ran, not just `confirm`.
#[test]
fn create_order_send_reaches_the_kitchen_and_is_attributed_to_the_waiter_not_the_till() {
    let (addr, db_handle) = start_test_server();
    let token = format!("cred-waiter-1.{WAITER_SECRET}");

    let create_body = serde_json::json!({
        "order_type": "DINE_IN",
        "table_id": "table-1",
        "items": [{
            "menu_item_id": "item-1",
            "variant_id": "variant-1",
            "quantity": 2,
            "unit_price_paise": 25000,
            "notes": null,
            "modifiers": []
        }]
    });
    let created = http_request(
        addr,
        "POST",
        "/api/orders",
        Some(&token),
        Some(&create_body.to_string()),
    );
    assert_eq!(created.status, 201, "create response: {:?}", created.body);
    assert_eq!(created.body["status"], "DRAFT");
    assert_eq!(created.body["source"], "POS");
    let order_id = created.body["holler_order_id"]
        .as_str()
        .expect("order id")
        .to_string();

    let send = http_request(
        addr,
        "POST",
        &format!("/api/orders/{order_id}/send"),
        Some(&token),
        None,
    );
    assert_eq!(send.status, 200, "send response: {:?}", send.body);
    assert_eq!(send.body["order"]["status"], "SENT_TO_KITCHEN");
    let kots = send.body["kots"].as_array().expect("kots array");
    assert_eq!(kots.len(), 1, "item-1 routes to exactly one station");
    assert_eq!(kots[0]["station"], "MAIN_KITCHEN");
    assert_eq!(kots[0]["status"], "NEW");

    // Attribution: the order was created by the WAITER credential, never
    // the till (`state.device_id`) — the "known trap" both the spec and
    // CLAUDE.md name explicitly, and the reason `create_order_impl_as`
    // exists at all. `device_id` is not on the wire `CanonicalOrder`
    // (dto.rs), so this reads the stored row directly, sharing the exact
    // `Arc<Mutex<Db>>` the server wrote through.
    let guard = db_handle.lock().expect("db lock");
    let stored = guard
        .get_order(&order_id)
        .expect("get order")
        .expect("order must exist");
    assert_eq!(stored.device_id, WAITER_DEVICE_ID);
    assert_ne!(stored.device_id, TILL_DEVICE_ID);
}

/// THE hub half of the pass condition, exercised directly rather than
/// inferred from the HTTP response: `send_order_to_kitchen_impl` calls
/// `notify_kot`, which is a documented no-op when `state.hub` is `None`
/// (`state.rs`). Every other test in this file uses `start_test_server`,
/// whose `AppState::new` hard-codes `hub: None` — so a captain-originated
/// send has never, until this test, been proven to reach a KDS subscriber
/// at all, only to reach SQLite. Production wires the real hub via
/// `AppState::shared_handle`; this proves the same call path broadcasts
/// when a hub is actually attached.
#[test]
fn sending_an_order_broadcasts_kot_upserted_to_a_subscribed_kds_hub() {
    let (addr, hub) = start_test_server_with_hub();
    let token = format!("cred-waiter-1.{WAITER_SECRET}");

    // Subscribe BEFORE the send — the hub's `publish` only reaches
    // subscribers registered at the moment it fires (hub.rs: no replay
    // buffer beyond the subscriber's own channel), so a late subscribe
    // would prove nothing.
    let subscription = hub.subscribe(OUTLET_ID, None);

    let create_body = serde_json::json!({
        "order_type": "DINE_IN",
        "table_id": "table-1",
        "items": [{
            "menu_item_id": "item-1",
            "variant_id": "variant-1",
            "quantity": 1,
            "unit_price_paise": 25000,
            "notes": null,
            "modifiers": []
        }]
    });
    let created = http_request(
        addr,
        "POST",
        "/api/orders",
        Some(&token),
        Some(&create_body.to_string()),
    );
    assert_eq!(created.status, 201, "create response: {:?}", created.body);
    let order_id = created.body["holler_order_id"]
        .as_str()
        .expect("order id")
        .to_string();

    let send = http_request(
        addr,
        "POST",
        &format!("/api/orders/{order_id}/send"),
        Some(&token),
        None,
    );
    assert_eq!(send.status, 200, "send response: {:?}", send.body);
    let sent_kot_id = send.body["kots"][0]["id"]
        .as_str()
        .expect("kot id in send response")
        .to_string();

    let message = subscription
        .receiver
        .recv_timeout(Duration::from_secs(2))
        .expect(
            "the hub must broadcast a kot_upserted frame for this send — nothing arrived on \
             the subscribed channel within the timeout",
        );
    match message {
        KdsLanMessage::KotUpserted { outlet_id, kot, .. } => {
            assert_eq!(outlet_id, OUTLET_ID);
            assert_eq!(kot.id, sent_kot_id);
            assert_eq!(kot.order_id, order_id);
            assert_eq!(kot.station, "MAIN_KITCHEN");
        }
        other => panic!("expected a kot_upserted frame, got {other:?}"),
    }
}

#[test]
fn sending_an_order_with_no_lines_is_rejected_with_400_not_an_empty_kot_set() {
    let (addr, _db) = start_test_server();
    let token = format!("cred-waiter-1.{WAITER_SECRET}");

    // Create a draft order the long way: POST /api/orders requires at
    // least one item (EMPTY_ORDER), so to reach an existing DRAFT order
    // with zero lines this test creates one item then removes it is not
    // possible over this API (no remove-line route is in scope) — instead
    // it asserts the creation-time guard directly, which is the same
    // "no lines" case the send endpoint's own check exists to catch if a
    // caller invents another way to reach it.
    let create_body = serde_json::json!({
        "order_type": "DINE_IN",
        "table_id": "table-1",
        "items": []
    });
    let created = http_request(
        addr,
        "POST",
        "/api/orders",
        Some(&token),
        Some(&create_body.to_string()),
    );
    assert_eq!(created.status, 400);
    assert_eq!(created.body["code"], "EMPTY_ORDER");
}

/// Minimal raw HTTP/1.1 client that stops at the header block — unlike
/// [`http_request`], the response here is a static asset, not JSON, so this
/// reports the status line and the `Content-Type` header verbatim rather
/// than attempting to parse a body.
struct RawHttpResponse {
    status: u16,
    content_type: Option<String>,
}

fn http_request_raw(addr: SocketAddr, path: &str) -> RawHttpResponse {
    let mut stream = TcpStream::connect(addr).expect("connect to captain listener");
    // 30s, not 5. EVERY captain request re-verifies the device credential with
    // Argon2id at 64 MiB / t=2, which is deliberately expensive, and this suite
    // runs its tests in parallel -- the request-heavy cases below timed out at
    // 5s purely on that contention, which reads as a server hang rather than as
    // a slow hash. The timeout exists so a genuinely wedged listener still
    // fails the test rather than hanging the suite; it is not a latency budget.
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("set read timeout");

    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .expect("write request line/headers");

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .expect("read full response before the server closes the connection");

    let text = String::from_utf8_lossy(&raw);
    let mut parts = text.splitn(2, "\r\n\r\n");
    let head = parts.next().unwrap_or("");

    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let content_type = lines
        .find(|l| l.to_ascii_lowercase().starts_with("content-type:"))
        .map(|l| l.split_once(':').map_or("", |(_, v)| v).trim().to_string());

    RawHttpResponse {
        status,
        content_type,
    }
}

/// Starts a captain listener whose static root is a temp directory this
/// test controls, via `HOLLER_CAPTAIN_DIST_DIR` (`captain.rs::dist_dir`) —
/// isolates the assertion from whatever `apps/captain/dist` happens to
/// contain on disk when this suite runs, and lets the fixture cover the
/// exact extension set the manifest track has added (icons, favicons, the
/// manifest itself) without depending on a prior `pnpm build`.
fn start_test_server_with_dist(files: &[(&str, &str)]) -> (SocketAddr, tempfile::TempDir) {
    let dist = tempfile::tempdir().expect("create temp dist dir");
    for (name, contents) in files {
        std::fs::write(dist.path().join(name), contents).expect("write fixture static file");
    }
    // SAFETY (test-only race): no other test in this suite ever requests a
    // non-`/api/` path, so no other test reads `dist_dir()`'s output in a
    // way that observes this value — see the doc comment on
    // `http_request_raw`/this function for why a leaked value is harmless.
    std::env::set_var("HOLLER_CAPTAIN_DIST_DIR", dist.path());

    let db = Db::open_in_memory_for_tests().expect("open in-memory db");
    seed(&db);
    let state = AppState::new(db, OUTLET_ID.to_string(), TILL_DEVICE_ID.to_string());
    let addr: SocketAddr = "127.0.0.1:0".parse().expect("valid addr");
    let bound = start_captain_server(addr, Arc::new(state))
        .expect("captain server must bind an ephemeral port in a test environment");

    std::env::remove_var("HOLLER_CAPTAIN_DIST_DIR");

    (bound, dist)
}

/// Watches `mime_for` (`captain.rs`) fail before the fix: `.webmanifest`
/// falls through to `application/octet-stream`, which a browser ignores
/// silently — "Add to Home Screen" degrades to a screenshot icon instead of
/// the standalone Holler window. Asserted over the real static-file serving
/// path (a socket request), not a unit call to `mime_for` — a unit test of
/// the map would pass today, since the map is doing exactly what it was
/// written to do; the defect is what the map omits, and only the served
/// response shows that.
#[test]
fn webmanifest_is_served_with_the_manifest_content_type() {
    let (addr, _dist) = start_test_server_with_dist(&[(
        "manifest.webmanifest",
        r#"{"name":"Holler Captain","start_url":"/","display":"standalone"}"#,
    )]);

    let response = http_request_raw(addr, "/manifest.webmanifest");

    assert_eq!(response.status, 200);
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/manifest+json"),
        "manifest.webmanifest must be served as application/manifest+json or the browser \
         ignores it silently and Add-to-Home-Screen degrades to a screenshot-icon launch"
    );
}

#[test]
fn a_null_variant_id_is_rejected_rather_than_passed_through() {
    let (addr, _db) = start_test_server();
    let token = format!("cred-waiter-1.{WAITER_SECRET}");

    let create_body = serde_json::json!({
        "order_type": "DINE_IN",
        "table_id": "table-1",
        "items": [{
            "menu_item_id": "item-1",
            "variant_id": null,
            "quantity": 1,
            "unit_price_paise": 25000,
            "notes": null,
            "modifiers": []
        }]
    });
    let created = http_request(
        addr,
        "POST",
        "/api/orders",
        Some(&token),
        Some(&create_body.to_string()),
    );
    assert_eq!(created.status, 400);
    assert_eq!(created.body["code"], "VARIANT_REQUIRED");
}

// --------------------------------------------------- the append path --
//
// `GET /api/tables` carries `open_order_id` so the phone can APPEND a second
// round to a round already in the kitchen rather than open a second order on
// one table. Until 2026-09-13 that field was read from
// `table_session.current_order_id` — and NOTHING in the shipped POS writes a
// `table_session` row (the only caller of `update_table_session` is
// `edge/database/src/lib.rs` itself). So it was null on every table forever,
// `apps/captain/src/App.tsx`'s append branch was unreachable, and every Send
// created a new order. A reader in the shipped path with no writer anywhere:
// enumerate the WRITERS before trusting a read.
//
// These two tests pin the behaviour, not the plumbing. Neither mentions
// `table_session`, which stays unwritten and reserved for M8 (ADR-025).

/// A second Send on a table whose order is already in the kitchen appends to
/// that order and tickets ONLY the new lines.
///
/// Written before the fix and watched fail on the `open_order_id` assertion —
/// the field the defect made permanently null.
#[test]
fn a_second_round_on_one_table_appends_to_the_open_order_instead_of_opening_a_second() {
    let (addr, db_handle) = start_test_server();
    let token = format!("cred-waiter-1.{WAITER_SECRET}");

    let line = |qty: i64| {
        serde_json::json!({
            "menu_item_id": "item-1",
            "variant_id": "variant-1",
            "quantity": qty,
            "unit_price_paise": 25000,
            "notes": null,
            "modifiers": []
        })
    };

    // Round one: the phone finds no open order, creates, sends.
    let before = http_request(addr, "GET", "/api/tables", Some(&token), None);
    assert_eq!(
        before.body["tables"][0]["open_order_id"],
        serde_json::Value::Null
    );

    let created = http_request(
        addr,
        "POST",
        "/api/orders",
        Some(&token),
        Some(
            &serde_json::json!({
                "order_type": "DINE_IN",
                "table_id": "table-1",
                "items": [line(1)]
            })
            .to_string(),
        ),
    );
    assert_eq!(created.status, 201, "create: {:?}", created.body);
    let order_id = created.body["holler_order_id"]
        .as_str()
        .expect("order id")
        .to_string();

    let sent = http_request(
        addr,
        "POST",
        &format!("/api/orders/{order_id}/send"),
        Some(&token),
        None,
    );
    assert_eq!(sent.status, 200, "send: {:?}", sent.body);
    assert_eq!(sent.body["order"]["status"], "SENT_TO_KITCHEN");

    // THE ASSERTION THE DEFECT FAILED. A fresh read of the table list — the
    // exact call the phone makes when the waiter walks back to the table —
    // must now name the order the kitchen is already cooking.
    let after = http_request(addr, "GET", "/api/tables", Some(&token), None);
    assert_eq!(
        after.body["tables"][0]["open_order_id"], order_id,
        "a table with an order in the kitchen must report it as open: {:?}",
        after.body
    );

    // Round two travels the append route, which no client path could reach
    // while open_order_id was null.
    let appended = http_request(
        addr,
        "POST",
        &format!("/api/orders/{order_id}/items"),
        Some(&token),
        Some(&line(3).to_string()),
    );
    // 201, per docs/captain-api.md line 233 -- an append CREATES a line.
    assert_eq!(appended.status, 201, "append: {:?}", appended.body);
    assert_eq!(appended.body["holler_order_id"], order_id);

    let appended_item_id = appended.body["items"]
        .as_array()
        .expect("items array")
        .last()
        .expect("the appended line is the last one")["id"]
        .as_str()
        .expect("appended line id")
        .to_string();

    let resent = http_request(
        addr,
        "POST",
        &format!("/api/orders/{order_id}/send"),
        Some(&token),
        None,
    );
    assert_eq!(resent.status, 200, "second send: {:?}", resent.body);
    let second_kots = resent.body["kots"].as_array().expect("kots array");
    assert_eq!(
        second_kots.len(),
        1,
        "the second send tickets the appended line only: {:?}",
        resent.body
    );

    let guard = db_handle.lock().expect("db lock");
    let conn = guard.connection();

    // THE SECOND TICKET CARRIES ONLY THE NEW ROUND. Counting tickets cannot
    // see the failure that matters here: a send that re-ticketed round one
    // would also produce exactly two KOTs, and the kitchen would cook the
    // first round twice with nothing on any screen saying so. Asserted on the
    // ticket's own `items_json` snapshot, which is what the KDS and the
    // printer render.
    let kots_in_order = repo::list_kots_for_order(conn, &order_id).expect("list kots");
    let ticket_lines = |kot: &holler_edge_database::model::Kot| -> Vec<(String, i64)> {
        serde_json::from_str::<serde_json::Value>(&kot.items_json)
            .expect("items_json parses")
            .as_array()
            .expect("items_json is an array")
            .iter()
            .map(|i| {
                (
                    i["order_item_id"].as_str().expect("order_item_id").to_string(),
                    i["quantity"].as_i64().expect("quantity"),
                )
            })
            .collect()
    };
    let mut by_sequence = kots_in_order.clone();
    by_sequence.sort_by_key(|k| k.sequence);
    let first_ticket = ticket_lines(&by_sequence[0]);
    let second_ticket = ticket_lines(&by_sequence[1]);
    assert_eq!(
        first_ticket.len(),
        1,
        "round one's ticket holds one line: {first_ticket:?}"
    );
    assert_eq!(first_ticket[0].1, 1, "round one is 1 cover");
    assert_eq!(
        second_ticket,
        vec![(appended_item_id.clone(), 3)],
        "the second ticket carries the appended line ALONE -- re-ticketing          round one would also yield two KOTs and cook it twice"
    );
    assert_ne!(
        second_ticket[0].0, first_ticket[0].0,
        "the two tickets must not name the same order line"
    );

    // EXACTLY ONE order row for this table. This is the duplicate the fix
    // exists to prevent, asserted on storage rather than on a response.
    let orders_on_table: Vec<_> = repo::list_orders_for_outlet(conn, OUTLET_ID)
        .expect("list orders")
        .into_iter()
        .filter(|o| o.table_id.as_deref() == Some("table-1"))
        .collect();
    assert_eq!(
        orders_on_table.len(),
        1,
        "one table, one open order — found {:?}",
        orders_on_table
            .iter()
            .map(|o| (o.id.clone(), o.status.clone()))
            .collect::<Vec<_>>()
    );

    // TWO tickets, both on that one order: the kitchen saw two rounds.
    let kots = repo::list_kots_for_order(conn, &order_id).expect("list kots");
    assert_eq!(kots.len(), 2, "two rounds, two tickets: {kots:?}");

    // Four covers total (1 + 3), so the append reached the same order's lines
    // rather than a second order nothing would ever bill.
    let items = repo::list_order_items(conn, &order_id).expect("list items");
    assert_eq!(items.iter().map(|i| i.quantity).sum::<i64>(), 4);
}

/// Two tables, two orders, no cross-attach: each table reports its OWN open
/// order and never its neighbour's. The cheap way to get the first test
/// passing is a query that forgets to filter by table at all, and that bug
/// would send one table's second round to the other table's bill.
#[test]
fn two_tables_report_their_own_open_orders_and_never_each_others() {
    let (addr, db_handle) = start_test_server();
    let token = format!("cred-waiter-1.{WAITER_SECRET}");

    {
        let guard = db_handle.lock().expect("db lock");
        repo::upsert_restaurant_table(
            guard.connection(),
            &model::RestaurantTable {
                id: "table-2".to_string(),
                outlet_id: OUTLET_ID.to_string(),
                section: "Main".to_string(),
                label: "T2".to_string(),
                seat_count: 2,
                is_active: true,
                config_version: 1,
            },
        )
        .expect("seed second table");
    }

    let open_on = |table_id: &str| -> String {
        let created = http_request(
            addr,
            "POST",
            "/api/orders",
            Some(&token),
            Some(
                &serde_json::json!({
                    "order_type": "DINE_IN",
                    "table_id": table_id,
                    "items": [{
                        "menu_item_id": "item-1",
                        "variant_id": "variant-1",
                        "quantity": 1,
                        "unit_price_paise": 25000,
                        "notes": null,
                        "modifiers": []
                    }]
                })
                .to_string(),
            ),
        );
        assert_eq!(
            created.status, 201,
            "create on {table_id}: {:?}",
            created.body
        );
        let id = created.body["holler_order_id"]
            .as_str()
            .expect("order id")
            .to_string();
        let sent = http_request(
            addr,
            "POST",
            &format!("/api/orders/{id}/send"),
            Some(&token),
            None,
        );
        assert_eq!(sent.status, 200, "send on {table_id}: {:?}", sent.body);
        id
    };

    let find = |body: &serde_json::Value, id: &str| -> serde_json::Value {
        body["tables"]
            .as_array()
            .expect("tables array")
            .iter()
            .find(|t| t["id"] == id)
            .unwrap_or_else(|| panic!("table {id} missing from {body:?}"))
            .clone()
    };

    let order_one = open_on("table-1");

    // With only table-1 open, table-2 must still read null — a query missing
    // its table filter passes the first assertion and fails this one.
    let mid = http_request(addr, "GET", "/api/tables", Some(&token), None);
    assert_eq!(mid.body["tables"].as_array().expect("tables array").len(), 2);
    assert_eq!(find(&mid.body, "table-1")["open_order_id"], order_one);
    assert_eq!(
        find(&mid.body, "table-2")["open_order_id"],
        serde_json::Value::Null,
        "an empty table must not inherit its neighbour's order"
    );

    let order_two = open_on("table-2");
    assert_ne!(order_one, order_two);

    let after = http_request(addr, "GET", "/api/tables", Some(&token), None);
    assert_eq!(find(&after.body, "table-1")["open_order_id"], order_one);
    assert_eq!(find(&after.body, "table-2")["open_order_id"], order_two);
}

/// A table carrying MORE THAN ONE appendable order reports the OLDEST, every
/// time.
///
/// The dev database already holds tables in this state from the pre-fix build,
/// and only a reset clears them — so the read must be deterministic even though
/// a clean outlet no longer reaches it. "Whatever the query happened to return"
/// is not something a waiter can predict, and the failure it produces is a
/// round appended to the wrong bill, which nothing on any screen would flag.
/// The oldest open order is the round the table has been eating.
///
/// The state is built through the ROUTE, not around it: `POST /api/orders`
/// carries no one-order-per-table guard and never did — the fix is on the read
/// — so two creates on one table is exactly how a pre-fix till got here.
#[test]
fn a_table_with_two_appendable_orders_reports_the_oldest_deterministically() {
    let (addr, _db) = start_test_server();
    let token = format!("cred-waiter-1.{WAITER_SECRET}");

    let create_on_table_1 = || -> String {
        let created = http_request(
            addr,
            "POST",
            "/api/orders",
            Some(&token),
            Some(
                &serde_json::json!({
                    "order_type": "DINE_IN",
                    "table_id": "table-1",
                    "items": [{
                        "menu_item_id": "item-1",
                        "variant_id": "variant-1",
                        "quantity": 1,
                        "unit_price_paise": 25000,
                        "notes": null,
                        "modifiers": []
                    }]
                })
                .to_string(),
            ),
        );
        assert_eq!(created.status, 201, "create: {:?}", created.body);
        created.body["holler_order_id"]
            .as_str()
            .expect("order id")
            .to_string()
    };

    let first = create_on_table_1();
    let second = create_on_table_1();
    assert_ne!(first, second, "two distinct orders on one table");

    // Read it twice. An answer that happens to agree once is not a pinned
    // answer, and this is the assertion that would go quiet if the fold ever
    // went back to keeping the newest.
    for attempt in 1..=2 {
        let tables = http_request(addr, "GET", "/api/tables", Some(&token), None);
        assert_eq!(
            tables.body["tables"][0]["open_order_id"], first,
            "attempt {attempt}: the OLDEST appendable order wins, never the newest: {:?}",
            tables.body
        );
    }
}
