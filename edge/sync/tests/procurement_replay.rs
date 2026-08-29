//! Procurement replay (Milestone 5, track T3, ADR-019 + contracts 0.6.1):
//! a real in-memory `holler_edge_database::Db` against a local `tiny_http`
//! standing in for Holler Cloud, exactly as `ranged_replay.rs` does for the
//! stock streams.
//!
//! **The receipts are EARNED, not placed.** Every outbox row under test is
//! written by `Db::record_goods_receipt_with_outbox` — the same call the POS
//! makes. A test that hand-inserted `local_outbox` rows would keep passing
//! after a rename in `edge/database`'s emitter, and replay would be broken in
//! production with a green suite behind it. That is the M4 criterion-1 shape
//! ("a deduction test proves deduction only for the path its caller takes").
//!
//! The behaviour these exist for is the non-wedging one: **a receipt the cloud
//! will never accept must not strand the receipts behind it.**

use std::sync::{Arc, Mutex};
use std::time::Duration;

use holler_edge_database::model::{
    NewGoodsReceiptNote, NewGrnLine, Outlet, ProcurementOutboxMeta,
};
use holler_edge_database::{repo, Db};
use holler_edge_sync::worker::{StopReason, SyncWorker, WorkerConfig};
use holler_edge_sync::MAX_PROCUREMENT_REPLAY_ATTEMPTS;
use tiny_http::{Response, Server};

const OUTLET: &str = "outlet-1";
const USER: &str = "user-1";
const RICE: &str = "item-rice";
const SUPPLIER: &str = "sup-1";
/// One kilogram in micro-units of the canonical gram.
const KG: i64 = 1_000_000_000;
const RECV_DEADLINE: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------- fixtures --

fn seed_outlet(db: &Db) {
    repo::upsert_outlet(
        db.connection(),
        &Outlet {
            id: OUTLET.to_string(),
            brand_id: "brand-1".to_string(),
            name: "Test Outlet".to_string(),
            timezone: "Asia/Kolkata".to_string(),
            config_version: 1,
            created_at: "2026-08-29T00:00:00Z".to_string(),
            updated_at: "2026-08-29T00:00:00Z".to_string(),
        },
    )
    .expect("seed outlet");
}

/// Config rows only. Every operational row asserted about below is written by
/// the code under test.
fn configured() -> Db {
    let db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet(&db);
    db.connection()
        .execute(
            "INSERT INTO app_user
                (id, tenant_id, outlet_id, email, full_name, password_hash, pin_hash,
                 is_active, permissions_json, config_version, updated_at)
             VALUES (?1, 'tenant-1', ?2, 'r@example.test', 'Receiver', 'not-a-hash', NULL,
                     1, '[]', 1, '2026-08-29T00:00:00Z')",
            rusqlite::params![USER, OUTLET],
        )
        .expect("seed app_user");
    db.connection()
        .execute(
            "INSERT INTO inventory_item
                (id, outlet_id, sku, name, dimension, is_active, yield_factor_ppm, config_version)
             VALUES (?1, ?2, 'SKU-RICE', 'Rice', 'MASS', 1, 1000000, 1)",
            rusqlite::params![RICE, OUTLET],
        )
        .expect("seed inventory_item");
    db.connection()
        .execute(
            "INSERT INTO supplier
                (id, outlet_id, code, name, payment_terms_days, is_active, config_version)
             VALUES (?1, ?2, 'SUP-1', 'Test Supplier', 0, 1, 1)",
            rusqlite::params![SUPPLIER, OUTLET],
        )
        .expect("seed supplier");
    db.connection()
        .execute(
            "INSERT INTO supplier_item
                (id, supplier_id, inventory_item_id, purchase_unit, pack_size_micro,
                 quantity_dimension, last_price_paise, is_preferred)
             VALUES ('si-1', ?1, ?2, 'SACK', ?3, 'MASS', NULL, 1)",
            rusqlite::params![SUPPLIER, RICE, 50 * KG],
        )
        .expect("seed supplier_item");
    db
}

/// Records one receipt through the PUBLIC write path, with its outbox rows.
/// No PO is referenced, so each receipt also earns a `NO_PURCHASE_ORDER` gap —
/// which is the point: the gap rides beside the receipt it explains.
fn record_receipt(db: &mut Db, grn_id: &str, occurred_at: &str) {
    db.record_goods_receipt_with_outbox(
        NewGoodsReceiptNote {
            id: grn_id.to_string(),
            outlet_id: OUTLET.to_string(),
            purchase_order_id: None,
            supplier_id: Some(SUPPLIER.to_string()),
            delivery_note_ref: Some("DN-11821".to_string()),
            received_at: occurred_at.to_string(),
            received_by_user_id: USER.to_string(),
            notes: None,
            lines: vec![NewGrnLine {
                inventory_item_id: RICE.to_string(),
                entered_purchase_unit: "SACK".to_string(),
                entered_quantity_micro: 2_000_000,
                // The author's own declaration, never read off the item.
                quantity_dimension: "MASS".to_string(),
                purchase_price_paise: 250_000,
                batch_code: Some("BATCH-A".to_string()),
                expiry_date: Some("2026-12-31".to_string()),
                purchase_order_line_id: None,
            }],
        },
        &ProcurementOutboxMeta {
            outbox_id: format!("outbox-{grn_id}"),
            occurred_at: occurred_at.to_string(),
        },
    )
    .expect("a receipt is never refused");
}

fn unpublished_ids(db: &Db) -> Vec<String> {
    let conn = db.connection();
    let mut stmt = conn
        .prepare("SELECT id FROM local_outbox WHERE published_at IS NULL ORDER BY id")
        .expect("prepare");
    stmt.query_map([], |r| r.get::<_, String>(0))
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect")
}

fn attempt_count(db: &Db, outbox_id: &str) -> i64 {
    db.connection()
        .query_row(
            "SELECT attempt_count FROM local_outbox WHERE id = ?1",
            [outbox_id],
            |r| r.get(0),
        )
        .expect("read attempt_count")
}

fn worker_config(base_url: String) -> WorkerConfig {
    WorkerConfig {
        tenant_id: "tenant-1".to_string(),
        outlet_id: OUTLET.to_string(),
        device_id: "device-1".to_string(),
        base_url,
        device_token: "cred-1.test-secret".to_string(),
    }
}

// ------------------------------------------------------------ the fake cloud --

/// A cloud that answers `n` requests, refusing every envelope whose
/// `record_id` is in `refuse` with `status` and accepting everything else.
///
/// Keyed on the envelope's own `record_id` rather than a positional script:
/// the property under test is that ONE row is refused while the others get
/// through, and a positional script would silently pass if the pump sent them
/// in a different order than expected. Records `(path, record_id)` per call.
///
/// **Every receive carries a deadline.** A count one higher than the requests
/// that actually arrive would otherwise block this thread forever and
/// `join()` with it: the test would hang rather than fail — the same shape of
/// defect as the one under test, one layer up.
#[allow(clippy::type_complexity)]
fn cloud(
    calls: usize,
    refuse: Vec<String>,
    status: u16,
) -> (
    String,
    Arc<Mutex<Vec<(String, String)>>>,
    std::thread::JoinHandle<()>,
) {
    let server = Server::http("127.0.0.1:0").expect("start test server");
    let base_url = format!("http://{}", server.server_addr());
    let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(vec![]));
    let seen_clone = seen.clone();

    let handle = std::thread::spawn(move || {
        for _ in 0..calls {
            match server.recv_timeout(RECV_DEADLINE) {
                Ok(Some(mut req)) => {
                    let path = req.url().to_string();
                    let mut body = String::new();
                    // A GET (verify_enrollment) has no body; that is not a
                    // failure, it is the first call of every worker's life.
                    let _ = std::io::Read::read_to_string(&mut req.as_reader(), &mut body);
                    let record_id = serde_json::from_str::<serde_json::Value>(&body)
                        .ok()
                        .and_then(|v| {
                            v.get("record_id")
                                .and_then(|r| r.as_str())
                                .map(str::to_string)
                        })
                        .unwrap_or_default();
                    let code = if refuse.contains(&record_id) { status } else { 201 };
                    seen_clone.lock().unwrap().push((path, record_id));
                    let _ = req.respond(Response::from_string("{}").with_status_code(code));
                }
                Ok(None) | Err(_) => break,
            }
        }
    });

    (base_url, seen, handle)
}

// ------------------------------------------------------------------- tests --

/// The happy path, and the route contract with it: a receipt and the gap that
/// explains it travel to the SAME route (ADR-019 §9), because a gap arriving
/// by another path could not be joined to the receipt.
#[test]
fn a_receipt_and_its_gap_replay_to_the_same_contracted_route() {
    let mut db = configured();
    record_receipt(&mut db, "grn-1", "2026-08-29T05:30:00Z");

    let pending = unpublished_ids(&db);
    assert_eq!(
        pending.len(),
        2,
        "the write path must have earned a receipt row AND a gap row: {pending:?}"
    );

    let (base_url, seen, handle) = cloud(3, vec![], 422);
    let worker = SyncWorker::new(worker_config(base_url));
    let report = worker.pump_procurement(&mut db, 50).expect("pump");
    handle.join().unwrap();

    assert_eq!(report.published.len(), 2, "{report:?}");
    assert!(report.stopped.is_none());
    assert!(report.refused_locally.is_empty(), "{report:?}");
    assert!(unpublished_ids(&db).is_empty(), "nothing left pending");

    let calls = seen.lock().unwrap().clone();
    assert_eq!(calls.len(), 3, "verify_enrollment, then two envelopes");
    assert!(
        calls[1..]
            .iter()
            .all(|(path, _)| path == "/procurement/goods-receipts"),
        "receipt and gap share one route: {calls:?}"
    );

    // Marked published, never deleted (docs/spec/sync.md), and a second pass
    // sends nothing.
    let again = worker.pump_procurement(&mut db, 50).expect("second pump");
    assert!(again.published.is_empty());
    let rows: i64 = db
        .connection()
        .query_row("SELECT COUNT(*) FROM local_outbox", [], |r| r.get(0))
        .expect("count outbox");
    assert_eq!(rows, 2, "an acked row is marked, never removed");
}

/// **THE TEST THIS FILE EXISTS FOR.** A receipt the cloud permanently refuses
/// must not hold back the receipts behind it — the head-of-line outage 0.5.8
/// named at the edge end, on the transport ADR-019 chose for procurement.
///
/// `grn-1` is refused with 422 forever. In the SAME pass, `grn-2` and `grn-3`
/// still replay: the budget is spent per entry, not per stream. After
/// `MAX_PROCUREMENT_REPLAY_ATTEMPTS` passes the row is abandoned and becomes
/// visible to a human, and it is never sent again.
#[test]
fn a_permanently_rejected_receipt_never_strands_the_receipts_behind_it() {
    let mut db = configured();
    record_receipt(&mut db, "grn-1", "2026-08-29T05:30:00Z");
    record_receipt(&mut db, "grn-2", "2026-08-29T06:30:00Z");
    record_receipt(&mut db, "grn-3", "2026-08-29T07:30:00Z");

    let pending = unpublished_ids(&db);
    assert_eq!(pending.len(), 6, "three receipts, each with its gap");

    // Pass 1: verify + 6 envelopes. Passes 2..N: only the refused row is left.
    let (base_url, _seen, handle) = cloud(1 + 6 + 10, vec!["grn-1".to_string()], 422);
    let worker = SyncWorker::new(worker_config(base_url));

    let first = worker.pump_procurement(&mut db, 50).expect("pump");
    assert_eq!(
        first.published.len(),
        5,
        "EVERY row except the refused one gets through IN THE SAME PASS — this \
         is the difference between a bounded failure and an outage: {first:?}"
    );
    assert!(
        first.stopped.is_none(),
        "a permanent rejection stops nothing: {first:?}"
    );
    assert!(first.blocked.is_empty(), "still inside the budget");
    assert_eq!(attempt_count(&db, "outbox-grn-1"), 1);
    assert_eq!(unpublished_ids(&db), vec!["outbox-grn-1".to_string()]);

    // The remaining passes retry only the refused row, and only until its
    // budget is spent.
    for pass in 2..MAX_PROCUREMENT_REPLAY_ATTEMPTS {
        let report = worker.pump_procurement(&mut db, 50).expect("pump");
        assert!(report.blocked.is_empty(), "pass {pass}: not yet abandoned");
        assert_eq!(attempt_count(&db, "outbox-grn-1"), pass);
    }

    let final_pass = worker.pump_procurement(&mut db, 50).expect("final pump");
    assert_eq!(final_pass.blocked.len(), 1, "{final_pass:?}");
    assert_eq!(final_pass.blocked[0].outbox_id, "outbox-grn-1");
    assert_eq!(final_pass.blocked[0].aggregate_type, "goods_receipt_note");
    assert_eq!(final_pass.blocked[0].status, Some(422));
    assert_eq!(
        attempt_count(&db, "outbox-grn-1"),
        MAX_PROCUREMENT_REPLAY_ATTEMPTS
    );

    // Never silent: the abandoned receipt is a durable row the POS shows a
    // human, and it is never sent again.
    let over_budget = db
        .list_over_budget_procurement_replays()
        .expect("over-budget read");
    assert_eq!(over_budget.len(), 1);
    assert_eq!(over_budget[0].id, "outbox-grn-1");
    assert!(
        over_budget[0].published_at.is_none(),
        "abandoned is not published: a fixed cloud can still land it"
    );

    let after = worker.pump_procurement(&mut db, 50).expect("post-budget pump");
    handle.join().unwrap();
    assert_eq!(after.over_budget, vec!["outbox-grn-1".to_string()]);
    assert!(after.published.is_empty());
    assert_eq!(
        attempt_count(&db, "outbox-grn-1"),
        MAX_PROCUREMENT_REPLAY_ATTEMPTS,
        "an abandoned row is not retried, so its count stops moving"
    );
}

/// A transient condition spends NO budget and stops the pass where it stands.
/// Otherwise a long cloud outage would abandon a run of perfectly good
/// receipts — data loss dressed as resilience. Retrying forever is safe here
/// precisely because nothing at the outlet depends on the uplink (ADR-013).
#[test]
fn a_transient_failure_spends_no_budget() {
    let mut db = configured();
    record_receipt(&mut db, "grn-1", "2026-08-29T05:30:00Z");

    let (base_url, _seen, handle) = cloud(
        1 + (MAX_PROCUREMENT_REPLAY_ATTEMPTS as usize + 2),
        vec!["grn-1".to_string()],
        503,
    );
    let worker = SyncWorker::new(worker_config(base_url));

    for _ in 0..MAX_PROCUREMENT_REPLAY_ATTEMPTS + 1 {
        let report = worker.pump_procurement(&mut db, 50).expect("pump");
        assert_eq!(report.stopped, Some(StopReason::Rejected { status: 503 }));
        assert!(report.blocked.is_empty());
    }
    handle.join().unwrap();

    assert_eq!(
        attempt_count(&db, "outbox-grn-1"),
        0,
        "a 5xx is the cloud being unwell, not this row being wrong"
    );
    assert!(
        db.list_over_budget_procurement_replays()
            .expect("read")
            .is_empty(),
        "no row is ever abandoned for an outage"
    );
}

/// One row is drained by exactly ONE pump. The general outbox pump keeps its
/// stop-at-the-first-rejection ordering guarantee for `order`, and must not
/// also carry procurement rows — two pumps over one row would send it twice,
/// and neither would see the other's classification of a rejection.
#[test]
fn the_general_outbox_pump_leaves_procurement_rows_to_the_procurement_pump() {
    let mut db = configured();
    record_receipt(&mut db, "grn-1", "2026-08-29T05:30:00Z");
    assert_eq!(unpublished_ids(&db).len(), 2, "fixtures must have inserted");

    // No requests are expected at all: the general pump should find nothing
    // to send and never reach the network.
    let (base_url, seen, handle) = cloud(0, vec![], 422);
    let worker = SyncWorker::new(worker_config(base_url));
    let report = worker.pump_outbox(&mut db, 50).expect("outbox pump");
    handle.join().unwrap();

    assert!(report.published.is_empty(), "{report:?}");
    assert!(report.authority_violations.is_empty(), "{report:?}");
    assert!(report.unrouted_skipped.is_empty(), "{report:?}");
    assert!(seen.lock().unwrap().is_empty(), "nothing was sent");
    assert_eq!(
        unpublished_ids(&db).len(),
        2,
        "the rows are still there for their own pump"
    );
}
