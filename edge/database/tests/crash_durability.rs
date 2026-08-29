//! M4 acceptance criterion 2, proved against a real process death.
//!
//! > Kill the POS between confirm and deduction → order and ledger agree on
//! > reopen. **Judged against the crash, not the API.**
//!
//! The test that previously claimed this criterion called a function inside
//! the same process that dropped a database handle. That is a durability
//! guarantee proved by a method call — the kind of evidence the milestone's
//! acceptance rules exclude. It has been deleted rather than left standing
//! beside this one, because a fake proof next to a real one is read by
//! whoever finds it first.
//!
//! # Shape
//!
//! Spawn the `crashpoint` binary, which calls the same
//! `Db::confirm_order_with_outbox` the POS calls, with `HOLLER_CRASH_POINT`
//! set so it aborts at a deterministic point INSIDE the confirm+deduct
//! transaction. Wait for the abnormal exit. Reopen the same sealed file from
//! this process and assert.
//!
//! No sleeps, no guessed kill moment: an external kill at an approximate time
//! is a race, and a flaky durability test gets disabled, which is worse than
//! having none.
//!
//! # The residual gap, stated rather than papered over
//!
//! This proves the WAL and the transaction boundary survive **process**
//! death. It does not prove the release binary makes no non-transactional
//! write outside the gated path, and it does not exercise OS page-cache loss.
//! Machine death is a different failure mode and lives in the parked bare-4GB
//! validation (ADR-013) as hard power-cut recovery. Neither substitutes for
//! the other.
#![cfg(feature = "crash-points")]

use std::path::{Path, PathBuf};
use std::process::Command;

use holler_edge_database::crypto::EncryptionKey;
use holler_edge_database::model::{NewOrder, NewOrderItem, NewOutboxEntry};
use holler_edge_database::Db;

const KEY_HEX: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";
const ORDER_ID: &str = "0191b000-0000-7000-8000-0000000000c1";
const ORDER_ITEM_ID: &str = "0191b000-0000-7000-8000-0000000000c2";

fn key() -> EncryptionKey {
    let mut bytes = [0u8; 32];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&KEY_HEX[i * 2..i * 2 + 2], 16).expect("hex key");
    }
    EncryptionKey::new(bytes)
}

fn open(dir: &Path) -> Db {
    Db::open(&dir.join("edge.db.enc"), &dir.join("edge.db"), key()).expect("open sealed db")
}

/// Seeds with the REAL development seed — the same 32-item menu with recipes
/// an outlet would have — rather than a fixture shaped to suit the test.
/// Criterion 1 makes that a requirement for the offline sale; there is no
/// reason for the crash test to be seeded any differently.
fn devseed(dir: &Path) {
    let out = Command::new(env!("CARGO_BIN_EXE_devseed"))
        .env("HOLLER_EDGE_DATA_DIR", dir)
        .env("HOLLER_DB_KEY_HEX", KEY_HEX)
        // Never verified here (that needs HOLLER_SEED_PASSWORD, which is what
        // devseed's own offline-login check uses); it only has to be stored.
        .env(
            "HOLLER_SEED_PASSWORD_HASH",
            "$argon2id$v=19$m=65536,t=3,p=4$c2FsdHNhbHRzYWx0$0000000000000000000000000000000000000000000",
        )
        .output()
        .expect("run devseed");
    assert!(
        out.status.success(),
        "devseed failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// One sellable unit that actually has a recipe, taken from the seed rather
/// than assumed: if the seed changes, this test follows it instead of
/// silently testing an item with nothing to deduct.
fn a_variant_with_a_recipe(db: &Db) -> (String, String) {
    db.connection()
        .query_row(
            "SELECT v.menu_item_id, v.id
             FROM recipe r
             JOIN menu_item_variant v ON v.id = r.menu_item_variant_id
             ORDER BY v.id LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the seed must contain at least one variant with a recipe")
}

fn create_draft_order(db: &mut Db, outlet_id: &str, menu_item_id: &str, variant_id: &str) {
    let order = NewOrder {
        id: ORDER_ID.to_string(),
        outlet_id: outlet_id.to_string(),
        device_id: device_id(db),
        order_type: "DINE_IN".to_string(),
        status: "DRAFT".to_string(),
        table_id: None,
        subtotal_paise: 30_000,
        discount_paise: 0,
        taxes_paise: 0,
        total_paise: 30_000,
        source: "POS".to_string(),
        external_order_id: None,
        payment_status: "UNPAID".to_string(),
        payment_source: None,
        confirmed_at: None,
        source_payload_json: None,
        schema_version: 1,
        created_at: "2026-08-23T10:00:00Z".to_string(),
        updated_at: "2026-08-23T10:00:00Z".to_string(),
    };
    let item = NewOrderItem {
        id: ORDER_ITEM_ID.to_string(),
        order_id: ORDER_ID.to_string(),
        menu_item_id: menu_item_id.to_string(),
        variant_id: Some(variant_id.to_string()),
        quantity: 2,
        unit_price_paise: 15_000,
        line_total_paise: 30_000,
        notes: None,
        created_at: "2026-08-23T10:00:00Z".to_string(),
    };
    let outbox = NewOutboxEntry {
        id: "outbox-crash-order".to_string(),
        aggregate_type: "order".to_string(),
        aggregate_id: ORDER_ID.to_string(),
        event_type: "OrderCreated".to_string(),
        payload_json: "{}".to_string(),
        created_at: "2026-08-23T10:00:00Z".to_string(),
    };
    db.create_order_with_outbox(&order, &[item], &outbox)
        .expect("create the draft order the crash will interrupt");
}

fn outlet_id(db: &Db) -> String {
    db.connection()
        .query_row("SELECT id FROM outlet LIMIT 1", [], |r| r.get(0))
        .expect("seeded outlet")
}

fn device_id(db: &Db) -> String {
    db.connection()
        .query_row("SELECT id FROM device LIMIT 1", [], |r| r.get(0))
        .expect("seeded device")
}

fn order_status(db: &Db) -> String {
    db.connection()
        .query_row(
            "SELECT status FROM \"order\" WHERE id = ?1",
            [ORDER_ID],
            |r| r.get(0),
        )
        .expect("the order row itself must survive — it was committed before the crash")
}

fn ledger_rows_for_order(db: &Db) -> i64 {
    db.connection()
        .query_row(
            "SELECT COUNT(*) FROM stock_ledger_entry WHERE source_order_id = ?1",
            [ORDER_ID],
            |r| r.get(0),
        )
        .expect("count ledger rows")
}

/// Runs `crashpoint` against the sealed database, optionally arming a crash
/// point. Returns the exit status.
fn run_crashpoint(dir: &Path, crash_point: Option<&str>) -> std::process::ExitStatus {
    run_crashpoint_target(dir, ORDER_ID, crash_point)
}

fn run_crashpoint_target(
    dir: &Path,
    target: &str,
    crash_point: Option<&str>,
) -> std::process::ExitStatus {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_crashpoint"));
    cmd.arg(dir).arg(target).env("HOLLER_DB_KEY_HEX", KEY_HEX);
    if let Some(point) = crash_point {
        cmd.env("HOLLER_CRASH_POINT", point);
    }
    cmd.status().expect("run crashpoint")
}

fn seeded_dir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    devseed(&path);
    let mut db = open(&path);
    let outlet = outlet_id(&db);
    let (menu_item_id, variant_id) = a_variant_with_a_recipe(&db);
    create_draft_order(&mut db, &outlet, &menu_item_id, &variant_id);
    db.close().expect("seal before the crash run");
    (dir, path)
}

/// THE CRITERION. The process dies inside the confirm+deduct transaction,
/// after the order is stamped CONFIRMED and before the deduction runs.
/// Because both writes share one transaction, "agree" means NEITHER
/// survives: the order is still DRAFT and the ledger is empty for it.
///
/// A partial outcome — CONFIRMED with no deduction, or deducted stock for an
/// order that is not confirmed — is the failure this criterion exists to
/// exclude, and either would fail here.
#[test]
fn order_and_ledger_agree_after_the_process_dies_mid_confirm() {
    let (_guard, dir) = seeded_dir();

    let status = run_crashpoint(&dir, Some("after_confirm_before_deduct"));

    // Neither a clean exit nor the binary's own error path: the process was
    // terminated. Checking this rather than a specific code keeps the test
    // honest across platforms, where an abort surfaces differently (SIGABRT
    // on Unix, a fastfail status on Windows).
    assert!(
        !status.success(),
        "the process must have died inside the transaction, not completed"
    );
    assert_ne!(
        status.code(),
        Some(2),
        "exit 2 is crashpoint's own error path — the abort point did not fire, \
         so nothing about durability was tested"
    );

    // A genuinely independent reopen: a different process wrote this file,
    // and recovery runs from whatever it left behind.
    let db = open(&dir);
    assert_eq!(
        order_status(&db),
        "DRAFT",
        "the confirm was never committed, so the order must still be DRAFT"
    );
    assert_eq!(
        ledger_rows_for_order(&db),
        0,
        "no deduction may survive a confirm that did not commit"
    );
}

/// The positive control, and it is not optional: without it the test above
/// passes just as well against a build that deducts nothing at all, or an
/// order that was never deductible in the first place. Same seed, same order,
/// same binary, no crash point — the confirm must commit AND produce ledger
/// rows.
#[test]
fn without_the_crash_point_the_same_confirm_commits_and_deducts() {
    let (_guard, dir) = seeded_dir();

    let status = run_crashpoint(&dir, None);
    assert!(
        status.success(),
        "an unarmed run must complete and seal: {status:?}"
    );

    let db = open(&dir);
    assert_eq!(order_status(&db), "CONFIRMED");
    assert!(
        ledger_rows_for_order(&db) > 0,
        "the seeded item has a recipe, so a committed confirm must deduct — \
         without this the crash assertion above would hold vacuously"
    );
}

// ===========================================================================
// M5 acceptance criterion 2, the same shape one milestone on.
//
//   > Kill the POS between the GRN write and the ledger post → GRN and
//   > ledger agree on reopen. **Judged against the crash, not the API.**
//
// The receipt, its lines, its gaps, the PURCHASE ledger entries and the
// grn_sequence advance that minted the number all ride in ONE transaction,
// so "agree" means NEITHER survives. A partial outcome — a receipt with no
// stock behind it, or stock with no receipt explaining it — is exactly what
// this excludes, and a delivery that was recorded but never reached the
// ledger is the worse of the two: it reads as received while the shelf
// figure says it never arrived.
//
// The seed carries no supplier and no purchase order, which makes this the
// walk-in delivery case — the one an outlet actually performs with the
// uplink down.
// ===========================================================================

const GRN_ID: &str = "0191b000-0000-7000-8000-0000000000d1";

fn grn_rows(db: &Db) -> i64 {
    db.connection()
        .query_row(
            "SELECT COUNT(*) FROM goods_receipt_note WHERE id = ?1",
            [GRN_ID],
            |r| r.get(0),
        )
        .expect("count receipts")
}

fn grn_line_rows(db: &Db) -> i64 {
    db.connection()
        .query_row(
            "SELECT COUNT(*) FROM grn_line WHERE grn_id = ?1",
            [GRN_ID],
            |r| r.get(0),
        )
        .expect("count receipt lines")
}

fn ledger_rows_for_grn(db: &Db) -> i64 {
    db.connection()
        .query_row(
            "SELECT COUNT(*) FROM stock_ledger_entry WHERE source_grn_id = ?1",
            [GRN_ID],
            |r| r.get(0),
        )
        .expect("count ledger rows")
}

/// The GRN number the counter would have issued. Read back to prove the
/// counter itself rolled back with the receipt: a consumed-but-unused number
/// would make the next receipt jump, and a gap in the series must only ever
/// mean "a receipt was rolled back", never "two receipts share a number".
fn grn_sequence_next_value(db: &Db) -> i64 {
    db.connection()
        .query_row(
            "SELECT COALESCE(MAX(next_value), 0) FROM grn_sequence",
            [],
            |r| r.get(0),
        )
        .expect("read grn_sequence")
}

/// THE CRITERION. The process dies inside the receipt transaction, after the
/// `goods_receipt_note` and its gaps are written and before a single
/// `PURCHASE` entry is posted.
#[test]
fn grn_and_ledger_agree_after_the_process_dies_mid_receipt() {
    let (_guard, dir) = seeded_dir();

    let status = run_crashpoint_target(&dir, "--grn", Some("after_grn_before_ledger"));

    assert!(
        !status.success(),
        "the process must have died inside the transaction, not completed"
    );
    assert_ne!(
        status.code(),
        Some(2),
        "exit 2 is crashpoint's own error path — the abort point did not fire, \
         so nothing about durability was tested"
    );

    // A genuinely independent reopen: a different process wrote this file.
    let db = open(&dir);
    assert_eq!(
        grn_rows(&db),
        0,
        "the receipt was written before the abort but never committed, so it \
         must not survive"
    );
    assert_eq!(grn_line_rows(&db), 0);
    assert_eq!(
        ledger_rows_for_grn(&db),
        0,
        "no stock may exist for a receipt that did not commit"
    );
    assert_eq!(
        grn_sequence_next_value(&db),
        0,
        "the counter advance rides in the same transaction: an uncommitted \
         receipt must not consume a GRN number"
    );
}

/// The positive control, and it is not optional: without it the test above
/// passes just as well against a build that receives nothing at all. Same
/// seed, same binary, no crash point — the receipt must commit AND post
/// stock, and the counter must have moved exactly once.
#[test]
fn without_the_crash_point_the_same_receipt_commits_and_posts_stock() {
    let (_guard, dir) = seeded_dir();

    let status = run_crashpoint_target(&dir, "--grn", None);
    assert!(
        status.success(),
        "an unarmed run must complete and seal: {status:?}"
    );

    let db = open(&dir);
    assert_eq!(grn_rows(&db), 1);
    assert_eq!(grn_line_rows(&db), 1);
    assert_eq!(
        ledger_rows_for_grn(&db),
        1,
        "a committed receipt posts its PURCHASE entry — without this the \
         crash assertion above would hold vacuously"
    );
    assert_eq!(
        grn_sequence_next_value(&db),
        1,
        "1-based, and advanced exactly once by the one receipt that committed"
    );
}
