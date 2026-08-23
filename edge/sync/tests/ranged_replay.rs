//! Integration tests for ranged replay (contracts 0.5.8): a real in-memory
//! `holler_edge_database::Db` against a local `tiny_http` server standing in
//! for Holler Cloud, exactly as `worker_integration.rs` does for the outbox.
//!
//! The behaviour under test is the one that keeps a single bad row from
//! becoming an outage. The cloud half of that bargain — accept an entry
//! beyond the high-water mark and record the hole — is tested in
//! `backend/internal/inventory`. This is the edge half: a row the cloud will
//! never accept must not hold back every row behind it.
//!
//! The deadline bounds the RESPONDER side only. A script shorter than the
//! requests that actually arrive leaves the worker waiting out its own HTTP
//! read timeout on each unanswered one — bounded, but slow and silent about
//! why. Size a script from what the flow under test will really send.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use holler_edge_database::{model, repo, repo::ReplayStream, Db};
use holler_edge_sync::worker::{StopReason, SyncWorker, WorkerConfig};
use holler_edge_sync::MAX_ENTRY_REPLAY_ATTEMPTS;
use tiny_http::{Response, Server};

fn seed_outlet(db: &Db, outlet_id: &str) {
    repo::upsert_outlet(
        db.connection(),
        &model::Outlet {
            id: outlet_id.to_string(),
            brand_id: "brand-1".to_string(),
            name: "Test Outlet".to_string(),
            timezone: "Asia/Kolkata".to_string(),
            config_version: 1,
            created_at: "2026-08-23T00:00:00Z".to_string(),
            updated_at: "2026-08-23T00:00:00Z".to_string(),
        },
    )
    .expect("seed outlet");
}

/// Writes ledger rows directly. The writer (`confirm_order`'s deduction path)
/// is covered by `edge/database`'s own suite; what is under test here is the
/// pump that carries them, so the rows are placed rather than earned.
fn seed_ledger_entries(db: &Db, outlet_id: &str, seqs: &[i64]) {
    for seq in seqs {
        db.connection()
            .execute(
                "INSERT INTO stock_ledger_entry
                   (id, outlet_id, entry_seq, inventory_item_id, inventory_item_name,
                    dimension, entry_type, origin, quantity_applied_micro,
                    occurred_at, business_date)
                 VALUES (?1, ?2, ?3, 'item-1', 'Chicken', 'MASS', 'CONSUMPTION',
                         'MANUAL', -1000000, '2026-08-23T10:00:00Z', '2026-08-23')",
                rusqlite::params![format!("entry-{seq}"), outlet_id, seq],
            )
            .expect("seed ledger entry");
    }
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

/// How long the stand-in cloud waits for a request that should already be on
/// its way.
const RECV_DEADLINE: Duration = Duration::from_secs(5);

/// A cloud that answers each request with the next status in a script, and
/// records the paths it was asked for. The first request of a worker's life
/// is always `verify_enrollment` (ADR-017).
///
/// **Every receive carries a deadline.** A script one entry longer than the
/// requests that actually arrive would otherwise block this thread forever,
/// and `handle.join()` with it: the test would hang rather than fail, telling
/// nobody what went wrong. That is exactly the shape of the defect this file
/// tests for — waiting forever on something that never arrives — one layer
/// up, in the code doing the checking. On timeout the responder simply stops
/// and the test fails on its own assertions.
fn scripted_cloud(statuses: Vec<u16>) -> (String, Arc<Mutex<Vec<String>>>, std::thread::JoinHandle<()>) {
    let server = Server::http("127.0.0.1:0").expect("start test server");
    let base_url = format!("http://{}", server.server_addr());
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    let seen_clone = seen.clone();

    let handle = std::thread::spawn(move || {
        for status in statuses {
            match server.recv_timeout(RECV_DEADLINE) {
                Ok(Some(req)) => {
                    seen_clone.lock().unwrap().push(req.url().to_string());
                    let _ = req.respond(Response::from_string("{}").with_status_code(status));
                }
                // Timed out, or the socket died: the expected request is not
                // coming. Stop, so join() returns and the assertions speak.
                Ok(None) | Err(_) => break,
            }
        }
    });

    (base_url, seen, handle)
}

fn blocked_rows(db: &Db) -> Vec<model::SyncReplayBlock> {
    repo::list_blocked_replays(db.connection(), "outlet-1").expect("list blocked")
}

fn recorded_failures(db: &Db) -> i64 {
    db.connection()
        .query_row("SELECT COUNT(*) FROM sync_replay_block", [], |r| r.get(0))
        .expect("count failures")
}

#[test]
fn the_stream_replays_in_mark_order_and_the_cursor_advances_on_ack() {
    // verify_enrollment, then three entries.
    let (base_url, seen, handle) = scripted_cloud(vec![200, 201, 201, 201]);

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet(&db, "outlet-1");
    seed_ledger_entries(&db, "outlet-1", &[1, 2, 3]);

    let worker = SyncWorker::new(worker_config(base_url));
    let report = worker.pump_ranged_streams(&mut db, 10).expect("pump");
    handle.join().unwrap();

    assert_eq!(report.ledger_acked, vec![1, 2, 3], "sent in mark order");
    assert!(report.stopped.is_none());
    assert_eq!(
        repo::get_replay_cursor(db.connection(), "outlet-1", ReplayStream::Ledger).unwrap(),
        3,
        "the cursor is the high-water mark of what the cloud acked"
    );

    let paths = seen.lock().unwrap().clone();
    assert_eq!(paths.len(), 4);
    assert!(paths[1..].iter().all(|p| p == "/inventory/ledger-entries"));

    // Nothing is deleted on ack, and a second pass sends nothing.
    let again = worker.pump_ranged_streams(&mut db, 10).expect("second pump");
    assert!(
        again.ledger_acked.is_empty(),
        "an acked range must not be resent"
    );
}

/// THE TEST THIS FILE EXISTS FOR. A row the cloud permanently rejects must
/// not wedge the stream behind it — the mirror image of a contiguity check
/// that rejects instead of recording.
///
/// Entry 2 is refused with 422 forever. The first four passes retry it and
/// hold position (order matters while an entry may still land). The fifth
/// spends its budget: entry 2 is recorded as blocked, the cursor moves past
/// it, and entry 3 — which had been stuck behind it — replays.
#[test]
fn a_permanently_rejected_entry_is_bounded_and_the_rest_of_the_stream_continues() {
    let (base_url, _seen, handle) = scripted_cloud(vec![
        200, // verify_enrollment
        201, // entry 1 accepted
        422, 422, 422, 422, 422, // entry 2, five times
        201, // entry 3, once entry 2 stops holding the line
    ]);

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet(&db, "outlet-1");
    seed_ledger_entries(&db, "outlet-1", &[1, 2, 3]);

    let worker = SyncWorker::new(worker_config(base_url));

    for pass in 1..MAX_ENTRY_REPLAY_ATTEMPTS {
        let report = worker.pump_ranged_streams(&mut db, 10).expect("pump");
        assert_eq!(
            report.stopped,
            Some(StopReason::Rejected { status: 422 }),
            "pass {pass}: still inside the budget, so the stream holds position"
        );
        assert!(report.blocked.is_empty(), "pass {pass}: not yet abandoned");
        assert_eq!(
            repo::get_replay_cursor(db.connection(), "outlet-1", ReplayStream::Ledger).unwrap(),
            1,
            "pass {pass}: the cursor must not pass an entry still being retried"
        );
    }

    let report = worker.pump_ranged_streams(&mut db, 10).expect("final pump");
    handle.join().unwrap();

    assert_eq!(report.blocked.len(), 1, "entry 2 is given up on");
    assert_eq!(report.blocked[0].entry_seq, 2);
    assert_eq!(report.blocked[0].stream, "LEDGER");
    assert_eq!(
        report.ledger_acked,
        vec![3],
        "entry 3 must replay once entry 2 stops holding the line — this is \
         the difference between a bounded failure and an outage"
    );

    // Never silent: the abandoned entry is a durable row a human can be shown.
    let blocked = blocked_rows(&db);
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].entry_seq, 2);
    assert_eq!(blocked[0].attempts, MAX_ENTRY_REPLAY_ATTEMPTS);
    assert_eq!(blocked[0].last_status, Some(422));
    assert!(
        blocked[0].blocked_at.is_some(),
        "blocked_at is what makes the row visible on the POS surface"
    );
}

/// A transient failure must NOT spend an entry's budget. Otherwise a long
/// cloud outage would abandon a run of perfectly good entries — data loss
/// dressed as resilience. Retrying forever is safe here precisely because
/// nothing at the outlet depends on the uplink (ADR-013).
#[test]
fn a_transient_rejection_never_spends_the_per_entry_budget() {
    let (base_url, _seen, handle) = scripted_cloud(vec![200, 503, 503, 503, 503, 503, 503]);

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet(&db, "outlet-1");
    seed_ledger_entries(&db, "outlet-1", &[1, 2]);

    let worker = SyncWorker::new(worker_config(base_url));
    for _ in 0..MAX_ENTRY_REPLAY_ATTEMPTS + 1 {
        let report = worker.pump_ranged_streams(&mut db, 10).expect("pump");
        assert_eq!(report.stopped, Some(StopReason::Rejected { status: 503 }));
        assert!(report.blocked.is_empty());
    }
    handle.join().unwrap();

    assert_eq!(
        recorded_failures(&db),
        0,
        "a 5xx is the cloud being unwell, not this row being wrong"
    );
    assert_eq!(
        repo::get_replay_cursor(db.connection(), "outlet-1", ReplayStream::Ledger).unwrap(),
        0,
        "nothing was acked, so nothing may be skipped"
    );
}

/// An entry that failed earlier and is later accepted must leave no block
/// behind. A surface full of resolved alarms stops being read, which is the
/// outcome a table was chosen over a log line to avoid.
#[test]
fn an_entry_that_later_succeeds_clears_its_failure_record() {
    let (base_url, _seen, handle) = scripted_cloud(vec![200, 400, 201]);

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet(&db, "outlet-1");
    seed_ledger_entries(&db, "outlet-1", &[1]);

    let worker = SyncWorker::new(worker_config(base_url));

    let first = worker.pump_ranged_streams(&mut db, 10).expect("pump");
    assert_eq!(first.stopped, Some(StopReason::Rejected { status: 400 }));
    assert_eq!(recorded_failures(&db), 1, "the attempt is recorded");

    let second = worker.pump_ranged_streams(&mut db, 10).expect("pump");
    handle.join().unwrap();

    assert_eq!(second.ledger_acked, vec![1]);
    assert_eq!(
        recorded_failures(&db),
        0,
        "an accepted entry leaves no alarm behind"
    );
    assert!(blocked_rows(&db).is_empty());
}

/// The two streams are independent: separate counters, separate cursors. A
/// ledger stream that stops must not stop the gap stream — a gap row is the
/// signal explaining an unaccounted sale, and holding it back because a
/// movement was refused would suppress the one record that explains the
/// missing movements.
#[test]
fn the_two_streams_carry_independent_cursors() {
    let (base_url, _seen, handle) = scripted_cloud(vec![200, 500, 201]);

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet(&db, "outlet-1");
    seed_ledger_entries(&db, "outlet-1", &[1]);
    db.connection()
        .execute(
            "INSERT INTO stock_deduction_gap
               (id, outlet_id, entry_seq, order_id, order_item_id, menu_item_id,
                menu_item_variant_id, menu_item_name, quantity, reason,
                occurred_at, business_date)
             VALUES ('gap-1','outlet-1',1,'ord-1','oi-1','mi-1',NULL,
                     'Butter Chicken',1,'NO_RECIPE','2026-08-23T10:05:00Z','2026-08-23')",
            [],
        )
        .expect("seed gap");

    let worker = SyncWorker::new(worker_config(base_url));
    let report = worker.pump_ranged_streams(&mut db, 10).expect("pump");
    handle.join().unwrap();

    assert!(report.ledger_acked.is_empty(), "the ledger stream stopped");
    assert_eq!(
        report.gap_acked,
        vec![1],
        "the gap stream must replay regardless"
    );
    assert_eq!(
        repo::get_replay_cursor(db.connection(), "outlet-1", ReplayStream::Ledger).unwrap(),
        0
    );
    assert_eq!(
        repo::get_replay_cursor(db.connection(), "outlet-1", ReplayStream::DeductionGap).unwrap(),
        1,
        "one mark cannot mean two positions"
    );
}

/// ADR-017 hole 1, pinned for the ranged pump specifically.
///
/// This pump originally omitted the enrollment check the outbox pump had
/// performed since M1 — a second implementation of an existing pattern that
/// silently dropped one of its checks, and nothing failed, because a dropped
/// check is invisible until the day it was the check that mattered. The
/// structural fix was to make `SyncWorker::post_verified` the only path to
/// the cloud; this test is what fails if a future flow finds another one.
///
/// Two assertions, and the second is the one with teeth: nothing is sent,
/// AND no entry is charged for it. A credential this node cannot present is
/// not the row's fault, so spending a per-entry retry budget on it would
/// abandon good entries over a problem that has nothing to do with them.
#[test]
fn a_mis_enrolled_node_sends_nothing_and_charges_no_entry() {
    // The node is never verified, so EVERY pass re-pings /sync/config: one
    // request per pass, all refused. The script is sized from the loop below
    // rather than guessed — a script SHORTER than the requests that actually
    // arrive leaves the client waiting out its own read timeout on each
    // unanswered one, which is slow and explains nothing. The responder's
    // deadline does not help there: it bounds this side of the socket only.
    let passes = (MAX_ENTRY_REPLAY_ATTEMPTS + 1) as usize;
    let (base_url, seen, handle) = scripted_cloud(vec![404; passes]);

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet(&db, "outlet-1");
    seed_ledger_entries(&db, "outlet-1", &[1, 2]);

    let worker = SyncWorker::new(worker_config(base_url));
    for _ in 0..passes {
        let report = worker.pump_ranged_streams(&mut db, 10).expect("pump");
        assert_eq!(report.stopped, Some(StopReason::Rejected { status: 404 }));
        assert!(report.ledger_acked.is_empty());
        assert!(report.blocked.is_empty(), "no entry is at fault here");
    }
    handle.join().unwrap();

    let paths = seen.lock().unwrap().clone();
    assert!(
        paths.iter().all(|p| p.starts_with("/sync/config")),
        "a mis-enrolled node must never reach the ingest route: {paths:?}"
    );
    assert_eq!(
        recorded_failures(&db),
        0,
        "a credential this node cannot present is not the entry's fault"
    );
    assert_eq!(
        repo::get_replay_cursor(db.connection(), "outlet-1", ReplayStream::Ledger).unwrap(),
        0,
        "nothing was acked, so nothing may be skipped"
    );
}
