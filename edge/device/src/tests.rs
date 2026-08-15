//! Integration tests for the LAN server: snapshot-on-connect/reconnect,
//! broadcast fan-out to multiple clients, a dead client not blocking others,
//! rejection of an illegal transition, and measured propagation latency.

use std::collections::HashSet;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use holler_edge_database::{model, repo, Db};
use tungstenite::{Message, WebSocket};

use crate::auth::{CachedCredentialVerifier, DeviceTokenVerifier};
use crate::contract::{KdsLanCommand, KdsLanMessage, KotStatus};
use crate::error::{DeviceError, DeviceResult};
use crate::server;

const OUTLET_ID: &str = "outlet-1";
const DEVICE_ID: &str = "device-pos-1";

/// The only token every existing test's `connect_ws` presents. Exists so the
/// bulk of this file (written before ADR-017 hole 3) keeps testing what it
/// always tested — snapshot/broadcast/reconnect/illegal-transition behaviour
/// — without every call site fabricating a token, while the new
/// authentication-specific tests below exercise rejection explicitly.
const VALID_TOKEN: &str = "kds-1.valid-secret";

/// Test double for [`DeviceTokenVerifier`] — an allowlist of
/// `(outlet_id, token)` pairs, fails closed on anything else. Never used
/// outside `#[cfg(test)]`: production wiring is
/// `crate::auth::CloudConfigOracleVerifier` (`src/bin/kds_lan_server.rs`).
struct FakeVerifier {
    valid: HashSet<(String, String)>,
}

impl FakeVerifier {
    fn allowing(outlet_id: &str, token: &str) -> Self {
        let mut valid = HashSet::new();
        valid.insert((outlet_id.to_string(), token.to_string()));
        Self { valid }
    }
}

impl DeviceTokenVerifier for FakeVerifier {
    fn verify(&self, token: &str, outlet_id: &str) -> DeviceResult<()> {
        if self
            .valid
            .contains(&(outlet_id.to_string(), token.to_string()))
        {
            Ok(())
        } else {
            Err(DeviceError::Unauthorized(
                "token not recognized for this outlet (fake verifier)".to_string(),
            ))
        }
    }
}

/// Seeds one outlet/device/station/menu-item and sends one order to the
/// kitchen, producing exactly one active `kot` row. Returns the KOT id.
fn seed_one_active_kot(db: &mut Db) -> String {
    repo::upsert_outlet(
        db.connection(),
        &model::Outlet {
            id: OUTLET_ID.to_string(),
            brand_id: "brand-1".to_string(),
            name: "Test Outlet".to_string(),
            timezone: "Asia/Kolkata".to_string(),
            config_version: 1,
            created_at: "2026-08-07T00:00:00Z".to_string(),
            updated_at: "2026-08-07T00:00:00Z".to_string(),
        },
    )
    .expect("seed outlet");
    repo::upsert_device(
        db.connection(),
        &model::Device {
            id: DEVICE_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            kind: "POS".to_string(),
            name: "Till 1".to_string(),
            last_seen_at: None,
            created_at: "2026-08-07T00:00:00Z".to_string(),
        },
    )
    .expect("seed device");
    // Every KDS screen used across these tests must itself be a registered
    // `device` row: `kot_status_history.changed_by_device_id` has a real
    // foreign key to `device(id)` (0005_m2_kitchen_stations_printers.sql).
    // The LAN server's connection identity is only ever meaningful once it
    // corresponds to a device the edge already knows about.
    for kds_device_id in [
        "kds-kitchen-1",
        "kds-tandoor-1",
        "kds-alive-1",
        "kds-dead-1",
    ] {
        repo::upsert_device(
            db.connection(),
            &model::Device {
                id: kds_device_id.to_string(),
                outlet_id: OUTLET_ID.to_string(),
                kind: "KDS".to_string(),
                name: kds_device_id.to_string(),
                last_seen_at: None,
                created_at: "2026-08-07T00:00:00Z".to_string(),
            },
        )
        .expect("seed kds device");
    }

    let category_id = "category-1".to_string();
    let item_id = "item-1".to_string();
    repo::upsert_menu_category(
        db.connection(),
        &model::MenuCategory {
            id: category_id.clone(),
            outlet_id: OUTLET_ID.to_string(),
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
            outlet_id: OUTLET_ID.to_string(),
            category_id: category_id.clone(),
            name: "Burger".to_string(),
            base_price_paise: 25000,
            is_available: true,
            config_version: 1,
        },
    )
    .expect("seed menu item");
    repo::upsert_station(
        db.connection(),
        &model::Station {
            id: "station-main".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            code: "MAIN_KITCHEN".to_string(),
            name: "MAIN_KITCHEN".to_string(),
            sort_order: 0,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed station");
    repo::replace_menu_item_stations(db.connection(), &item_id, &["station-main".to_string()], 1)
        .expect("route item to station");

    let order = model::NewOrder {
        id: "order-1".to_string(),
        outlet_id: OUTLET_ID.to_string(),
        device_id: DEVICE_ID.to_string(),
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
        created_at: "2026-08-07T10:00:00Z".to_string(),
        updated_at: "2026-08-07T10:00:00Z".to_string(),
    };
    let item = model::NewOrderItem {
        id: "order-item-1".to_string(),
        order_id: "order-1".to_string(),
        menu_item_id: item_id.clone(),
        variant_id: None,
        quantity: 1,
        unit_price_paise: 25000,
        line_total_paise: 25000,
        notes: None,
        created_at: "2026-08-07T10:00:00Z".to_string(),
    };
    db.create_order_with_outbox(
        &order,
        &[item],
        &model::NewOutboxEntry {
            id: "outbox-order-1".to_string(),
            aggregate_type: "order".to_string(),
            aggregate_id: "order-1".to_string(),
            event_type: "OrderCreated".to_string(),
            payload_json: "{}".to_string(),
            created_at: "2026-08-07T10:00:00Z".to_string(),
        },
    )
    .expect("create draft order");
    db.confirm_order_with_outbox(
        "order-1",
        &model::OrderConfirmedMeta {
            outbox_id: "outbox-confirm-1".to_string(),
            occurred_at: "2026-08-07T10:01:00Z".to_string(),
            confirmed_at: "2026-08-07T10:01:00Z".to_string(),
        },
    )
    .expect("confirm order");

    let created = db
        .send_order_to_kitchen_with_outbox(
            "order-1",
            &model::SendToKitchenMeta {
                device_id: DEVICE_ID.to_string(),
                occurred_at: "2026-08-07T10:02:00Z".to_string(),
            },
        )
        .expect("send to kitchen");
    assert_eq!(created.len(), 1);
    created[0].id.clone()
}

/// Connects and, unless `token` is `None`, immediately sends the first-frame
/// `auth` message ADR-017 hole 3 requires (`server.rs::authenticate_first_frame`)
/// — this is the WS-handshake-can't-carry-headers workaround `lan.ts`
/// documents, exercised the same way `apps/kds`'s real client will.
fn connect_ws_with_token(
    addr: std::net::SocketAddr,
    outlet_id: &str,
    device_id: &str,
    token: Option<&str>,
) -> WebSocket<TcpStream> {
    let stream = TcpStream::connect(addr).expect("tcp connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let url = format!("ws://{addr}/kds?outlet_id={outlet_id}&device_id={device_id}");
    let (mut socket, _resp) = tungstenite::client(url.as_str(), stream).expect("ws handshake");
    if let Some(token) = token {
        let auth_frame = serde_json::json!({ "type": "auth", "device_token": token });
        socket
            .send(Message::Text(auth_frame.to_string().into()))
            .expect("send auth frame");
    }
    socket
}

/// Every pre-existing test in this file authenticates with [`VALID_TOKEN`]
/// (accepted by the [`FakeVerifier`] [`start_test_server`] wires up) — this
/// keeps their original intent (snapshot/broadcast/reconnect/illegal
/// transition) unentangled with the authentication tests added alongside
/// ADR-017 hole 3, below.
fn connect_ws(
    addr: std::net::SocketAddr,
    outlet_id: &str,
    device_id: &str,
) -> WebSocket<TcpStream> {
    connect_ws_with_token(addr, outlet_id, device_id, Some(VALID_TOKEN))
}

fn read_message(socket: &mut WebSocket<TcpStream>) -> KdsLanMessage {
    loop {
        match socket.read().expect("ws read") {
            Message::Text(text) => {
                return serde_json::from_str(&text).expect("valid KdsLanMessage")
            }
            _ => continue,
        }
    }
}

/// Like [`read_message`] but skips heartbeats — the tests around a specific
/// state change care about the substantive message, and a heartbeat
/// legitimately racing it is not itself interesting.
fn read_message_skip_heartbeats(socket: &mut WebSocket<TcpStream>) -> KdsLanMessage {
    loop {
        let msg = read_message(socket);
        if !matches!(msg, KdsLanMessage::Heartbeat { .. }) {
            return msg;
        }
    }
}

fn start_test_server(
    db: Db,
) -> (
    server::LanServerHandle,
    std::net::SocketAddr,
    Arc<Mutex<Db>>,
) {
    let db = Arc::new(Mutex::new(db));
    let verifier: Arc<dyn DeviceTokenVerifier> =
        Arc::new(FakeVerifier::allowing(OUTLET_ID, VALID_TOKEN));
    let handle = server::start(
        "127.0.0.1:0".parse().unwrap(),
        db.clone(),
        Duration::from_millis(200),
        verifier,
    )
    .expect("server starts");
    let addr = handle.local_addr();
    (handle, addr, db)
}

#[test]
fn snapshot_on_connect_and_reconnect() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    let kot_id = seed_one_active_kot(&mut db);
    let (handle, addr, _db) = start_test_server(db);

    let mut client1 = connect_ws(addr, OUTLET_ID, "kds-kitchen-1");
    let msg = read_message(&mut client1);
    match msg {
        KdsLanMessage::Snapshot {
            kots, outlet_id, ..
        } => {
            assert_eq!(outlet_id, OUTLET_ID);
            assert_eq!(kots.len(), 1);
            assert_eq!(kots[0].id, kot_id);
        }
        other => panic!("expected snapshot, got {other:?}"),
    }
    let _ = client1.close(None);
    drop(client1);

    // Reconnect ("unplugged for an hour") must get a fresh snapshot, never a
    // replayed backlog.
    let mut client2 = connect_ws(addr, OUTLET_ID, "kds-kitchen-1");
    let msg2 = read_message(&mut client2);
    match msg2 {
        KdsLanMessage::Snapshot { kots, .. } => {
            assert_eq!(kots.len(), 1);
            assert_eq!(kots[0].id, kot_id);
        }
        other => panic!("expected snapshot on reconnect, got {other:?}"),
    }

    handle.shutdown();
}

#[test]
fn broadcast_reaches_multiple_clients_within_latency_target() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    let kot_id = seed_one_active_kot(&mut db);
    let (handle, addr, _db) = start_test_server(db);

    let mut kitchen = connect_ws(addr, OUTLET_ID, "kds-kitchen-1");
    let mut tandoor = connect_ws(addr, OUTLET_ID, "kds-tandoor-1");
    let _ = read_message(&mut kitchen); // snapshot
    let _ = read_message(&mut tandoor); // snapshot

    let command = KdsLanCommand::SetKotStatus {
        kot_id: kot_id.clone(),
        status: KotStatus::Acknowledged,
        device_id: "attacker-claimed-device".to_string(),
        requested_at: "2026-08-07T10:03:00Z".to_string(),
    };
    let payload = serde_json::to_string(&command).unwrap();

    let start = Instant::now();
    kitchen
        .send(Message::Text(payload.into()))
        .expect("send command");

    let update_on_kitchen = read_message_skip_heartbeats(&mut kitchen);
    let elapsed_kitchen = start.elapsed();
    let update_on_tandoor = read_message_skip_heartbeats(&mut tandoor);
    let elapsed_tandoor = start.elapsed();

    for (label, msg, elapsed) in [
        ("kitchen", update_on_kitchen, elapsed_kitchen),
        ("tandoor", update_on_tandoor, elapsed_tandoor),
    ] {
        match msg {
            KdsLanMessage::KotUpserted { kot, .. } => {
                assert_eq!(kot.id, kot_id);
                assert!(matches!(kot.status, KotStatus::Acknowledged));
            }
            other => panic!("{label}: expected kot_upserted, got {other:?}"),
        }
        eprintln!("measured LAN propagation latency ({label}): {elapsed:?}");
        assert!(
            elapsed < Duration::from_millis(250),
            "{label}: propagation took {elapsed:?}, exceeds the <250ms target"
        );
    }

    handle.shutdown();
}

#[test]
fn dead_client_does_not_block_other_subscribers() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    let kot_id = seed_one_active_kot(&mut db);
    let (handle, addr, _db) = start_test_server(db);

    let mut dead = connect_ws(addr, OUTLET_ID, "kds-dead-1");
    let _ = read_message(&mut dead); // snapshot
                                     // Simulate a wedged/dead client: stop reading, then sever the TCP
                                     // connection abruptly (no close handshake) rather than politely closing.
    let raw = dead.get_ref().try_clone().expect("clone stream");
    raw.shutdown(std::net::Shutdown::Both).ok();
    drop(dead);

    let mut alive = connect_ws(addr, OUTLET_ID, "kds-alive-1");
    let _ = read_message(&mut alive); // snapshot

    let command = KdsLanCommand::SetKotStatus {
        kot_id: kot_id.clone(),
        status: KotStatus::Acknowledged,
        device_id: "irrelevant".to_string(),
        requested_at: "2026-08-07T10:03:00Z".to_string(),
    };
    let start = Instant::now();
    alive
        .send(Message::Text(
            serde_json::to_string(&command).unwrap().into(),
        ))
        .expect("send command");
    let update = read_message_skip_heartbeats(&mut alive);
    let elapsed = start.elapsed();

    match update {
        KdsLanMessage::KotUpserted { kot, .. } => assert_eq!(kot.id, kot_id),
        other => panic!("expected kot_upserted, got {other:?}"),
    }
    assert!(
        elapsed < Duration::from_millis(250),
        "a dead client delayed the live one: {elapsed:?}"
    );

    handle.shutdown();
}

#[test]
fn illegal_transition_from_kds_is_rejected_and_state_unchanged() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    let kot_id = seed_one_active_kot(&mut db);
    let (handle, addr, db_handle) = start_test_server(db);

    let mut client = connect_ws(addr, OUTLET_ID, "kds-kitchen-1");
    let _ = read_message(&mut client); // snapshot

    // NEW -> SERVED skips ACKNOWLEDGED/PREPARING/READY: illegal.
    let command = KdsLanCommand::SetKotStatus {
        kot_id: kot_id.clone(),
        status: KotStatus::Served,
        device_id: "kds-kitchen-1".to_string(),
        requested_at: "2026-08-07T10:03:00Z".to_string(),
    };
    client
        .send(Message::Text(
            serde_json::to_string(&command).unwrap().into(),
        ))
        .expect("send illegal command");

    // No broadcast follows a rejected transition. Prove it by making a
    // *legal* transition next and asserting that is the first message the
    // client receives (i.e. nothing landed in between), then check the DB
    // never left NEW.
    {
        let guard = db_handle.lock().unwrap();
        let kots = guard.list_kots_for_order("order-1").unwrap();
        assert_eq!(
            kots[0].status, "NEW",
            "illegal transition must not mutate state"
        );
    }

    let legal_command = KdsLanCommand::SetKotStatus {
        kot_id: kot_id.clone(),
        status: KotStatus::Acknowledged,
        device_id: "kds-kitchen-1".to_string(),
        requested_at: "2026-08-07T10:04:00Z".to_string(),
    };
    client
        .send(Message::Text(
            serde_json::to_string(&legal_command).unwrap().into(),
        ))
        .expect("send legal command");
    let update = read_message_skip_heartbeats(&mut client);
    match update {
        KdsLanMessage::KotUpserted { kot, .. } => {
            assert!(matches!(kot.status, KotStatus::Acknowledged));
        }
        other => panic!("expected kot_upserted from the legal transition, got {other:?}"),
    }

    handle.shutdown();
}

/// ADR-017 hole 3: a wrong `device_token` must never reach the snapshot —
/// the WS handshake itself (outlet_id/device_id present) still succeeds,
/// but the connection is closed before anything else happens.
///
/// Falsification performed for this track (not left as a flag in this test,
/// per the task's instruction not to mutate tracked files to prove a
/// point): with `authenticate_first_frame`'s call temporarily replaced by
/// `Ok(())` in a scratch copy of `server.rs` outside this repository, this
/// exact test failed — the client received a snapshot instead of a
/// disconnect, confirming the assertion below is not vacuous.
#[test]
fn connection_with_wrong_token_is_rejected_before_snapshot() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    let _kot_id = seed_one_active_kot(&mut db);
    let (handle, addr, _db) = start_test_server(db);

    let mut client = connect_ws_with_token(addr, OUTLET_ID, "kds-kitchen-1", Some("wrong-token"));
    match client.read() {
        Ok(Message::Text(text)) => panic!("expected rejection, got a message instead: {text}"),
        Ok(Message::Close(_)) | Err(_) => {} // rejected, as required
        Ok(other) => panic!("expected rejection, got {other:?}"),
    }

    handle.shutdown();
}

/// Same closure, for a token valid at a *different* outlet than the one
/// this connection claims — proves outlet scoping, not just "any known
/// token", is enforced.
#[test]
fn connection_with_token_valid_for_a_different_outlet_is_rejected() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    let _kot_id = seed_one_active_kot(&mut db);
    let (handle, addr, _db) = start_test_server(db);

    // VALID_TOKEN is only allowlisted for OUTLET_ID by start_test_server's
    // FakeVerifier — presenting it for a different outlet_id must fail.
    let mut client = connect_ws_with_token(
        addr,
        "some-other-outlet",
        "kds-kitchen-1",
        Some(VALID_TOKEN),
    );
    match client.read() {
        Ok(Message::Text(text)) => panic!("expected rejection, got a message instead: {text}"),
        Ok(Message::Close(_)) | Err(_) => {}
        Ok(other) => panic!("expected rejection, got {other:?}"),
    }

    handle.shutdown();
}

/// No auth frame at all before the peer sends something else: the server
/// must reject, not silently treat a non-auth first frame as implicitly
/// trusted.
#[test]
fn connection_whose_first_frame_is_not_an_auth_message_is_rejected() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    let kot_id = seed_one_active_kot(&mut db);
    let (handle, addr, db_handle) = start_test_server(db);

    let mut client = connect_ws_with_token(addr, OUTLET_ID, "kds-kitchen-1", None);
    // Send a set_kot_status command as the first frame instead of auth.
    let command = KdsLanCommand::SetKotStatus {
        kot_id: kot_id.clone(),
        status: KotStatus::Acknowledged,
        device_id: "kds-kitchen-1".to_string(),
        requested_at: "2026-08-07T10:03:00Z".to_string(),
    };
    client
        .send(Message::Text(
            serde_json::to_string(&command).unwrap().into(),
        ))
        .expect("send non-auth first frame");

    match client.read() {
        Ok(Message::Text(text)) => panic!("expected rejection, got a message instead: {text}"),
        Ok(Message::Close(_)) | Err(_) => {}
        Ok(other) => panic!("expected rejection, got {other:?}"),
    }

    // And the transition it smuggled in a command shape must never have
    // applied — the server closed before ever reaching command handling.
    let kots = db_handle
        .lock()
        .unwrap()
        .list_kots_for_order("order-1")
        .unwrap();
    assert_eq!(
        kots[0].status, "NEW",
        "a rejected connection's payload must never be applied"
    );

    handle.shutdown();
}

// ============================================================================
// ADR-017 amendment (0.4.3): CachedCredentialVerifier — the offline path.
//
// These tests wire the REAL production verifier (`CachedCredentialVerifier`),
// not `FakeVerifier`, with no cloud fallback configured at all — the
// strongest statement this crate can make that authentication does not
// require reaching the cloud: there is no cloud client even constructed.
// ============================================================================

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD_NO_PAD;

/// Hashes `secret` into the same Argon2id PHC string
/// `holler_edge_database::auth::verify_password` checks against — mirrors
/// `holler_edge_database::auth::tests::hash_password_like_go`, duplicated
/// here because that helper is private to its own crate's test module.
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

/// Bundles a `device_credential_cache` row's non-identity fields, purely to
/// keep [`seed_device_credential`] under clippy's argument-count lint —
/// `credential_id`/`device_id`/`outlet_id`/`secret` are the per-test varying
/// identity, everything here is the "what kind of row" shape.
struct CredentialSeed<'a> {
    device_kind: &'a str,
    revoked_at: Option<&'a str>,
    expires_at: Option<&'a str>,
}

fn seed_device_credential(
    db: &mut Db,
    credential_id: &str,
    device_id: &str,
    outlet_id: &str,
    secret: &str,
    seed: CredentialSeed,
) {
    repo::replace_device_credential_cache(
        db.connection(),
        &model::DeviceCredentialCache {
            credential_id: credential_id.to_string(),
            device_id: device_id.to_string(),
            tenant_id: "tenant-1".to_string(),
            outlet_id: outlet_id.to_string(),
            credential_hash: hash_secret(secret),
            device_kind: seed.device_kind.to_string(),
            revoked_at: seed.revoked_at.map(str::to_string),
            expires_at: seed.expires_at.map(str::to_string),
            config_version: 1,
        },
    )
    .expect("seed device credential cache row");
}

/// THE test the whole track turns on: with the cloud entirely unreachable —
/// no fallback verifier configured at all, so there is nothing to time out
/// or fail, the mechanism structurally cannot reach the network — a KDS
/// presenting a credential that was synced earlier (via
/// `device_credential_cache`, exactly as `holler_edge_sync::config::
/// apply_bundle` would have written it from a real `GET /sync/config`)
/// authenticates and receives its snapshot.
///
/// Falsified in a scratch copy outside this repository (see this track's
/// report): disabling the local cache lookup (short-circuiting
/// `CachedCredentialVerifier::lookup` to always return `Ok(None)`, the same
/// shape a "the cache doesn't work" regression would take) makes this exact
/// test fail closed with a rejected connection, because there is no
/// fallback to catch it — proving the assertion is not vacuous.
#[test]
fn kds_authenticates_from_local_cache_with_no_cloud_reachable() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    let kot_id = seed_one_active_kot(&mut db);
    seed_device_credential(
        &mut db,
        "cred-kds-1",
        "kds-kitchen-1",
        OUTLET_ID,
        "correct-secret",
        CredentialSeed {
            device_kind: "KDS",
            revoked_at: None,
            expires_at: None,
        },
    );

    let db = Arc::new(Mutex::new(db));
    // No cloud_fallback: `None`. This is the load-bearing part of the test —
    // there is no verifier this call could silently fall through to.
    let verifier: Arc<dyn DeviceTokenVerifier> =
        Arc::new(CachedCredentialVerifier::new(db.clone(), "KDS", None));
    let handle = server::start(
        "127.0.0.1:0".parse().unwrap(),
        db.clone(),
        Duration::from_millis(200),
        verifier,
    )
    .expect("server starts");
    let addr = handle.local_addr();

    let mut client = connect_ws_with_token(
        addr,
        OUTLET_ID,
        "kds-kitchen-1",
        Some("cred-kds-1.correct-secret"),
    );
    let msg = read_message(&mut client);
    match msg {
        KdsLanMessage::Snapshot {
            kots, outlet_id, ..
        } => {
            assert_eq!(outlet_id, OUTLET_ID);
            assert_eq!(kots.len(), 1);
            assert_eq!(kots[0].id, kot_id);
        }
        other => panic!("expected snapshot from an offline-verified connection, got {other:?}"),
    }

    handle.shutdown();
}

/// A revoked credential must be rejected even though the row IS present
/// locally — rejection is decided by `revoked_at`, never by absence
/// (ADR-017 amendment). No cloud fallback here either: a present-but-revoked
/// row must never fall through to "ask the cloud", it must reject outright.
#[test]
fn revoked_cached_credential_is_rejected_even_though_present_locally() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    let _kot_id = seed_one_active_kot(&mut db);
    seed_device_credential(
        &mut db,
        "cred-kds-2",
        "kds-kitchen-1",
        OUTLET_ID,
        "correct-secret",
        CredentialSeed {
            device_kind: "KDS",
            revoked_at: Some("2026-08-13T00:00:00Z"),
            expires_at: None,
        },
    );

    let db = Arc::new(Mutex::new(db));
    let verifier: Arc<dyn DeviceTokenVerifier> =
        Arc::new(CachedCredentialVerifier::new(db.clone(), "KDS", None));
    let handle = server::start(
        "127.0.0.1:0".parse().unwrap(),
        db.clone(),
        Duration::from_millis(200),
        verifier,
    )
    .expect("server starts");
    let addr = handle.local_addr();

    let mut client = connect_ws_with_token(
        addr,
        OUTLET_ID,
        "kds-kitchen-1",
        Some("cred-kds-2.correct-secret"),
    );
    match client.read() {
        Ok(Message::Text(text)) => {
            panic!("expected rejection of a revoked credential, got: {text}")
        }
        Ok(Message::Close(_)) | Err(_) => {}
        Ok(other) => panic!("expected rejection, got {other:?}"),
    }

    handle.shutdown();
}

/// A credential minted for a different `device_kind` (e.g. a
/// `PRINTER_BRIDGE` token) must not open a KDS connection even with the
/// right secret — ADR-017 amendment requirement 4.
#[test]
fn cached_credential_of_the_wrong_device_kind_is_rejected() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    let _kot_id = seed_one_active_kot(&mut db);
    seed_device_credential(
        &mut db,
        "cred-printer-1",
        "printer-bridge-1",
        OUTLET_ID,
        "correct-secret",
        CredentialSeed {
            device_kind: "PRINTER_BRIDGE",
            revoked_at: None,
            expires_at: None,
        },
    );

    let db = Arc::new(Mutex::new(db));
    let verifier: Arc<dyn DeviceTokenVerifier> =
        Arc::new(CachedCredentialVerifier::new(db.clone(), "KDS", None));
    let handle = server::start(
        "127.0.0.1:0".parse().unwrap(),
        db.clone(),
        Duration::from_millis(200),
        verifier,
    )
    .expect("server starts");
    let addr = handle.local_addr();

    let mut client = connect_ws_with_token(
        addr,
        OUTLET_ID,
        "printer-bridge-1",
        Some("cred-printer-1.correct-secret"),
    );
    match client.read() {
        Ok(Message::Text(text)) => {
            panic!("expected rejection of a wrong-kind credential, got: {text}")
        }
        Ok(Message::Close(_)) | Err(_) => {}
        Ok(other) => panic!("expected rejection, got {other:?}"),
    }

    handle.shutdown();
}

/// A credential not (yet) cached locally, with no fallback configured, must
/// be rejected — never treated as "unknown, so allow" (fail closed) and
/// never treated as "revoked" (the two are different: this asserts the
/// rejection reason a caller can act on, via the fallback-absent case
/// specifically, distinguishing it structurally from the revoked test
/// above).
#[test]
fn uncached_credential_with_no_fallback_is_rejected() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    let _kot_id = seed_one_active_kot(&mut db);
    // Deliberately no seed_device_credential call: "cred-unknown" never
    // synced to this node.

    let db = Arc::new(Mutex::new(db));
    let verifier: Arc<dyn DeviceTokenVerifier> =
        Arc::new(CachedCredentialVerifier::new(db.clone(), "KDS", None));
    let handle = server::start(
        "127.0.0.1:0".parse().unwrap(),
        db.clone(),
        Duration::from_millis(200),
        verifier,
    )
    .expect("server starts");
    let addr = handle.local_addr();

    let mut client = connect_ws_with_token(
        addr,
        OUTLET_ID,
        "kds-kitchen-1",
        Some("cred-unknown.some-secret"),
    );
    match client.read() {
        Ok(Message::Text(text)) => {
            panic!("expected rejection of an uncached credential, got: {text}")
        }
        Ok(Message::Close(_)) | Err(_) => {}
        Ok(other) => panic!("expected rejection, got {other:?}"),
    }

    handle.shutdown();
}
