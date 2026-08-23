//! Integration tests: a real in-memory `holler_edge_database::Db` against a
//! local `tiny_http` server standing in for Holler Cloud.
//!
//! **Every receive in this harness carries a deadline.** A bare
//! `Server::recv()` blocks forever when the request it is waiting for never
//! arrives, so a test whose expectations are one request out does not fail —
//! it hangs, says nothing about why, and costs the full outer timeout per
//! iteration. A failure that names the problem in under a second is strictly
//! better than a hang that names nothing in ten minutes. See
//! [`recv_before_deadline`], and `docs/retro.md` 2026-08-23 for why this rule
//! is written down rather than remembered.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use holler_edge_database::{model, repo, Db};
use holler_edge_sync::client::HttpClient;
use holler_edge_sync::worker::{StopReason, SyncWorker, WorkerConfig};
use tiny_http::{Response, Server};

/// How long a stand-in cloud waits for a request that should already be on
/// its way. Generous next to the milliseconds a local round trip takes, and
/// still two orders of magnitude below the timeout that would otherwise be
/// reached by hanging.
const RECV_DEADLINE: Duration = Duration::from_secs(5);

/// `Server::recv()` with a deadline. `None` means the request never came,
/// which ends the responder thread so the test proceeds to its assertions and
/// fails on what it actually observed — instead of blocking forever on an
/// expectation that will never be met.
fn recv_before_deadline(server: &Server) -> Option<tiny_http::Request> {
    server.recv_timeout(RECV_DEADLINE).ok().flatten()
}

fn seed_outlet_and_device(db: &Db, outlet_id: &str, device_id: &str) {
    repo::upsert_outlet(
        db.connection(),
        &model::Outlet {
            id: outlet_id.to_string(),
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
            id: device_id.to_string(),
            outlet_id: outlet_id.to_string(),
            kind: "POS".to_string(),
            name: "Till 1".to_string(),
            last_seen_at: None,
            created_at: "2026-08-07T00:00:00Z".to_string(),
        },
    )
    .expect("seed device");
}

fn order_created_payload(order_id: &str) -> String {
    serde_json::json!({
        "event_id": "evt-1",
        "event_type": "OrderCreated",
        "occurred_at": "2026-08-07T10:00:00Z",
        "outlet_id": "outlet-1",
        "schema_version": 1,
        "data": { "order": { "holler_order_id": order_id, "total_paise": 12550i64 } }
    })
    .to_string()
}

fn seed_order_with_outbox(db: &mut Db, order_id: &str, outbox_id: &str) {
    let order = model::NewOrder {
        id: order_id.to_string(),
        outlet_id: "outlet-1".to_string(),
        device_id: "device-1".to_string(),
        order_type: "DINE_IN".to_string(),
        status: "DRAFT".to_string(),
        table_id: None,
        subtotal_paise: 12550,
        discount_paise: 0,
        taxes_paise: 0,
        total_paise: 12550,
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
    let outbox = model::NewOutboxEntry {
        id: outbox_id.to_string(),
        aggregate_type: "order".to_string(),
        aggregate_id: order_id.to_string(),
        event_type: "OrderCreated".to_string(),
        payload_json: order_created_payload(order_id),
        created_at: "2026-08-07T10:00:00Z".to_string(),
    };
    db.create_order_with_outbox(&order, &[], &outbox)
        .expect("create order with outbox");
}

fn worker_config(base_url: String) -> WorkerConfig {
    WorkerConfig {
        tenant_id: "tenant-1".to_string(),
        outlet_id: "outlet-1".to_string(),
        device_id: "device-1".to_string(),
        base_url,
        device_token: "cred-1.test-secret".to_string(),
    }
}

/// The exact path/query `SyncWorker::verify_enrollment` requests, so tests
/// that stand in for the cloud know what to expect as their first request.
const VERIFY_PATH: &str = "/sync/config?outlet_id=outlet-1&since_version=2147483647";

/// Money must be i64 paise, never float, even in test fixtures.
#[test]
fn outbox_drains_in_order_and_marks_published_without_deleting() {
    let server = Server::http("127.0.0.1:0").expect("start test server");
    let addr = server.server_addr();
    let base_url = format!("http://{addr}");

    let seen_paths: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    let seen_paths_clone = seen_paths.clone();
    let handle = std::thread::spawn(move || {
        // First request is SyncWorker::verify_enrollment (ADR-017), then the
        // two order pushes.
        for _ in 0..3 {
            if let Some(req) = recv_before_deadline(&server) {
                seen_paths_clone.lock().unwrap().push(req.url().to_string());
                let _ = req.respond(Response::from_string("{}").with_status_code(201));
            }
        }
    });

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_device(&db, "outlet-1", "device-1");
    seed_order_with_outbox(&mut db, "order-1", "outbox-1");
    seed_order_with_outbox(&mut db, "order-2", "outbox-2");

    let worker = SyncWorker::new(worker_config(base_url));
    let report = worker.pump_outbox(&mut db, 10).expect("pump");

    handle.join().unwrap();

    assert_eq!(report.published, vec!["outbox-1", "outbox-2"]);
    assert!(report.stopped.is_none());
    assert_eq!(
        *seen_paths.lock().unwrap(),
        vec![VERIFY_PATH, "/orders", "/orders"]
    );

    // Never delete local transactions after sync ack.
    assert!(db.get_order("order-1").unwrap().is_some());
    assert!(db.get_order("order-2").unwrap().is_some());

    let pending = repo::list_unpublished_outbox(db.connection(), 10).unwrap();
    assert!(pending.is_empty(), "both rows must be marked published");
}

#[test]
fn resumption_after_interruption_does_not_resend_or_skip() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_device(&db, "outlet-1", "device-1");
    seed_order_with_outbox(&mut db, "order-1", "outbox-1");
    seed_order_with_outbox(&mut db, "order-2", "outbox-2");

    let request_count = Arc::new(AtomicUsize::new(0));

    // First "session": server only ever handles one request, simulating a
    // worker interrupted after acking outbox-1 but before outbox-2.
    {
        let server = Server::http("127.0.0.1:0").expect("start test server");
        let addr = server.server_addr();
        let base_url = format!("http://{addr}");
        let count = request_count.clone();
        let handle = std::thread::spawn(move || {
            // verify_enrollment, then the outbox-1 push.
            for _ in 0..2 {
                if let Some(req) = recv_before_deadline(&server) {
                    count.fetch_add(1, Ordering::SeqCst);
                    let _ = req.respond(Response::from_string("{}").with_status_code(201));
                }
            }
            // Server dropped here — further connection attempts fail,
            // standing in for the process disappearing mid-drain.
        });
        let worker = SyncWorker::new(worker_config(base_url));
        // Limit 1: this call only sends outbox-1.
        let report = worker.pump_outbox(&mut db, 1).expect("first pump");
        assert_eq!(report.published, vec!["outbox-1"]);
        handle.join().unwrap();
    }

    assert_eq!(request_count.load(Ordering::SeqCst), 2);
    let pending = repo::list_unpublished_outbox(db.connection(), 10).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "outbox-2");

    // "Restart": a brand-new server and worker resume from local_outbox
    // state alone — nothing re-sent, nothing skipped. A fresh `SyncWorker`
    // has its own `enrollment_verified` flag, so it re-verifies too.
    let server2 = Server::http("127.0.0.1:0").expect("start second server");
    let addr2 = server2.server_addr();
    let base_url2 = format!("http://{addr2}");
    let seen = Arc::new(std::sync::Mutex::new(vec![]));
    let seen_clone = seen.clone();
    let handle2 = std::thread::spawn(move || {
        for _ in 0..2 {
            if let Ok(req) = server2.recv() {
                seen_clone.lock().unwrap().push(req.url().to_string());
                let _ = req.respond(Response::from_string("{}").with_status_code(201));
            }
        }
    });
    let worker2 = SyncWorker::new(worker_config(base_url2));
    let report2 = worker2.pump_outbox(&mut db, 10).expect("resumed pump");
    handle2.join().unwrap();

    assert_eq!(report2.published, vec!["outbox-2"]);
    let pending_after = repo::list_unpublished_outbox(db.connection(), 10).unwrap();
    assert!(pending_after.is_empty());
    // Neither order was deleted across the whole sequence.
    assert!(db.get_order("order-1").unwrap().is_some());
    assert!(db.get_order("order-2").unwrap().is_some());
}

#[test]
fn rejected_envelope_increments_attempt_count_and_computes_backoff() {
    let server = Server::http("127.0.0.1:0").expect("start test server");
    let addr = server.server_addr();
    let base_url = format!("http://{addr}");
    let handle = std::thread::spawn(move || {
        // verify_enrollment succeeds first, then the order push is rejected.
        if let Some(req) = recv_before_deadline(&server) {
            let _ = req.respond(Response::from_string("{}").with_status_code(200));
        }
        if let Some(req) = recv_before_deadline(&server) {
            let _ = req.respond(Response::from_string("{\"code\":\"boom\"}").with_status_code(500));
        }
    });

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_device(&db, "outlet-1", "device-1");
    seed_order_with_outbox(&mut db, "order-1", "outbox-1");

    let worker = SyncWorker::new(worker_config(base_url));
    let report = worker.pump_outbox(&mut db, 10).expect("pump");
    handle.join().unwrap();

    assert!(report.published.is_empty());
    assert_eq!(report.stopped, Some(StopReason::Rejected { status: 500 }));

    let pending = repo::list_unpublished_outbox(db.connection(), 10).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].attempt_count, 1, "attempt_count must increment");
    assert!(holler_edge_sync::backoff::backoff_ms(pending[0].attempt_count) > 0);

    // Order row itself must still exist — never deleted, ack or not.
    assert!(db.get_order("order-1").unwrap().is_some());
}

#[test]
fn authority_violation_is_refused_locally_and_never_sent() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_device(&db, "outlet-1", "device-1");

    // A config aggregate must never originate an outbox row in real
    // production code (only edge/database's operational-write paths insert
    // outbox rows) — this simulates a data-integrity bug producing one
    // anyway, to prove the worker refuses to send it rather than crashing
    // or, worse, posting it to a cloud-owned aggregate's route.
    db.connection()
        .execute(
            "INSERT INTO local_outbox (id, aggregate_type, aggregate_id, event_type, payload_json, created_at)
             VALUES ('outbox-bad', 'app_user', 'user-1', 'SomethingHappened', '{}', '2026-08-07T10:00:00Z')",
            (),
        )
        .expect("seed bogus outbox row");

    // No server at all: if the worker tried to send this it would fail with
    // a transport error, not silently succeed — proving absence of a
    // request is not a false negative for this test.
    let worker = SyncWorker::new(worker_config("http://127.0.0.1:1".to_string()));
    let report = worker.pump_outbox(&mut db, 10).expect("pump");

    assert_eq!(report.authority_violations, vec!["outbox-bad"]);
    assert!(report.published.is_empty());
    assert!(report.stopped.is_none());

    // Still not published, and not deleted either.
    let pending = repo::list_unpublished_outbox(db.connection(), 10).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "outbox-bad");
}

#[test]
fn offline_degrades_silently_without_panic_or_busy_loop() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_device(&db, "outlet-1", "device-1");
    seed_order_with_outbox(&mut db, "order-1", "outbox-1");

    // Port 1 is a reserved/unroutable port on virtually every platform —
    // a connection attempt fails fast with a transport error rather than
    // hanging, which is what "offline" looks like at this layer.
    let worker = SyncWorker::new(worker_config("http://127.0.0.1:1".to_string()));
    let report = worker
        .pump_outbox(&mut db, 10)
        .expect("pump must not panic when offline");

    assert!(report.published.is_empty());
    assert_eq!(report.stopped, Some(StopReason::Offline));

    let state = repo::get_sync_state(db.connection(), "outlet-1")
        .unwrap()
        .expect("sync_state row exists");
    assert!(!state.is_online);

    // The POS must remain fully functional offline: local writes still work.
    seed_order_with_outbox(&mut db, "order-2", "outbox-2");
    assert!(db.get_order("order-2").unwrap().is_some());
}

#[test]
fn config_pull_replaces_wholesale_at_newer_version_and_ignores_older_or_equal() {
    use holler_edge_sync::config::{apply_bundle, ConfigBundle};

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_device(&db, "outlet-1", "device-1");
    repo::init_sync_state(db.connection(), "outlet-1").unwrap();

    let bundle_v2_json = serde_json::json!({
        "config_version": 2,
        "users": [{
            "id": "user-1", "tenant_id": "tenant-1", "outlet_id": "outlet-1",
            "email": "cashier@example.com", "full_name": "Cashier One",
            "password_hash": "argon2id$fake-hash-for-test",
            "pin_hash": null, "is_active": true,
            "permissions": ["order.create"], "config_version": 2
        }],
        "roles": [],
        "tables": [{
            "id": "table-1", "outlet_id": "outlet-1", "section": "Main",
            "label": "T1", "seat_count": 4, "is_active": true, "config_version": 2
        }],
        "categories": [],
        "items": []
    });
    let bundle_v2: ConfigBundle = serde_json::from_value(bundle_v2_json).unwrap();

    let applied = apply_bundle(&mut db, "outlet-1", 0, bundle_v2).expect("apply v2");
    assert!(applied);

    let user = repo::get_app_user_by_id(db.connection(), "user-1")
        .unwrap()
        .expect("user replaced wholesale");
    assert_eq!(user.config_version, 2);
    assert_eq!(user.email, "cashier@example.com");

    let cursor = repo::get_sync_state(db.connection(), "outlet-1")
        .unwrap()
        .unwrap();
    assert_eq!(cursor.last_applied_config_version, 2);

    // An equal-or-older bundle must be ignored outright — not merged.
    let bundle_v1_json = serde_json::json!({
        "config_version": 1,
        "users": [{
            "id": "user-1", "tenant_id": "tenant-1", "outlet_id": "outlet-1",
            "email": "SHOULD-NOT-APPLY@example.com", "full_name": "Stale",
            "password_hash": "argon2id$stale", "pin_hash": null,
            "is_active": true, "permissions": [], "config_version": 1
        }],
        "roles": [], "tables": [], "categories": [], "items": []
    });
    let bundle_v1: ConfigBundle = serde_json::from_value(bundle_v1_json).unwrap();
    let applied_v1 = apply_bundle(&mut db, "outlet-1", 2, bundle_v1).expect("apply v1 (ignored)");
    assert!(
        !applied_v1,
        "an older-or-equal config_version must be ignored"
    );

    let user_after = repo::get_app_user_by_id(db.connection(), "user-1")
        .unwrap()
        .expect("user still present");
    assert_eq!(
        user_after.email, "cashier@example.com",
        "stale bundle must not have overwritten the newer applied config"
    );

    let cursor_after = repo::get_sync_state(db.connection(), "outlet-1")
        .unwrap()
        .unwrap();
    assert_eq!(cursor_after.last_applied_config_version, 2);
}

/// ADR-011: proves the whole config-pull path — including the one place a
/// credential-bearing HTTP response is deserialized — never lets
/// `password_hash` escape through any `Display`/error surface this crate
/// exposes, even when the pull itself fails.
#[test]
fn config_pull_error_paths_never_expose_password_hash() {
    let server = Server::http("127.0.0.1:0").expect("start test server");
    let addr = server.server_addr();
    let base_url = format!("http://{addr}");
    let handle = std::thread::spawn(move || {
        if let Some(req) = recv_before_deadline(&server) {
            // Malformed on purpose (items must be an array) so the pull
            // fails while a hash is present in the body.
            let body = r#"{"config_version":1,"users":[{"id":"u1","tenant_id":"t1","outlet_id":"o1","email":"a@b.com","full_name":"A","password_hash":"argon2id$LEAK-ME","is_active":true,"permissions":[],"config_version":1}],"roles":[],"tables":[],"categories":[],"items":"not-an-array"}"#;
            let _ = req.respond(Response::from_string(body).with_status_code(200));
        }
    });

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_device(&db, "outlet-1", "device-1");
    repo::init_sync_state(db.connection(), "outlet-1").unwrap();

    let client = HttpClient::new(base_url);
    let result = holler_edge_sync::config::pull_and_apply_config(&mut db, &client, "outlet-1");
    handle.join().unwrap();

    let err = result.expect_err("malformed bundle must fail to parse");
    let msg = err.to_string();
    assert!(!msg.contains("LEAK-ME"));
}

/// ADR-017 hole 1, falsified: a mis-enrolled node — WorkerConfig.outlet_id
/// does not match what the presented device_token actually resolves to —
/// must be stopped before it sends any envelope, not after. Stands in for
/// the cloud's real behaviour (`backend/cmd/api/syncconfig.go`: caller
/// outlet_id != device principal's own outlet_id -> 404) with a fake
/// server that rejects the verify call the same way.
///
/// Falsification performed for this track: with the
/// `!self.enrollment_verified.get()` guard in `SyncWorker::pump_outbox`
/// temporarily replaced by `if false { .. }` (skipping verification
/// entirely), this test fails — the fake server never receives the expected
/// 404 verify request, `req.respond` on the leftover queued response times
/// out the test, demonstrating the guard is load-bearing rather than
/// vacuously satisfied.
#[test]
fn mis_enrolled_outlet_id_is_rejected_before_any_envelope_is_sent() {
    let server = Server::http("127.0.0.1:0").expect("start test server");
    let addr = server.server_addr();
    let base_url = format!("http://{addr}");
    let seen_paths: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    let seen_paths_clone = seen_paths.clone();
    let handle = std::thread::spawn(move || {
        // Exactly one request must arrive: the verify ping. If a push
        // request also arrives, this loop consumes it as a second
        // "unexpected" 404 and the test's path-count assertion below
        // catches it — the pump must never get that far.
        if let Some(req) = recv_before_deadline(&server) {
            seen_paths_clone.lock().unwrap().push(req.url().to_string());
            let _ = req
                .respond(Response::from_string("{\"code\":\"not_found\"}").with_status_code(404));
        }
    });

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_device(&db, "outlet-1", "device-1");
    seed_order_with_outbox(&mut db, "order-1", "outbox-1");

    // This credential belongs to a different outlet than local config
    // claims — the fake server's 404 stands in for the cloud's real
    // rejection of that mismatch.
    let mut config = worker_config(base_url);
    config.device_token = "cred-for-a-different-outlet.secret".to_string();
    let worker = SyncWorker::new(config);
    let report = worker
        .pump_outbox(&mut db, 10)
        .expect("pump must not panic on rejection");
    handle.join().unwrap();

    assert!(
        report.published.is_empty(),
        "no envelope may be sent once verification is rejected"
    );
    assert_eq!(report.stopped, Some(StopReason::Rejected { status: 404 }));
    assert_eq!(*seen_paths.lock().unwrap(), vec![VERIFY_PATH]);

    // Never sent, never deleted.
    let pending = repo::list_unpublished_outbox(db.connection(), 10).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "outbox-1");
}
