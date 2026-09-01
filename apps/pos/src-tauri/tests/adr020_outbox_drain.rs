//! ADR-020 falsification: the shutdown outbox drain.
//!
//! Two claims in that ADR are load-bearing and neither is self-evident, so each
//! is falsified here rather than asserted:
//!
//! 1. **The drain must run BEFORE `Db::shutdown_in_place` seals the database.**
//!    Moved after the seal it cannot publish anything at all.
//! 2. **The drain is BOUNDED.** An outlet closing with no reachable cloud is the
//!    normal case, so shutdown returns on the deadline instead of hanging.
//!
//! Both run against a real file-backed encrypted database and a real HTTP
//! server, because both claims are about I/O and neither would be exercised by
//! an in-memory handle.

use std::io::Cursor;
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use holler_edge_database::crypto::EncryptionKey;
use holler_edge_database::Db;
use holler_edge_sync::{SyncWorker, WorkerConfig};
use holler_pos_lib::state::AppState;

const OUTLET_ID: &str = "0191a000-0000-7000-8000-00000000000a";
const DEVICE_ID: &str = "0191a000-0000-7000-8000-00000000000b";

fn test_key() -> EncryptionKey {
    EncryptionKey::new([7u8; 32])
}

/// A database with an outlet row (`sync_state` has a foreign key to it) and
/// `pending` outbox rows the router can resolve without any aggregate row
/// present: `("order", "SentToKitchen")` takes its payload straight from the
/// event JSON.
fn seeded_db(dir: &std::path::Path, pending: usize) -> Db {
    let db = Db::open(&dir.join("edge.db.enc"), &dir.join("edge.db"), test_key())
        .expect("opening a fresh encrypted database");

    db.connection()
        .execute(
            "INSERT INTO outlet
               (id, brand_id, name, timezone, config_version, created_at, updated_at)
             VALUES (?1, '0191a000-0000-7000-8000-000000000002', 'Test Outlet',
                     'Asia/Kolkata', 1, '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z')",
            [OUTLET_ID],
        )
        .expect("seeding the outlet row");

    db.connection()
        .execute(
            "INSERT INTO device (id, outlet_id, kind, name, created_at)
             VALUES (?1, ?2, 'POS', 'Test Till', '2026-09-01T00:00:00Z')",
            [DEVICE_ID, OUTLET_ID],
        )
        .expect("seeding the device row");

    for i in 0..pending {
        insert_pending(&db, i);
    }
    db
}

fn insert_pending(db: &Db, i: usize) {
    let id = format!("outbox-{i}");
    let aggregate_id = format!("order-{i}");

    // The router looks the aggregate up locally, so a synthetic outbox row
    // needs a real order behind it.
    db.connection()
        .execute(
            "INSERT INTO \"order\"
               (id, outlet_id, device_id, order_type, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'TAKEAWAY', 'DRAFT',
                     '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z')",
            [aggregate_id.as_str(), OUTLET_ID, DEVICE_ID],
        )
        .expect("seeding the order the outbox row refers to");

    db.connection()
        .execute(
            "INSERT INTO local_outbox
               (id, aggregate_type, aggregate_id, event_type, payload_json, created_at)
             VALUES (?1, 'order', ?2, 'SentToKitchen', ?3, '2026-09-01T00:00:00Z')",
            [id.as_str(), aggregate_id.as_str(), r#"{"data":{}}"#],
        )
        .expect("seeding an outbox row");
}

fn worker_for(base_url: String) -> SyncWorker {
    SyncWorker::new(WorkerConfig {
        tenant_id: "0191a000-0000-7000-8000-000000000001".to_string(),
        outlet_id: OUTLET_ID.to_string(),
        device_id: DEVICE_ID.to_string(),
        base_url,
        device_token: "cred-test.not-a-real-secret".to_string(),
    })
}

/// A cloud that acknowledges everything: 200 for the enrollment ping on
/// `/sync/config`, 201 for every ingest POST. Returns its base URL and a
/// counter of ingest calls.
fn fake_cloud() -> (String, Arc<AtomicUsize>) {
    let server = tiny_http::Server::http("127.0.0.1:0").expect("binding a fake cloud");
    let base = format!("http://{}", server.server_addr());
    let ingests = Arc::new(AtomicUsize::new(0));
    let counter = ingests.clone();

    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let is_config = req.url().starts_with("/sync/config");
            if !is_config {
                counter.fetch_add(1, Ordering::SeqCst);
            }
            let status = if is_config { 200 } else { 201 };
            let body = b"{}";
            let _ = req.respond(tiny_http::Response::new(
                tiny_http::StatusCode(status),
                vec![],
                Cursor::new(body.to_vec()),
                Some(body.len()),
                None,
            ));
        }
    });

    (base, ingests)
}

/// An address nothing is listening on. Bind a port, read it, drop the listener:
/// a connect there is refused rather than left hanging, which is the honest
/// shape of "the cloud is unreachable from this outlet".
fn dead_address() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserving a port");
    let addr: SocketAddr = listener.local_addr().expect("reading the reserved port");
    drop(listener);
    format!("http://{addr}")
}

// ---------------------------------------------------------------------------
// Claim 1: the drain must run BEFORE the seal.
// ---------------------------------------------------------------------------

#[test]
fn drain_before_the_seal_publishes_and_after_the_seal_publishes_nothing() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let (base_url, ingests) = fake_cloud();

    let state = AppState::new_with_sync(
        seeded_db(dir.path(), 3),
        OUTLET_ID.to_string(),
        DEVICE_ID.to_string(),
        worker_for(base_url),
    );

    // BEFORE the seal: the drain reaches the cloud and publishes.
    let published = state.drain_outbox("test-before-seal", Duration::from_secs(20));
    assert_eq!(
        published, 3,
        "a drain before the seal must publish every pending row"
    );
    assert_eq!(
        ingests.load(Ordering::SeqCst),
        3,
        "three ingest calls should have reached the cloud"
    );

    // Leave rows pending across the seal, so "published nothing" cannot be
    // confused with "there was nothing to publish".
    {
        let db = state.db.lock().expect("the db lock");
        for i in 3..6 {
            insert_pending(&db, i);
        }
    }

    let before = ingests.load(Ordering::SeqCst);
    state.seal_for_tests();

    // AFTER the seal: the drain cannot publish. It does not merely return zero
    // -- `Db::connection` panics with "edge database handle used after
    // shutdown", so a drain moved below the seal would take the exit handler
    // down with it. Either outcome is the same where it counts: NOTHING
    // REPLAYS. Ordering is load-bearing, and getting it wrong is invisible in
    // review.
    let after_seal = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.drain_outbox("test-after-seal", Duration::from_secs(20))
    }));

    match after_seal {
        Ok(n) => assert_eq!(n, 0, "a drain after the seal must publish nothing"),
        Err(_) => { /* panicked on the closed handle: also publishes nothing */ }
    }
    assert_eq!(
        ingests.load(Ordering::SeqCst),
        before,
        "a drain after the seal must not reach the cloud at all, yet three rows were pending"
    );
}

// ---------------------------------------------------------------------------
// Claim 2: the drain is bounded.
// ---------------------------------------------------------------------------

#[test]
fn shutdown_drain_with_no_reachable_cloud_returns_instead_of_hanging() {
    let dir = tempfile::tempdir().expect("a temp dir");

    let state = AppState::new_with_sync(
        seeded_db(dir.path(), 5),
        OUTLET_ID.to_string(),
        DEVICE_ID.to_string(),
        worker_for(dead_address()),
    );

    let budget = Duration::from_secs(5);
    let started = Instant::now();
    let published = state.drain_outbox("test-offline", budget);
    let elapsed = started.elapsed();

    assert_eq!(
        published, 0,
        "nothing can be published with no cloud to publish to"
    );

    // The real assertion is that it RETURNS. A shutdown that blocks on a
    // network that is not coming strands a cashier at a till that will not
    // close -- worse than the day replaying tomorrow morning.
    assert!(
        elapsed < budget + Duration::from_secs(30),
        "the offline drain must give up near its budget, took {elapsed:?}"
    );

    // And the rows survive: giving up is not discarding.
    let db = state.db.lock().expect("the db lock");
    let pending: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM local_outbox WHERE published_at IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("counting pending rows");
    assert_eq!(
        pending, 5,
        "an abandoned drain must leave every row in the outbox"
    );
}
