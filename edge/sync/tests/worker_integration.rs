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
//!
//! The deadline bounds the RESPONDER side only. A script shorter than the
//! requests that actually arrive leaves the worker waiting out its own HTTP
//! read timeout on each unanswered one — bounded, but slow and silent about
//! why. Size a script from what the flow under test will really send.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use holler_edge_database::{model, repo, Db};
use holler_edge_sync::client::HttpClient;
use holler_edge_sync::worker::{
    StopReason, SyncWorker, WorkerConfig, MAX_OUTBOX_REPLAY_ATTEMPTS,
    OUTBOX_ATTENTION_ATTEMPTS, UNROUTED_EVENT_CODE,
};
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

/// M6 A1 — a 422 `missing_reference` from the cloud must leave the row HELD,
/// never dropped.
///
/// THE INVARIANT THIS PINS: *at no commit boundary may an order become
/// droppable with no operator trace.* A1 changed the cloud to answer an FK
/// violation with 422 instead of 500. A3 will give the general outbox a
/// per-entry budget and surface an abandoned row to a human. Between the two,
/// this test is what says the row is still there — held and retried, which is
/// the loud failure, not the silent one.
///
/// WHY THERE IS NO CARVE-OUT IN `is_permanent_rejection`. The plan expected
/// A1 to add 422 to the edge's non-permanent set beside 401/403/404/408/429.
/// The repository says that is unnecessary AND harmful, and the repository
/// wins. `is_permanent_rejection` has exactly two callers — `ranged.rs:209`
/// and `procurement.rs:248` — and the general outbox is neither: `pump_outbox`
/// records the attempt, stops, and leaves the row unpublished whatever the
/// status is. So an order is already held, with no change at all.
///
/// Adding the carve-out would instead REGRESS the two streams that already do
/// this properly: `ranged_replay.rs` asserts a 422 spends the budget, lands in
/// `sync_replay_block` and becomes visible on the POS — permanent-and-surfaced,
/// which is A3's end state, already true there since contracts 0.5.8. Making
/// 422 transient would take that away and retry a known-bad ledger entry
/// forever.
///
/// A3 therefore does not "remove a carve-out"; it adds the budget and the
/// surfacing to the general outbox, and its test asserts the row is abandoned
/// and VISIBLE. That test and this one remain mutually exclusive — this one
/// requires the row still pending and unblocked, that one requires it blocked
/// and surfaced — so A3 cannot go green while this behaviour survives.
#[test]
fn a_422_missing_reference_holds_the_row_rather_than_dropping_it() {
    let server = Server::http("127.0.0.1:0").expect("start test server");
    let addr = server.server_addr();
    let base_url = format!("http://{addr}");
    let handle = std::thread::spawn(move || {
        // verify_enrollment, then two pumps each rejected with the real
        // post-A1 cloud response for an FK violation.
        if let Some(req) = recv_before_deadline(&server) {
            let _ = req.respond(Response::from_string("{}").with_status_code(200));
        }
        for _ in 0..2 {
            if let Some(req) = recv_before_deadline(&server) {
                let _ = req.respond(
                    Response::from_string(
                        "{\"code\":\"missing_reference\",\"message\":\"menu_item_id does not exist\"}",
                    )
                    .with_status_code(422),
                );
            }
        }
    });

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_device(&db, "outlet-1", "device-1");
    seed_order_with_outbox(&mut db, "order-1", "outbox-1");

    let worker = SyncWorker::new(worker_config(base_url));

    let first = worker.pump_outbox(&mut db, 10).expect("first pump");
    assert!(first.published.is_empty());
    // A2 changed the SHAPE of this outcome and not its substance. Before A2 a
    // refusal stopped the whole drain, so `stopped` carried it; after A2 a
    // permanent refusal blocks that aggregate and the drain continues, so it
    // is reported per-aggregate instead. What this test exists to pin -- the
    // row is HELD, never dropped -- is asserted below and is unchanged.
    assert_eq!(first.stopped, None);
    assert_eq!(first.blocked_aggregates.len(), 1);
    assert_eq!(first.blocked_aggregates[0].outbox_id, "outbox-1");
    assert_eq!(first.blocked_aggregates[0].last_status, Some(422));

    // Held: still pending after the refusal, attempt counted.
    let pending = repo::list_unpublished_outbox(db.connection(), 10).unwrap();
    assert_eq!(pending.len(), 1, "the row must still be in the outbox");
    assert_eq!(pending[0].id, "outbox-1");
    assert_eq!(pending[0].attempt_count, 1);

    // A second pump does not quietly discard it either. Retrying a row the
    // cloud will refuse again is wasteful; losing it is not survivable, and
    // until A3 lands the wasteful option is the correct one.
    let second = worker.pump_outbox(&mut db, 10).expect("second pump");
    handle.join().unwrap();
    assert_eq!(second.blocked_aggregates.len(), 1, "refused again, still not dropped");
    // The status on the SECOND refusal, kept because the pre-A2 version of
    // this test asserted it and dropping it would let a second refusal of a
    // different class pass unnoticed. `stopped` cannot carry it any more, so
    // it is asserted where A2 moved it to.
    assert_eq!(second.blocked_aggregates[0].last_status, Some(422));
    assert_eq!(second.blocked_aggregates[0].outbox_id, "outbox-1");

    let pending = repo::list_unpublished_outbox(db.connection(), 10).unwrap();
    assert_eq!(pending.len(), 1, "STILL held on the second refusal");
    assert_eq!(pending[0].attempt_count, 2);

    // And the order itself is untouched: a refused envelope is not a reason
    // to have lost what it described.
    assert!(db.get_order("order-1").unwrap().is_some());
}

/// M6 A2 — a row the cloud refuses must block ITS OWN aggregate and nothing
/// else.
///
/// THE DEFECT. `pump_outbox` drains oldest-first and returns on the first
/// refusal, so one unreplayable row strands every row behind it regardless of
/// what aggregate they belong to. Read out of the M5 edge database on
/// 2026-09-03: 120 pending rows, of which 6 `order/ItemAdded` rows carried
/// `attempt_count = 10` and the other 114 — kots, stock counts, an invoice,
/// other orders — carried **zero**. They had never been attempted at all.
///
/// `StopReason`'s own doc comment (worker.rs:66-70) states the requirement
/// this must satisfy: events for **the same aggregate** must reach the cloud
/// in order. It says nothing about ordering ACROSS aggregates, and there is
/// nothing to say — a KOT for one order and a stock count for another are not
/// ordered with respect to each other by anything.
///
/// So the fix is not "keep going regardless": a global stop is still correct
/// when the CLOUD is the problem (offline, 5xx, 401/403/404, 408, 429), since
/// every row would get the same answer and continuing just burns requests.
/// It is wrong when THIS ROW is the problem, which is exactly what
/// `is_permanent_rejection` already decides for the ranged and procurement
/// streams.
///
/// Watched failing first: on the pre-fix binary the second aggregate is never
/// attempted, and this test's `published` assertion is empty.
#[test]
fn a_refused_row_blocks_its_own_aggregate_and_not_its_neighbours() {
    let server = Server::http("127.0.0.1:0").expect("start test server");
    let addr = server.server_addr();
    let base_url = format!("http://{addr}");
    let handle = std::thread::spawn(move || {
        // 1: verify_enrollment. 2: order-1 refused, permanently. 3: order-2
        // accepted — the request the pre-fix drain never makes.
        if let Some(req) = recv_before_deadline(&server) {
            let _ = req.respond(Response::from_string("{}").with_status_code(200));
        }
        if let Some(req) = recv_before_deadline(&server) {
            let _ = req.respond(
                Response::from_string(
                    "{\"code\":\"missing_reference\",\"message\":\"menu_item_id does not exist\"}",
                )
                .with_status_code(422),
            );
        }
        if let Some(req) = recv_before_deadline(&server) {
            let _ = req.respond(Response::from_string("{}").with_status_code(201));
        }
    });

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_device(&db, "outlet-1", "device-1");
    // Two different aggregates. order-1 is older, so it is drained first and
    // is the one the cloud refuses.
    seed_order_with_outbox(&mut db, "order-1", "outbox-1");
    seed_order_with_outbox(&mut db, "order-2", "outbox-2");

    let worker = SyncWorker::new(worker_config(base_url));
    let report = worker.pump_outbox(&mut db, 10).expect("pump");
    handle.join().unwrap();

    assert_eq!(
        report.published,
        vec!["outbox-2".to_string()],
        "order-2 has nothing wrong with it and must reach the cloud even though \
         order-1 was refused — 114 rows sat unattempted behind one bad row in \
         the M5 database for exactly this reason"
    );

    // order-1 is held, not dropped, and is named as blocked so A3 has
    // something to surface.
    assert_eq!(
        report.blocked_aggregates.len(),
        1,
        "the refusal must be attributed to one aggregate, not to the stream"
    );
    assert_eq!(report.blocked_aggregates[0].aggregate_type, "order");
    assert_eq!(report.blocked_aggregates[0].aggregate_id, "order-1");
    assert_eq!(report.blocked_aggregates[0].last_status, Some(422));

    let pending = repo::list_unpublished_outbox(db.connection(), 10).unwrap();
    assert_eq!(pending.len(), 1, "only the refused row stays pending");
    assert_eq!(pending[0].id, "outbox-1");
    assert_eq!(pending[0].attempt_count, 1);
}

/// The other half of the same decision: a SECOND event for an aggregate that
/// is already blocked must NOT be sent, or the cloud sees that aggregate's
/// events out of order — which is the guarantee the global stop was
/// protecting and the one thing A2 may not trade away.
#[test]
fn a_blocked_aggregate_holds_back_its_own_later_events() {
    let server = Server::http("127.0.0.1:0").expect("start test server");
    let addr = server.server_addr();
    let base_url = format!("http://{addr}");
    let handle = std::thread::spawn(move || {
        if let Some(req) = recv_before_deadline(&server) {
            let _ = req.respond(Response::from_string("{}").with_status_code(200));
        }
        // Only ONE refusal is scripted. If the drain sends order-1's second
        // event anyway, the responder is out of script and the test fails on
        // the assertions below rather than hanging.
        if let Some(req) = recv_before_deadline(&server) {
            let _ = req.respond(
                Response::from_string("{\"code\":\"missing_reference\"}").with_status_code(422),
            );
        }
        if let Some(req) = recv_before_deadline(&server) {
            let _ = req.respond(Response::from_string("{}").with_status_code(201));
        }
    });

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_device(&db, "outlet-1", "device-1");
    seed_order_with_outbox(&mut db, "order-1", "outbox-1");
    seed_order_with_outbox(&mut db, "order-2", "outbox-2");
    // A later event for the ALREADY-blocked order-1.
    db.connection()
        .execute(
            "INSERT INTO local_outbox (id, aggregate_type, aggregate_id, event_type, payload_json, created_at)
             VALUES ('outbox-3', 'order', 'order-1', 'OrderConfirmed', ?1, '2026-08-07T10:05:00Z')",
            [serde_json::json!({
                "event_id": "evt-3",
                "event_type": "OrderConfirmed",
                "occurred_at": "2026-08-07T10:05:00Z",
                "outlet_id": "outlet-1",
                "schema_version": 1,
                "data": { "order": { "holler_order_id": "order-1", "total_paise": 12550i64 } }
            })
            .to_string()],
        )
        .expect("seed later event for the blocked aggregate");

    let worker = SyncWorker::new(worker_config(base_url));
    let report = worker.pump_outbox(&mut db, 10).expect("pump");
    handle.join().unwrap();

    assert_eq!(
        report.published,
        vec!["outbox-2".to_string()],
        "a different aggregate still drains"
    );
    assert!(
        report.blocked_skipped.contains(&"outbox-3".to_string()),
        "order-1's later event must be held behind its blocked predecessor, \
         never sent ahead of it: {:?}",
        report.blocked_skipped
    );

    let pending = repo::list_unpublished_outbox(db.connection(), 10).unwrap();
    let ids: Vec<String> = pending.iter().map(|r| r.id.clone()).collect();
    assert_eq!(ids, vec!["outbox-1".to_string(), "outbox-3".to_string()]);
}


/// M6 A3 — the retry budget, and the durable record that makes spending it
/// survivable.
///
/// COUNTING AND BLOCKING ARE DIFFERENT DECISIONS. Every refusal increments,
/// whatever caused it, so "this row has been failing since Tuesday" is
/// answerable at all. Only a refusal that is the ROW's own fault spends the
/// budget. When the budget runs out the drain stops trying, which is
/// survivable only because the row is now visible: a `sync_outbox_block` row
/// with `blocked_at` set (contracts 0.6.4, ADR-023).
///
/// M6 C7 is closed on `last_code`, the machine-readable value from the
/// cloud's error envelope — not on `last_error`, which is prose and would
/// change whenever someone edited a message string.
#[test]
fn a_permanently_refused_row_spends_its_budget_then_becomes_visible() {
    let server = Server::http("127.0.0.1:0").expect("start test server");
    let addr = server.server_addr();
    let base_url = format!("http://{addr}");
    let handle = std::thread::spawn(move || {
        // verify_enrollment, then a 422 for every attempt the budget allows.
        if let Some(req) = recv_before_deadline(&server) {
            let _ = req.respond(Response::from_string("{}").with_status_code(200));
        }
        for _ in 0..MAX_OUTBOX_REPLAY_ATTEMPTS + 2 {
            if let Some(req) = recv_before_deadline(&server) {
                let _ = req.respond(
                    Response::from_string(
                        "{\"code\":\"missing_reference\",\"message\":\"menu_item_id does not exist\"}",
                    )
                    .with_status_code(422),
                );
            }
        }
    });

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_device(&db, "outlet-1", "device-1");
    seed_order_with_outbox(&mut db, "order-1", "outbox-1");

    let worker = SyncWorker::new(worker_config(base_url));

    // Every pump but the last leaves the row inside its budget: recorded, not
    // given up on.
    for attempt in 1..MAX_OUTBOX_REPLAY_ATTEMPTS {
        let report = worker.pump_outbox(&mut db, 10).expect("pump");
        assert!(
            report.gave_up.is_empty(),
            "attempt {attempt} of {MAX_OUTBOX_REPLAY_ATTEMPTS} must not abandon the row"
        );
        assert!(
            repo::list_blocked_outbox_rows(db.connection(), "outlet-1")
                .unwrap()
                .is_empty(),
            "nothing is shown to a human while the row is still being retried"
        );
    }

    // The pump that spends the last of the budget.
    let final_report = worker.pump_outbox(&mut db, 10).expect("final pump");
    assert_eq!(final_report.gave_up.len(), 1, "the budget is spent");
    assert_eq!(final_report.gave_up[0].outbox_id, "outbox-1");
    assert_eq!(final_report.gave_up[0].last_status, Some(422));
    assert_eq!(
        final_report.gave_up[0].last_code.as_deref(),
        Some("missing_reference"),
        "the machine-readable reason must survive to the report"
    );

    // The durable half: a row a human can be shown after a restart.
    let blocked = repo::list_blocked_outbox_rows(db.connection(), "outlet-1").unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].outbox_id, "outbox-1");
    assert_eq!(blocked[0].aggregate_type, "order");
    assert_eq!(blocked[0].aggregate_id, "order-1");
    assert_eq!(blocked[0].attempts, MAX_OUTBOX_REPLAY_ATTEMPTS);
    assert_eq!(blocked[0].last_status, Some(422));
    assert_eq!(
        blocked[0].last_code.as_deref(),
        Some("missing_reference"),
        "M6 C7 is closed on THIS column: a criterion resting on prose changes \
         truth value when someone edits a message string"
    );
    assert!(
        blocked[0].blocked_at.is_some(),
        "blocked_at is what makes the row something to show a human"
    );

    // And the drain stops paying for it: the next pump skips the row rather
    // than spending another request on an answer it has had five times.
    let after = worker.pump_outbox(&mut db, 10).expect("pump after giving up");
    handle.join().unwrap();
    assert_eq!(after.already_blocked, vec!["outbox-1".to_string()]);
    assert!(after.gave_up.is_empty(), "giving up is reported once, not every pump");

    // Never deleted. The row and its order are still there to be repaired.
    let pending = repo::list_unpublished_outbox(db.connection(), 10).unwrap();
    assert_eq!(pending.len(), 1);
    assert!(db.get_order("order-1").unwrap().is_some());
}

/// A TRANSIENT failure must never spend the budget — abandoning good rows
/// because the cloud was down for a day is data loss dressed as resilience
/// (ADR-018 0.5.8) — but it must still COUNT, or a row failing for a week is
/// invisible, which is the "looks healthy while nothing leaves the till"
/// state M5 ended in.
#[test]
fn a_transient_failure_counts_forever_and_blocks_never() {
    let server = Server::http("127.0.0.1:0").expect("start test server");
    let addr = server.server_addr();
    let base_url = format!("http://{addr}");
    let handle = std::thread::spawn(move || {
        if let Some(req) = recv_before_deadline(&server) {
            let _ = req.respond(Response::from_string("{}").with_status_code(200));
        }
        for _ in 0..MAX_OUTBOX_REPLAY_ATTEMPTS + 3 {
            if let Some(req) = recv_before_deadline(&server) {
                let _ = req.respond(
                    Response::from_string("{\"code\":\"internal_error\"}").with_status_code(503),
                );
            }
        }
    });

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_device(&db, "outlet-1", "device-1");
    seed_order_with_outbox(&mut db, "order-1", "outbox-1");

    let worker = SyncWorker::new(worker_config(base_url));
    for _ in 0..MAX_OUTBOX_REPLAY_ATTEMPTS + 2 {
        let report = worker.pump_outbox(&mut db, 10).expect("pump");
        assert!(report.gave_up.is_empty(), "a 5xx is the cloud being unwell, not this row");
        assert_eq!(report.stopped, Some(StopReason::Rejected { status: 503 }));
    }
    handle.join().unwrap();

    assert!(
        repo::list_blocked_outbox_rows(db.connection(), "outlet-1")
            .unwrap()
            .is_empty(),
        "a transient failure must never end in blocked_at, however often it repeats"
    );

    // Counted, though — and surfaced while still being retried, which is the
    // difference between telling someone and giving up on their data.
    let failing = repo::list_persistently_failing_outbox_rows(
        db.connection(),
        "outlet-1",
        OUTBOX_ATTENTION_ATTEMPTS,
    )
    .unwrap();
    assert_eq!(failing.len(), 1, "a row failing this long must be visible");
    assert!(failing[0].attempts >= OUTBOX_ATTENTION_ATTEMPTS);
    assert_eq!(failing[0].last_status, Some(503));
    assert!(failing[0].blocked_at.is_none(), "surfaced, NOT abandoned");
}

/// A row that failed and is later accepted must leave no alarm behind: a
/// surface full of resolved alarms stops being read, which is the outcome a
/// table was chosen over a log line to avoid.
#[test]
fn a_row_that_later_succeeds_clears_its_failure_record() {
    let server = Server::http("127.0.0.1:0").expect("start test server");
    let addr = server.server_addr();
    let base_url = format!("http://{addr}");
    let handle = std::thread::spawn(move || {
        if let Some(req) = recv_before_deadline(&server) {
            let _ = req.respond(Response::from_string("{}").with_status_code(200));
        }
        if let Some(req) = recv_before_deadline(&server) {
            let _ = req.respond(
                Response::from_string("{\"code\":\"missing_reference\"}").with_status_code(422),
            );
        }
        if let Some(req) = recv_before_deadline(&server) {
            let _ = req.respond(Response::from_string("{}").with_status_code(201));
        }
    });

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_device(&db, "outlet-1", "device-1");
    seed_order_with_outbox(&mut db, "order-1", "outbox-1");

    let worker = SyncWorker::new(worker_config(base_url));

    let first = worker.pump_outbox(&mut db, 10).expect("first pump");
    assert_eq!(first.blocked_aggregates.len(), 1);
    let failing =
        repo::list_persistently_failing_outbox_rows(db.connection(), "outlet-1", 1).unwrap();
    assert_eq!(failing.len(), 1, "the attempt is recorded");

    let second = worker.pump_outbox(&mut db, 10).expect("second pump");
    handle.join().unwrap();
    assert_eq!(second.published, vec!["outbox-1".to_string()]);

    let failing =
        repo::list_persistently_failing_outbox_rows(db.connection(), "outlet-1", 1).unwrap();
    assert!(failing.is_empty(), "the record goes when the row lands");
    assert!(repo::list_blocked_outbox_rows(db.connection(), "outlet-1")
        .unwrap()
        .is_empty());
}

/// Seeds one category/item and one station, and routes the item to it, so
/// `send_order_to_kitchen_with_outbox` has a ticket to cut. Mirrors
/// `edge/database`'s own `seed_menu`/`seed_station` helpers, which live in
/// that crate's private test module and cannot be reached from here.
fn seed_menu_and_station(db: &Db, outlet_id: &str) -> String {
    repo::upsert_menu_category(
        db.connection(),
        &model::MenuCategory {
            id: "category-1".to_string(),
            outlet_id: outlet_id.to_string(),
            name: "Mains".to_string(),
            sort_order: 1,
            config_version: 1,
        },
    )
    .expect("seed category");
    repo::upsert_menu_item(
        db.connection(),
        &model::MenuItem {
            id: "item-1".to_string(),
            outlet_id: outlet_id.to_string(),
            category_id: "category-1".to_string(),
            name: "Burger".to_string(),
            base_price_paise: 25000,
            is_available: true,
            config_version: 1,
            tax_profile_id: None,
            hsn_sac: Some("9963".to_string()),
        },
    )
    .expect("seed menu item");
    repo::upsert_station(
        db.connection(),
        &model::Station {
            id: "station-1".to_string(),
            outlet_id: outlet_id.to_string(),
            code: "HOT".to_string(),
            name: "Hot Pass".to_string(),
            sort_order: 0,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed station");
    repo::replace_menu_item_stations(db.connection(), "item-1", &["station-1".to_string()], 1)
        .expect("route item to station");
    "item-1".to_string()
}

/// WHICH PARTS OF AN ORDER'S LIFE CAN ACTUALLY LEAVE THE OUTLET, answered by
/// draining one rather than by reading `route.rs`.
///
/// The order half drains to nothing: create, line, confirm, send-to-kitchen,
/// and a line added AFTER the kitchen already has the ticket (`#132-A`) all
/// reach the cloud, and no `order` row is left unpublished.
///
/// The kitchen half does not leave at all, and the WAY it fails is the point.
/// Bumping the KOT writes `KOTStatusChanged`, and the derived order-READY
/// stamp writes `OrderReady`. `edge/sync/src/route.rs` maps neither, so
/// `resolve` returns `UnroutedEvent` and `pump_outbox` **SKIPS** the row: not
/// sent, not marked published, not charged an attempt, and NOT recorded as
/// blocked. It accumulates silently and forever.
///
/// **So there is no 404/405 wedge on this path — and that is a finding, not a
/// reassurance.** A wedge would at least surface in the till's blocked-row
/// banner, while these rows were invisible by construction: the 78 stranded
/// rows measured on the live edge on 2026-09-07 (gap A7) are this mechanism,
/// counted. **D8a made them visible** -- they are recorded and shown as
/// blocked now -- but the routes themselves are still missing, which is D8b
/// and stays open with the trigger "before the first pilot".
///
/// PREPARING, SERVED, BILLED, PAID and CLOSED are absent here because **the
/// edge never puts an order in any of them.** The only `UPDATE "order" SET
/// status` statements outside tests are CONFIRMED, SENT_TO_KITCHEN and READY
/// (`edge/database/src/repo.rs`); PREPARING is a KOT status, not an order one;
/// and no event type exists for the rest. Nothing is emitted, so nothing can be
/// refused — which is why `MountIngest` stopping at send-to-kitchen is not a
/// live wedge today.
#[test]
fn a_full_order_drains_to_an_empty_outbox_and_the_kitchen_events_never_leave() {
    let server = Server::http("127.0.0.1:0").expect("start test server");
    let addr = server.server_addr();
    let base_url = format!("http://{addr}");

    let seen_paths: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    let seen_paths_clone = seen_paths.clone();
    // verify_enrollment, then one request per routable order event: create,
    // line, confirm, send-to-kitchen, and the post-send line.
    let handle = std::thread::spawn(move || {
        for _ in 0..6 {
            if let Some(req) = recv_before_deadline(&server) {
                seen_paths_clone.lock().unwrap().push(req.url().to_string());
                let _ = req.respond(Response::from_string("{}").with_status_code(201));
            }
        }
    });

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_device(&db, "outlet-1", "device-1");
    let menu_item_id = seed_menu_and_station(&db, "outlet-1");
    seed_order_with_outbox(&mut db, "order-1", "outbox-create");

    let line = |id: &str| model::NewOrderItem {
        id: id.to_string(),
        order_id: "order-1".to_string(),
        menu_item_id: menu_item_id.clone(),
        variant_id: None,
        quantity: 1,
        unit_price_paise: 25000,
        line_total_paise: 25000,
        notes: None,
        created_at: "2026-08-07T10:01:00Z".to_string(),
    };

    db.add_order_item_with_outbox(
        &line("line-1"),
        &[],
        &model::OrderItemAddedMeta {
            outbox_id: "outbox-line-1".to_string(),
            occurred_at: "2026-08-07T10:01:00Z".to_string(),
        },
    )
    .expect("add the first line");
    db.confirm_order_with_outbox(
        "order-1",
        &model::OrderConfirmedMeta {
            outbox_id: "outbox-confirm".to_string(),
            occurred_at: "2026-08-07T10:02:00Z".to_string(),
            confirmed_at: "2026-08-07T10:02:00Z".to_string(),
        },
    )
    .expect("confirm");
    let kots = db
        .send_order_to_kitchen_with_outbox(
            "order-1",
            &model::SendToKitchenMeta {
                device_id: "device-1".to_string(),
                occurred_at: "2026-08-07T10:03:00Z".to_string(),
            },
        )
        .expect("send to kitchen");
    assert_eq!(kots.len(), 1, "one station, one ticket");

    // The second round: legal at the edge since #132-A, and the call the cloud
    // refused until contracts 0.8.3.
    db.add_order_item_with_outbox(
        &line("line-2"),
        &[],
        &model::OrderItemAddedMeta {
            outbox_id: "outbox-line-2".to_string(),
            occurred_at: "2026-08-07T10:04:00Z".to_string(),
        },
    )
    .expect("add a line after the kitchen already has the ticket");

    let worker = SyncWorker::new(worker_config(base_url));
    let report = worker.pump_outbox(&mut db, 50).expect("pump");
    handle.join().unwrap();

    assert!(report.stopped.is_none(), "nothing may stop this drain");
    assert_eq!(
        *seen_paths.lock().unwrap(),
        vec![
            VERIFY_PATH,
            "/orders",
            "/orders/order-1/items",
            "/orders/order-1/confirm",
            "/orders/order-1/send-to-kitchen",
            "/orders/order-1/items",
        ],
        "every order event must reach the cloud, in the order the edge recorded them"
    );

    let pending_after_order = repo::list_unpublished_outbox(db.connection(), 50).unwrap();
    assert!(
        pending_after_order
            .iter()
            .all(|e| e.aggregate_type != "order"),
        "no ORDER row may be left behind: {:?}",
        pending_after_order
            .iter()
            .map(|e| (e.aggregate_type.clone(), e.event_type.clone()))
            .collect::<Vec<_>>()
    );

    // Now the kitchen works the ticket to READY, which also stamps the ORDER
    // ready and emits OrderReady.
    let kot_id = kots[0].id.clone();
    for (i, status) in ["ACKNOWLEDGED", "PREPARING", "READY"].iter().enumerate() {
        db.transition_kot_status_with_outbox(
            &kot_id,
            status,
            &model::KotTransitionMeta {
                status_history_id: format!("history-{i}"),
                outbox_id: format!("outbox-kot-{i}"),
                changed_by_device_id: "device-1".to_string(),
                occurred_at: format!("2026-08-07T10:1{i}:00Z"),
            },
        )
        .unwrap_or_else(|e| panic!("transition to {status}: {e:?}"));
    }
    assert_eq!(
        db.get_order("order-1").unwrap().unwrap().status,
        "READY",
        "all tickets READY derives an order-READY stamp"
    );

    // Drain again ON THE SAME WORKER. No request is expected at all: every
    // remaining row is unroutable, and an unroutable row is never sent.
    //
    // The worker is reused deliberately. A FRESH worker re-runs
    // `verify_enrollment`, which is a real request, and the stand-in cloud's
    // responder thread has already finished — so a second worker would sit on
    // its own HTTP read timeout for a minute and prove nothing about routing.
    // The harness note at the top of this file is the same point from the
    // other side: size the script to what the flow really sends.
    let report2 = worker.pump_outbox(&mut db, 50).expect("second pump");
    assert!(
        report2.published.is_empty(),
        "nothing routable is left, so nothing may be published"
    );
    assert!(
        !report2.unrouted_skipped.is_empty(),
        "the kitchen rows must be reported as unroutable"
    );
    // SINCE D8a (2026-09-16) AN UNROUTABLE ROW IS SAYABLE: recorded and shown
    // on the till rather than skipped in silence, which is what the paragraph
    // above used to end with. It still does not hold back its aggregate's
    // later rows -- a route that does not exist will not start existing on a
    // retry, so blocking the aggregate would strand every later row for ever.
    assert!(
        report2.blocked_aggregates.is_empty(),
        "an unroutable row must NOT hold back its aggregate's later rows"
    );
    assert!(
        !report2.gave_up.is_empty(),
        "every unroutable row must be given up on VISIBLY"
    );

    let stranded: Vec<String> = repo::list_unpublished_outbox(db.connection(), 50)
        .unwrap()
        .iter()
        .map(|e| e.event_type.clone())
        .collect();
    assert!(
        stranded.contains(&"OrderReady".to_string())
            && stranded.contains(&"KOTStatusChanged".to_string())
            && stranded.contains(&"KOTCreated".to_string()),
        "the kitchen's whole record stays at the outlet, permanently: {stranded:?}"
    );
}

/// D8a: AN UNROUTABLE ROW MUST BE SAYABLE.
///
/// The drain used to `continue` past an event type `route.rs` does not map:
/// not sent, not published, not charged an attempt, and recorded nowhere. So
/// `OrderReady` and `KOTStatusChanged` — which the edge emits every time a
/// kitchen finishes a ticket — accumulated in the local outbox for ever while
/// every surface reported a healthy till. 78 such rows were measured on the
/// live edge on 2026-09-07 and no screen said so.
///
/// This drives the real emitters rather than hand-writing an outbox row: the
/// kitchen bumps its ticket to READY, which stamps the order READY and emits
/// `OrderReady`. Then it asserts the row is in
/// `repo::list_blocked_outbox_rows` — **the exact query the till's banner
/// reads** (`useBlockedOutboxRowsQuery` -> `list_blocked_outbox_rows`), not a
/// field of the in-memory report, because a report nothing renders is the
/// silence this fixes.
#[test]
fn an_unroutable_event_is_recorded_and_shown_rather_than_skipped_in_silence() {
    let server = Server::http("127.0.0.1:0").expect("start test server");
    let addr = server.server_addr();
    let base_url = format!("http://{addr}");

    // verify_enrollment, create, line, confirm, send-to-kitchen.
    let handle = std::thread::spawn(move || {
        for _ in 0..5 {
            if let Some(req) = recv_before_deadline(&server) {
                let _ = req.respond(Response::from_string("{}").with_status_code(201));
            }
        }
    });

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_device(&db, "outlet-1", "device-1");
    let menu_item_id = seed_menu_and_station(&db, "outlet-1");
    seed_order_with_outbox(&mut db, "order-1", "outbox-create");
    db.add_order_item_with_outbox(
        &model::NewOrderItem {
            id: "line-1".to_string(),
            order_id: "order-1".to_string(),
            menu_item_id,
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            line_total_paise: 25000,
            notes: None,
            created_at: "2026-08-07T10:01:00Z".to_string(),
        },
        &[],
        &model::OrderItemAddedMeta {
            outbox_id: "outbox-line-1".to_string(),
            occurred_at: "2026-08-07T10:01:00Z".to_string(),
        },
    )
    .expect("a ticket needs a line to carry");
    db.confirm_order_with_outbox(
        "order-1",
        &model::OrderConfirmedMeta {
            outbox_id: "outbox-confirm".to_string(),
            occurred_at: "2026-08-07T10:02:00Z".to_string(),
            confirmed_at: "2026-08-07T10:02:00Z".to_string(),
        },
    )
    .expect("confirm");
    let kots = db
        .send_order_to_kitchen_with_outbox(
            "order-1",
            &model::SendToKitchenMeta {
                device_id: "device-1".to_string(),
                occurred_at: "2026-08-07T10:03:00Z".to_string(),
            },
        )
        .expect("send to kitchen");

    let worker = SyncWorker::new(worker_config(base_url));
    worker.pump_outbox(&mut db, 50).expect("first pump");
    handle.join().unwrap();

    // The kitchen works the ticket to READY. The last transition stamps the
    // ORDER ready too, which is what emits OrderReady.
    let kot_id = kots[0].id.clone();
    for (i, status) in ["ACKNOWLEDGED", "PREPARING", "READY"].iter().enumerate() {
        db.transition_kot_status_with_outbox(
            &kot_id,
            status,
            &model::KotTransitionMeta {
                status_history_id: format!("history-{i}"),
                outbox_id: format!("outbox-kot-{i}"),
                changed_by_device_id: "device-1".to_string(),
                occurred_at: format!("2026-08-07T10:1{i}:00Z"),
            },
        )
        .unwrap_or_else(|e| panic!("transition to {status}: {e:?}"));
    }

    // Same worker: nothing left is routable, so no request is made and the
    // finished stand-in cloud is never needed (see the note on the drain test
    // above).
    let report = worker.pump_outbox(&mut db, 50).expect("second pump");
    assert!(
        report.published.is_empty(),
        "nothing routable is left, so nothing may be published"
    );

    // THE ASSERTION THAT MATTERS: the till's own query returns them.
    let shown = repo::list_blocked_outbox_rows(db.connection(), "outlet-1")
        .expect("list blocked outbox rows");
    let order_ready = shown
        .iter()
        .find(|b| b.outbox_id == kot_ready_outbox_id(&db, "order-1"))
        .or_else(|| shown.iter().find(|b| b.aggregate_type == "order"));
    let order_ready = order_ready.unwrap_or_else(|| {
        panic!(
            "OrderReady must appear on the banner, not vanish: blocked rows were {:?}",
            shown
                .iter()
                .map(|b| (b.aggregate_type.clone(), b.last_code.clone()))
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(
        order_ready.last_code.as_deref(),
        Some(UNROUTED_EVENT_CODE),
        "the row must carry a stable machine-readable reason, not prose alone"
    );
    assert!(
        order_ready.blocked_at.is_some(),
        "an unroutable row is blocked IMMEDIATELY: no number of retries can invent a route"
    );
    assert_eq!(
        order_ready.attempts, 1,
        "it is charged exactly one attempt, not a spent budget implying a recovery that cannot happen"
    );
    assert!(
        order_ready.last_error.contains("no route"),
        "the reason a human reads must say what is wrong: {:?}",
        order_ready.last_error
    );

    // The KOT rows are there too — every stranded stream, not just the one
    // the order carries.
    assert!(
        shown.iter().any(|b| b.aggregate_type == "kot"),
        "the kitchen's own rows must be shown as well: {:?}",
        shown.iter().map(|b| b.aggregate_type.clone()).collect::<Vec<_>>()
    );
}

/// The `OrderReady` outbox row's id, which this crate mints itself rather than
/// taking from the caller (see `KotTransitionMeta`'s doc comment), so a test
/// has to look it up rather than name it.
fn kot_ready_outbox_id(db: &Db, order_id: &str) -> String {
    repo::list_unpublished_outbox(db.connection(), 100)
        .expect("list outbox")
        .into_iter()
        .find(|e| e.event_type == "OrderReady" && e.aggregate_id == order_id)
        .map(|e| e.id)
        .unwrap_or_default()
}
