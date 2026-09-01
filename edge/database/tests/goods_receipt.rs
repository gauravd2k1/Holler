//! Goods receipt at the edge, exercised through the PUBLIC `Db` API — the
//! same surface `apps/pos/src-tauri` calls (Milestone 5, track T2, ADR-019).
//!
//! ============================================================================
//! WHAT THESE TESTS ARE FOR
//! ============================================================================
//!
//! **A GRN NEVER BLOCKS ON A PO**, and each unmatched condition is proved to
//! record a gap AND leave the receipt standing. A test that only proved "a
//! gap row exists" would pass against an implementation that recorded the gap
//! and then rolled the receipt back — which is the exact outage ADR-019 §1
//! exists to prevent. So every case below asserts BOTH halves: the gap is
//! there, and so are the receipt, its line and its stock.
//!
//! **Fixtures are asserted present before anything is asserted about them.**
//! A rejected INSERT leaves zero rows and every later assertion then passes
//! trivially; `assert_receipt_landed` exists so a green run cannot mean
//! "nothing was written".
//!
//! Config rows (`inventory_item`, `supplier`, `purchase_order`) are seeded.
//! Every OPERATIONAL row these tests assert about — the receipt, its lines,
//! its gaps, its ledger entries — is written by the code under test.

use holler_edge_database::model::{
    NewGoodsReceiptNote, NewGrnLine, NewPurchaseReturn, NewPurchaseReturnLine,
    NewStockTransferLine, NewStockTransferOut, ProcurementOutboxMeta,
};
use holler_edge_database::Db;

mod support;
use support::procurement::{
    micro, seed_inventory_item, seed_outlet, seed_purchase_order_with_line, seed_supplier,
    seed_supplier_item, seed_user, IDENTITY_PPM, OUTLET, PO_ID, PO_LINE_ID, RICE, SUPPLIER, USER,
};

/// One kilogram in micro-units of the canonical gram.
const KG: i64 = 1_000_000_000;

fn line(item: &str, unit: &str, qty_units: i64, price_paise: i64, dimension: &str) -> NewGrnLine {
    NewGrnLine {
        inventory_item_id: item.to_string(),
        entered_purchase_unit: unit.to_string(),
        entered_quantity_micro: micro(qty_units),
        // THE AUTHOR'S OWN DECLARATION. Every test that wants a mismatch
        // states one explicitly; nothing here reads it off the item.
        quantity_dimension: dimension.to_string(),
        purchase_price_paise: price_paise,
        batch_code: Some("BATCH-A".to_string()),
        expiry_date: Some("2026-12-31".to_string()),
        purchase_order_line_id: None,
    }
}

fn receipt(
    id: &str,
    po: Option<&str>,
    supplier: Option<&str>,
    lines: Vec<NewGrnLine>,
) -> NewGoodsReceiptNote {
    NewGoodsReceiptNote {
        id: id.to_string(),
        outlet_id: OUTLET.to_string(),
        purchase_order_id: po.map(str::to_string),
        supplier_id: supplier.map(str::to_string),
        delivery_note_ref: Some("DN-11821".to_string()),
        received_at: "2026-08-29T05:30:00Z".to_string(),
        received_by_user_id: USER.to_string(),
        notes: None,
        lines,
    }
}

/// A fully configured outlet: one item, one supplier that sells it in 50 kg
/// sacks, one user. No receipts and no ledger rows — those are the code's job.
fn configured() -> Db {
    let db = Db::open_in_memory_for_tests().expect("open db");
    let conn = db.connection();
    seed_outlet(conn, OUTLET);
    seed_user(conn, USER, OUTLET);
    seed_inventory_item(conn, RICE, OUTLET, "Rice", "MASS", IDENTITY_PPM);
    seed_supplier(conn, SUPPLIER, OUTLET);
    seed_supplier_item(conn, SUPPLIER, RICE, "SACK", 50 * KG, "MASS");
    db
}

/// **The fixture assertion.** Called before any gap assertion, so a green
/// result cannot mean the receipt was refused and the gap checks ran over an
/// empty table.
fn assert_receipt_landed(db: &Db, grn_id: &str, expected_lines: usize) {
    let rows: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM goods_receipt_note WHERE id = ?1",
            [grn_id],
            |r| r.get(0),
        )
        .expect("count receipts");
    assert_eq!(rows, 1, "THE RECEIPT MUST STAND: a gap never refuses it");

    let lines: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM grn_line WHERE grn_id = ?1",
            [grn_id],
            |r| r.get(0),
        )
        .expect("count lines");
    assert_eq!(lines, expected_lines as i64);

    let ledger: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM stock_ledger_entry WHERE source_grn_id = ?1",
            [grn_id],
            |r| r.get(0),
        )
        .expect("count ledger entries");
    assert_eq!(
        ledger, expected_lines as i64,
        "every recorded line posts a PURCHASE entry: receipt and ledger agree"
    );
}

fn reasons(db: &Db) -> Vec<String> {
    db.list_grn_gaps(OUTLET)
        .expect("read gaps")
        .into_iter()
        .map(|g| g.reason)
        .collect()
}

// ---------------------------------------------------------------------------
// The happy path, and the stock it produces.
// ---------------------------------------------------------------------------

/// M5 acceptance criterion 1's shape: the receipt is recorded, the PURCHASE
/// entries land at the CONVERTED base quantity with `unit_cost_paise` set,
/// and stock rises by the received amount. (Criterion 1 itself is observed
/// against the shipping binaries with the network down; this is the
/// crate-level behaviour underneath it.)
#[test]
fn a_receipt_posts_purchase_entries_at_the_converted_quantity_and_raises_stock() {
    let mut db = configured();
    seed_purchase_order_with_line(
        db.connection(),
        OUTLET,
        SUPPLIER,
        USER,
        RICE,
        "SACK",
        micro(10),
    );

    let before = db.get_current_stock(OUTLET, RICE).expect("stock before");
    assert_eq!(before, 0, "nothing is seeded into the ledger");

    let stored = db
        .record_goods_receipt(receipt(
            "grn-1",
            Some(PO_ID),
            Some(SUPPLIER),
            vec![line(RICE, "SACK", 2, 200_000, "MASS")],
        ))
        .expect("record receipt");

    assert_receipt_landed(&db, "grn-1", 1);
    assert_eq!(stored.grn_number, "GRN/20260829/0001");
    assert_eq!(stored.business_date, "2026-08-29");

    let grn_line = &stored.lines[0];
    assert_eq!(grn_line.entered_quantity_micro, micro(2), "what was typed");
    assert_eq!(grn_line.entered_purchase_unit, "SACK");
    assert_eq!(grn_line.pack_size_micro_applied, 50 * KG);
    assert_eq!(grn_line.base_quantity_micro, 100 * KG, "2 sacks of 50 kg");
    assert_eq!(grn_line.line_total_paise, 400_000);
    assert_eq!(grn_line.unit_cost_paise, 4, "Rs 4000 over 100_000 g");

    let after = db.get_current_stock(OUTLET, RICE).expect("stock after");
    assert_eq!(
        after - before,
        100 * KG,
        "stock rises by the received amount"
    );

    // unit_cost_paise is SET, not left NULL as it was on every row for a
    // whole milestone.
    let cost: Option<i64> = db
        .connection()
        .query_row(
            "SELECT unit_cost_paise FROM stock_ledger_entry WHERE source_grn_id = 'grn-1'",
            [],
            |r| r.get(0),
        )
        .expect("read cost");
    assert_eq!(cost, Some(4));

    assert!(
        reasons(&db).is_empty(),
        "a fully matched receipt records no gap: {:?}",
        reasons(&db)
    );
}

/// The `entryIntentEcho` (M5 acceptance criterion 4) shows what WILL be
/// recorded, in base units, and is produced by the same resolution the write
/// uses — so it cannot disagree with the row that follows it.
#[test]
fn the_entry_intent_echo_matches_what_the_receipt_actually_records() {
    let mut db = configured();
    let candidate = line(RICE, "SACK", 3, 200_000, "MASS");

    let echo = db
        .grn_entry_intent_echo(Some(SUPPLIER), &candidate)
        .expect("echo");
    assert_eq!(echo.base_quantity_micro, 150 * KG);
    assert_eq!(echo.inventory_item_name, "Rice");
    assert_eq!(echo.item_dimension, "MASS");

    let stored = db
        .record_goods_receipt(receipt("grn-echo", None, Some(SUPPLIER), vec![candidate]))
        .expect("record");
    let written = &stored.lines[0];

    assert_eq!(echo.base_quantity_micro, written.base_quantity_micro);
    assert_eq!(
        echo.pack_size_micro_applied,
        written.pack_size_micro_applied
    );
    assert_eq!(echo.unit_cost_paise, written.unit_cost_paise);
    assert_eq!(echo.line_total_paise, written.line_total_paise);
}

// ---------------------------------------------------------------------------
// A GRN NEVER BLOCKS ON A PO — one test per unmatched condition.
// ---------------------------------------------------------------------------

#[test]
fn a_receipt_with_no_purchase_order_at_all_is_accepted_and_gapped() {
    let mut db = configured();
    db.record_goods_receipt(receipt(
        "grn-nopo",
        None,
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 1, 200_000, "MASS")],
    ))
    .expect("a walk-in delivery must be recordable");

    assert_receipt_landed(&db, "grn-nopo", 1);
    assert!(reasons(&db).contains(&"NO_PURCHASE_ORDER".to_string()));
}

/// M5 acceptance criterion 3: received against a PO that never synced here.
/// The receipt completes, the gap is recorded, and the gap is READABLE — the
/// detail is prose naming the order, because a person acts on it.
#[test]
fn a_receipt_against_an_unsynced_purchase_order_is_accepted_and_the_gap_is_readable() {
    let mut db = configured();
    db.record_goods_receipt(receipt(
        "grn-ghost",
        Some("po-that-never-synced"),
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 1, 200_000, "MASS")],
    ))
    .expect("a receipt against an unknown order must complete");

    assert_receipt_landed(&db, "grn-ghost", 1);

    let gaps = db.list_grn_gaps(OUTLET).expect("read gaps");
    let gap = gaps
        .iter()
        .find(|g| g.reason == "PURCHASE_ORDER_NOT_FOUND")
        .expect("the unmatched order must be reported");
    let detail = gap.detail.as_deref().unwrap_or_default();
    assert!(
        detail.contains("po-that-never-synced"),
        "the id the operator gave must survive in prose even though the \
         column had to be NULL: {detail:?}"
    );
    assert!(
        detail.len() > 40,
        "the gap is read by a human, not parsed: {detail:?}"
    );

    // The dangling link is NULL on the row (it carries a real FK), and that
    // is the only thing lost — the fact itself is in the gap above.
    let stored_po: Option<String> = db
        .connection()
        .query_row(
            "SELECT purchase_order_id FROM goods_receipt_note WHERE id = 'grn-ghost'",
            [],
            |r| r.get(0),
        )
        .expect("read po link");
    assert_eq!(stored_po, None);
}

#[test]
fn an_item_the_purchase_order_does_not_list_is_received_and_gapped() {
    let mut db = configured();
    seed_inventory_item(
        db.connection(),
        "item-oil",
        OUTLET,
        "Oil",
        "VOLUME",
        IDENTITY_PPM,
    );
    seed_purchase_order_with_line(
        db.connection(),
        OUTLET,
        SUPPLIER,
        USER,
        RICE,
        "SACK",
        micro(10),
    );

    // Oil was added to the delivery after the order was sent.
    db.record_goods_receipt(receipt(
        "grn-extra",
        Some(PO_ID),
        Some(SUPPLIER),
        vec![line("item-oil", "l", 20, 8_000, "VOLUME")],
    ))
    .expect("an unlisted item must still be received");

    assert_receipt_landed(&db, "grn-extra", 1);
    assert!(reasons(&db).contains(&"PO_LINE_NOT_FOUND".to_string()));
}

#[test]
fn an_over_delivery_is_received_and_gapped_never_truncated() {
    let mut db = configured();
    seed_purchase_order_with_line(
        db.connection(),
        OUTLET,
        SUPPLIER,
        USER,
        RICE,
        "SACK",
        micro(2),
    );

    let stored = db
        .record_goods_receipt(receipt(
            "grn-over",
            Some(PO_ID),
            Some(SUPPLIER),
            vec![line(RICE, "SACK", 5, 200_000, "MASS")],
        ))
        .expect("an over-delivery must be received");

    assert_receipt_landed(&db, "grn-over", 1);
    assert!(reasons(&db).contains(&"QUANTITY_EXCEEDS_ORDERED".to_string()));
    assert_eq!(
        stored.lines[0].base_quantity_micro,
        250 * KG,
        "the excess is FLAGGED, never silently clipped to the ordered amount"
    );
}

#[test]
fn a_delivery_from_an_unconfigured_supplier_is_received_and_gapped() {
    let mut db = configured();
    db.record_goods_receipt(receipt(
        "grn-stranger",
        None,
        Some("supplier-nobody-configured"),
        vec![line(RICE, "kg", 25, 4_000, "MASS")],
    ))
    .expect("a crate on the doorstep still contains food");

    assert_receipt_landed(&db, "grn-stranger", 1);
    assert!(reasons(&db).contains(&"SUPPLIER_NOT_FOUND".to_string()));
}

#[test]
fn a_missing_supplier_item_row_is_received_and_gapped() {
    let mut db = configured();
    db.record_goods_receipt(receipt(
        "grn-nosi",
        None,
        Some(SUPPLIER),
        // The supplier sells SACKs; this arrived measured in kg.
        vec![line(RICE, "kg", 25, 4_000, "MASS")],
    ))
    .expect("record");

    assert_receipt_landed(&db, "grn-nosi", 1);
    assert!(reasons(&db).contains(&"NO_SUPPLIER_ITEM".to_string()));
}

#[test]
fn an_unconvertible_unit_is_received_verbatim_and_gapped() {
    let mut db = configured();
    let stored = db
        .record_goods_receipt(receipt(
            "grn-gunny",
            None,
            Some(SUPPLIER),
            vec![line(RICE, "GUNNY", 7, 100, "MASS")],
        ))
        .expect("an unknown unit must not refuse the delivery");

    assert_receipt_landed(&db, "grn-gunny", 1);
    assert!(reasons(&db).contains(&"NO_UNIT_CONVERSION".to_string()));
    assert_eq!(
        stored.lines[0].base_quantity_micro,
        micro(7),
        "recorded exactly as entered, flagged, rather than guessed at"
    );
}

/// The `x == x` trap, at the level that matters: a receipt whose declared
/// dimension disagrees with the item is ACCEPTED with a `DIMENSION_MISMATCH`.
/// This can only ever fire because the declaration is an input on
/// `NewGrnLine`, never read off `inventory_item`.
#[test]
fn a_dimension_mismatch_is_received_and_gapped() {
    let mut db = configured();
    db.record_goods_receipt(receipt(
        "grn-dim",
        None,
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 1, 200_000, "VOLUME")],
    ))
    .expect("record");

    assert_receipt_landed(&db, "grn-dim", 1);
    assert!(reasons(&db).contains(&"DIMENSION_MISMATCH".to_string()));
}

/// The negative control for the test above. Without it, the mismatch
/// assertions would also pass against an implementation that gapped
/// unconditionally.
#[test]
fn a_matching_dimension_produces_no_mismatch_gap() {
    let mut db = configured();
    db.record_goods_receipt(receipt(
        "grn-ok-dim",
        None,
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 1, 200_000, "MASS")],
    ))
    .expect("record");
    assert!(!reasons(&db).contains(&"DIMENSION_MISMATCH".to_string()));
}

// ---------------------------------------------------------------------------
// Receipt progress, cost, returns and transfers.
// ---------------------------------------------------------------------------

/// The edge derives receipt progress from ITS OWN grn_line rows and writes
/// nothing back to `purchase_order` — the PO stays a cloud-owned config row
/// with no receipt state on it (ADR-019 §4).
#[test]
fn receipt_progress_is_derived_locally_and_never_written_back_to_the_order() {
    let mut db = configured();
    seed_purchase_order_with_line(
        db.connection(),
        OUTLET,
        SUPPLIER,
        USER,
        RICE,
        "SACK",
        micro(10),
    );

    let status_before: String = db
        .connection()
        .query_row(
            "SELECT status FROM purchase_order WHERE id = ?1",
            [PO_ID],
            |r| r.get(0),
        )
        .expect("read status");

    db.record_goods_receipt(receipt(
        "grn-prog",
        Some(PO_ID),
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 4, 200_000, "MASS")],
    ))
    .expect("record");

    let progress = db.purchase_order_receipt_progress(PO_ID).expect("progress");
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0].purchase_order_line_id, PO_LINE_ID);
    assert_eq!(
        progress[0].received_base_quantity_micro_at_this_outlet,
        200 * KG,
        "4 sacks of 50 kg, counted from THIS outlet's receipts only"
    );

    let status_after: String = db
        .connection()
        .query_row(
            "SELECT status FROM purchase_order WHERE id = ?1",
            [PO_ID],
            |r| r.get(0),
        )
        .expect("read status");
    assert_eq!(
        status_before, status_after,
        "receiving must never transition a cloud-owned purchase order"
    );
}

/// M5 acceptance criterion 7, through the shipping API, against a figure
/// computed by hand in the assertion rather than by the code under test.
#[test]
fn weighted_average_cost_after_two_receipts_matches_an_independent_figure() {
    let mut db = configured();

    // Receipt A: 2 sacks (100 kg) at Rs 2000/sack -> Rs 4000 for 100_000 g.
    db.record_goods_receipt(receipt(
        "grn-wac-a",
        None,
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 2, 200_000, "MASS")],
    ))
    .expect("receipt A");
    // Receipt B: 1 sack (50 kg) at Rs 2500/sack -> Rs 2500 for 50_000 g.
    db.record_goods_receipt(receipt(
        "grn-wac-b",
        None,
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 1, 250_000, "MASS")],
    ))
    .expect("receipt B");

    assert_receipt_landed(&db, "grn-wac-a", 1);
    assert_receipt_landed(&db, "grn-wac-b", 1);

    // Independently computed, by hand, from the two invoices:
    //   400_000 paise / 100_000 g = 4 paise/g
    //   250_000 paise /  50_000 g = 5 paise/g
    //   (100_000 x 4 + 50_000 x 5) / 150_000 = 650_000 / 150_000 = 4.33 -> 4
    let independent = (100_000i64 * 4 + 50_000 * 5) / 150_000;
    assert_eq!(independent, 4);

    assert_eq!(
        db.weighted_average_cost_paise(OUTLET, RICE).expect("wac"),
        Some(independent)
    );
}

/// A return posts a NEGATIVE `RETURN_TO_VENDOR` entry, valued at the
/// weighted average when the caller states no price, and stock falls.
#[test]
fn a_purchase_return_posts_a_negative_entry_valued_at_the_weighted_average() {
    let mut db = configured();
    db.record_goods_receipt(receipt(
        "grn-for-return",
        None,
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 2, 200_000, "MASS")],
    ))
    .expect("receipt");
    let before = db.get_current_stock(OUTLET, RICE).expect("stock");

    let stored = db
        .record_purchase_return(NewPurchaseReturn {
            id: "ret-1".to_string(),
            outlet_id: OUTLET.to_string(),
            supplier_id: Some(SUPPLIER.to_string()),
            grn_id: Some("grn-for-return".to_string()),
            return_number: "RET/20260829/0001".to_string(),
            reason: "DAMAGED".to_string(),
            returned_at: "2026-08-29T07:00:00Z".to_string(),
            returned_by_user_id: USER.to_string(),
            notes: None,
            lines: vec![NewPurchaseReturnLine {
                inventory_item_id: RICE.to_string(),
                grn_line_id: None,
                entered_purchase_unit: "SACK".to_string(),
                entered_quantity_micro: micro(1),
                quantity_dimension: "MASS".to_string(),
                unit_cost_paise: None,
            }],
        })
        .expect("record return");

    assert_eq!(stored.lines[0].base_quantity_micro, 50 * KG);
    assert_eq!(
        stored.lines[0].unit_cost_paise, 4,
        "valued at what this outlet actually paid, not a guess"
    );
    assert_eq!(
        db.get_current_stock(OUTLET, RICE).expect("stock") - before,
        -(50 * KG)
    );

    let entry_type: String = db
        .connection()
        .query_row(
            "SELECT entry_type FROM stock_ledger_entry WHERE source_purchase_return_id = 'ret-1'",
            [],
            |r| r.get(0),
        )
        .expect("read entry");
    assert_eq!(entry_type, "RETURN_TO_VENDOR");
}

/// An outbound transfer posts `TRANSFER_OUT` at the source and writes NO
/// inbound row anywhere — the destination half is M8.
#[test]
fn an_outbound_transfer_posts_transfer_out_and_no_inbound_row() {
    let mut db = configured();
    db.record_goods_receipt(receipt(
        "grn-for-transfer",
        None,
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 2, 200_000, "MASS")],
    ))
    .expect("receipt");

    db.record_stock_transfer_out(NewStockTransferOut {
        id: "xfer-1".to_string(),
        outlet_id: OUTLET.to_string(),
        // An outlet this edge database has never heard of: no FK, on purpose.
        destination_outlet_id: "outlet-elsewhere".to_string(),
        transfer_number: "TRF/20260829/0001".to_string(),
        dispatched_at: "2026-08-29T08:00:00Z".to_string(),
        dispatched_by_user_id: USER.to_string(),
        notes: None,
        lines: vec![NewStockTransferLine {
            inventory_item_id: RICE.to_string(),
            base_quantity_micro: 10 * KG,
            quantity_dimension: "MASS".to_string(),
            unit_cost_paise: None,
        }],
    })
    .expect("dispatch");

    let types: Vec<String> = {
        let conn = db.connection();
        let mut stmt = conn
            .prepare("SELECT entry_type FROM stock_ledger_entry ORDER BY entry_seq")
            .expect("prepare");
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect");
        rows
    };
    assert_eq!(types, vec!["PURCHASE", "TRANSFER_OUT"]);
    assert!(
        !types.iter().any(|t| t == "TRANSFER_IN"),
        "the destination receipt is M8 and must not be half-built here"
    );
}

/// Stock never blocks: dispatching more than the ledger holds drives the
/// balance negative rather than refusing, because a negative balance is a
/// variance signal (ADR-018 Rule 1).
#[test]
fn dispatching_more_than_is_on_hand_drives_stock_negative_rather_than_refusing() {
    let mut db = configured();
    db.record_stock_transfer_out(NewStockTransferOut {
        id: "xfer-neg".to_string(),
        outlet_id: OUTLET.to_string(),
        destination_outlet_id: "outlet-elsewhere".to_string(),
        transfer_number: "TRF/20260829/0002".to_string(),
        dispatched_at: "2026-08-29T08:00:00Z".to_string(),
        dispatched_by_user_id: USER.to_string(),
        notes: None,
        lines: vec![NewStockTransferLine {
            inventory_item_id: RICE.to_string(),
            base_quantity_micro: 5 * KG,
            quantity_dimension: "MASS".to_string(),
            unit_cost_paise: Some(4),
        }],
    })
    .expect("a dispatch is never refused for want of stock");

    assert_eq!(
        db.get_current_stock(OUTLET, RICE).expect("stock"),
        -(5 * KG)
    );
}

// ---------------------------------------------------------------------------
// Outbox.
// ---------------------------------------------------------------------------

/// Two aggregate types leave together, in the same transaction as the rows:
/// the receipt, and every gap that explains it.
#[test]
fn a_receipt_and_its_gaps_leave_on_the_outbox_together() {
    let mut db = configured();
    db.record_goods_receipt_with_outbox(
        receipt(
            "grn-outbox",
            None,
            Some(SUPPLIER),
            vec![line(RICE, "GUNNY", 3, 100, "VOLUME")],
        ),
        &ProcurementOutboxMeta {
            outbox_id: "outbox-grn-1".to_string(),
            occurred_at: "2026-08-29T05:30:00Z".to_string(),
        },
    )
    .expect("record with outbox");

    let rows: Vec<(String, String)> = {
        let conn = db.connection();
        let mut stmt = conn
            .prepare("SELECT aggregate_type, event_type FROM local_outbox ORDER BY id")
            .expect("prepare");
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect")
    };

    assert!(rows
        .iter()
        .any(|(a, e)| a == "goods_receipt_note" && e == "GoodsReceived"));
    assert!(
        rows.iter()
            .filter(|(a, e)| a == "grn_gap" && e == "GrnGapRecorded")
            .count()
            >= 3,
        "NO_PURCHASE_ORDER, NO_UNIT_CONVERSION and DIMENSION_MISMATCH each \
         ride out: {rows:?}"
    );

    // A PLAIN OUTBOX: the gap table has no sequence to carry, deliberately
    // (ADR-019 §2). The CONTRAST with stock_deduction_gap, which does carry
    // one, is what makes the absence a decision rather than an omission.
    let has_entry_seq: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('grn_gap') WHERE name = 'entry_seq'",
            [],
            |r| r.get(0),
        )
        .expect("inspect grn_gap");
    assert_eq!(has_entry_seq, 0);

    let deduction_gap_has_one: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('stock_deduction_gap') \
             WHERE name = 'entry_seq'",
            [],
            |r| r.get(0),
        )
        .expect("inspect stock_deduction_gap");
    assert_eq!(deduction_gap_has_one, 1);
}

// ---------------------------------------------------------------------------
// stock_ledger_entry.origin — one member per procurement document (0.6.2)
// ---------------------------------------------------------------------------
//
// Every procurement ledger row said `origin = 'MANUAL'` until contracts 0.6.2,
// which is false and was written permanently into an append-only table.
//
// EACH TEST ASSERTS THE STORED VALUE, not that a row exists. `MANUAL` is still
// a legal `origin` and always will be, so a regression to it breaks no CHECK,
// no trigger and no other assertion in this suite — the only thing that can
// catch it is an equality assertion on the stored string. That is also why
// each test asserts the origin AND its paired provenance column together: the
// pairing is the guarantee ("origin and source_*_id can never disagree about
// which document produced the movement"), and half of it is not evidence.

/// Reads `(origin, entry_type)` for the single ledger row matching a
/// provenance column, and PROVES exactly one such row exists first — a query
/// that matched nothing would make an `assert_eq!` on its result unreachable
/// rather than false.
fn origin_and_type_for(db: &Db, provenance_column: &str, id: &str) -> (String, String) {
    let conn = db.connection();
    let count: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM stock_ledger_entry WHERE {provenance_column} = ?1"),
            [id],
            |r| r.get(0),
        )
        .expect("count rows");
    assert_eq!(
        count, 1,
        "expected exactly one ledger row with {provenance_column} = {id} before asserting on it"
    );
    conn.query_row(
        &format!(
            "SELECT origin, entry_type FROM stock_ledger_entry WHERE {provenance_column} = ?1"
        ),
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .expect("read origin")
}

/// A receipt posts `origin = 'GOODS_RECEIPT'`, paired with `source_grn_id`.
/// Falsifiable by restoring `"MANUAL"` in `ORIGIN_GOODS_RECEIPT`
/// (`edge/database/src/procurement/receipt.rs`): the row still inserts, every
/// other test in this file still passes, and only this assertion goes red —
/// which is the whole reason it is written as an equality on the stored value.
#[test]
fn a_receipt_posts_a_goods_receipt_origin_never_manual() {
    let mut db = configured();
    db.record_goods_receipt(receipt(
        "grn-origin",
        None,
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 1, 200_000, "MASS")],
    ))
    .expect("record receipt");

    let (origin, entry_type) = origin_and_type_for(&db, "source_grn_id", "grn-origin");
    assert_eq!(
        origin, "GOODS_RECEIPT",
        "a delivery is not a hand adjustment: a variance report grouping by origin has to \
         be able to tell them apart"
    );
    assert_eq!(entry_type, "PURCHASE");
}

/// A return posts `origin = 'PURCHASE_RETURN'`, paired with
/// `source_purchase_return_id`.
#[test]
fn a_purchase_return_posts_a_purchase_return_origin_never_manual() {
    let mut db = configured();
    db.record_goods_receipt(receipt(
        "grn-for-origin-return",
        None,
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 2, 200_000, "MASS")],
    ))
    .expect("receipt");

    db.record_purchase_return(NewPurchaseReturn {
        id: "ret-origin".to_string(),
        outlet_id: OUTLET.to_string(),
        supplier_id: Some(SUPPLIER.to_string()),
        grn_id: Some("grn-for-origin-return".to_string()),
        return_number: "RET/20260829/0002".to_string(),
        reason: "DAMAGED".to_string(),
        returned_at: "2026-08-29T07:00:00Z".to_string(),
        returned_by_user_id: USER.to_string(),
        notes: None,
        lines: vec![NewPurchaseReturnLine {
            inventory_item_id: RICE.to_string(),
            grn_line_id: None,
            entered_purchase_unit: "SACK".to_string(),
            entered_quantity_micro: micro(1),
            quantity_dimension: "MASS".to_string(),
            unit_cost_paise: None,
        }],
    })
    .expect("record return");

    let (origin, entry_type) = origin_and_type_for(&db, "source_purchase_return_id", "ret-origin");
    assert_eq!(origin, "PURCHASE_RETURN");
    assert_eq!(entry_type, "RETURN_TO_VENDOR");

    // The receipt's own row in the same database still says GOODS_RECEIPT --
    // proving the two paths write DIFFERENT members and not one shared
    // constant that happens to match here.
    let (receipt_origin, _) = origin_and_type_for(&db, "source_grn_id", "grn-for-origin-return");
    assert_eq!(receipt_origin, "GOODS_RECEIPT");
}

/// An outbound transfer posts `origin = 'STOCK_TRANSFER'`, paired with
/// `source_stock_transfer_out_id`. In `movement.rs` the origin and the
/// provenance column are chosen by ONE match on ONE value, so a row claiming
/// PURCHASE_RETURN with a transfer id is not constructible — this test is what
/// holds that arrangement in place.
#[test]
fn an_outbound_transfer_posts_a_stock_transfer_origin_never_manual() {
    let mut db = configured();
    db.record_goods_receipt(receipt(
        "grn-for-origin-transfer",
        None,
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 2, 200_000, "MASS")],
    ))
    .expect("receipt");

    db.record_stock_transfer_out(NewStockTransferOut {
        id: "xfer-origin".to_string(),
        outlet_id: OUTLET.to_string(),
        destination_outlet_id: "outlet-elsewhere".to_string(),
        transfer_number: "TRF/20260829/0002".to_string(),
        dispatched_at: "2026-08-29T08:00:00Z".to_string(),
        dispatched_by_user_id: USER.to_string(),
        notes: None,
        lines: vec![NewStockTransferLine {
            inventory_item_id: RICE.to_string(),
            base_quantity_micro: 10 * KG,
            quantity_dimension: "MASS".to_string(),
            unit_cost_paise: None,
        }],
    })
    .expect("dispatch");

    let (origin, entry_type) =
        origin_and_type_for(&db, "source_stock_transfer_out_id", "xfer-origin");
    assert_eq!(origin, "STOCK_TRANSFER");
    assert_eq!(entry_type, "TRANSFER_OUT");

    // NOTHING in this database says MANUAL any more: the three procurement
    // paths are the only writers here, and a fourth path quietly falling back
    // to MANUAL would show up as a non-zero count.
    let manual: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM stock_ledger_entry WHERE origin = 'MANUAL'",
            [],
            |r| r.get(0),
        )
        .expect("count MANUAL rows");
    assert_eq!(
        manual, 0,
        "no procurement path may write MANUAL after contracts 0.6.2"
    );
}

// ---------------------------------------------------------------------------
// contracts 0.6.3 / ADR-021 — the ledger stores the INVOICED TOTAL, not only a
// rounded per-unit rate.
// ---------------------------------------------------------------------------

/// FALSIFICATION (§66). Written to FAIL against the pre-0.6.3 code, where the
/// ledger carried only `unit_cost_paise` and weighted average was summed from
/// that rate.
///
/// The price is chosen so the rate does NOT divide evenly. 4 sacks at
/// Rs 2,375 = Rs 9,500 = 950,000 paise over 4 x 50 kg = 200,000 g, which is
/// exactly 4.75 paise/g. Rounded half away from zero that rate is stored as 5,
/// and an average summed from it reads 5 — a +5.26% overstatement, permanently,
/// on every gram.
///
/// The assertion is on the STORED ROW, not only on the average. Testing the
/// average alone would still pass if some other path kept rounding first: the
/// defect is that the exact money was never written down, and that is what this
/// pins.
#[test]
fn the_ledger_stores_the_invoiced_total_exactly_not_a_rounded_rate() {
    let mut db = configured();
    db.record_goods_receipt(receipt(
        "grn-precision",
        None,
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 4, 237_500, "MASS")],
    ))
    .expect("a receipt at a price that does not divide evenly must still land");

    let (total, rate): (Option<i64>, Option<i64>) = db
        .connection()
        .query_row(
            "SELECT line_total_paise, unit_cost_paise FROM stock_ledger_entry \
             WHERE source_grn_id = 'grn-precision'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("read the posted ledger row");

    // The money the invoice actually said, to the paise. This is the assertion
    // that fails on the old code, where the column did not exist at all.
    assert_eq!(
        total,
        Some(950_000),
        "the ledger must carry the invoiced total exactly: 4 sacks at Rs 2,375 is Rs 9,500"
    );

    // The rate is still stored and is still rounded — fine, because it is a
    // display figure and no longer an averaging input. Pinned here so the loss
    // it carries stays visible rather than being quietly assumed away.
    assert_eq!(
        rate,
        Some(5),
        "4.75 paise/g rounds half away from zero to 5 — the rounding that used to reach the average"
    );

    let average = db
        .weighted_average_cost_paise(OUTLET, RICE)
        .expect("read wac")
        .expect("one costed receipt must produce an average");
    assert_eq!(
        average, 5,
        "950,000 paise over 200,000 g is 4.75, which rounds ONCE, at the end"
    );
}

/// The same defect where it accumulates: two receipts whose rates each round in
/// the same direction. Rate-summed, the answer inherits both roundings;
/// total-summed it inherits neither until the final division.
#[test]
fn two_receipts_at_uneven_prices_do_not_accumulate_rounding() {
    let mut db = configured();
    // 4 sacks at Rs 2,375 -> 950,000 paise, exactly 4.75 paise/g (stored 5).
    db.record_goods_receipt(receipt(
        "grn-a",
        None,
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 4, 237_500, "MASS")],
    ))
    .expect("first receipt");
    // 4 sacks at Rs 2,625 -> 1,050,000 paise, exactly 5.25 paise/g (stored 5).
    db.record_goods_receipt(receipt(
        "grn-b",
        None,
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 4, 262_500, "MASS")],
    ))
    .expect("second receipt");

    let average = db
        .weighted_average_cost_paise(OUTLET, RICE)
        .expect("read wac")
        .expect("average");

    // Computed from the INVOICES, not from the ledger, so this cannot agree
    // with the code by sharing its arithmetic.
    let invoiced_paise: i128 = 950_000 + 1_050_000;
    let grams_received: i128 = 400_000;
    assert_eq!(i128::from(average), invoiced_paise / grams_received);
    assert_eq!(average, 5);
}

/// REGRESSION GUARD, not a falsification: it passes today by design.
///
/// A positive count adjustment is valued at the current average, so folding it
/// back in would let a count drag the purchase-weighted figure. It is kept out
/// by `stock::count` writing NO cost at all — one layer earlier than the
/// averaging filter.
///
/// **`line_total_paise IS NOT NULL` is NOT what defends this.** Whoever adds a
/// cost to a count adjustment will very likely add a total with it: the two
/// travel together by convention now, and the filter would admit the row. THIS
/// TEST IS THE DEFENCE. It fails the day a count adjustment is costed, which is
/// a change that would look like an improvement.
#[test]
fn count_adjustments_are_uncosted_and_never_enter_the_average() {
    let mut db = configured();
    db.record_goods_receipt(receipt(
        "grn-base",
        None,
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 4, 250_000, "MASS")],
    ))
    .expect("receipt");

    let before = db
        .weighted_average_cost_paise(OUTLET, RICE)
        .expect("read wac")
        .expect("average");

    // ACTUALLY POST A COUNT ADJUSTMENT. Asserting COUNT(*) = 0 over a table
    // that contains no count adjustments at all is green on absent data -- it
    // passes whatever stock::count does, which makes it worthless as a guard.
    // Falsified: with count.rs temporarily costing its rows, the version of
    // this test without this block still passed.
    db.open_stock_count(holler_edge_database::model::NewStockCount {
        id: "count-guard".to_string(),
        outlet_id: OUTLET.to_string(),
        started_at: "2026-08-29T09:00:00Z".to_string(),
        counted_by_user_id: Some(USER.to_string()),
        note: None,
    })
    .expect("open the count");
    db.add_or_update_stock_count_line(
        "count-guard",
        OUTLET,
        holler_edge_database::model::NewStockCountLine {
            inventory_item_id: RICE.to_string(),
            // MORE than the receipt put in, so the adjustment is POSITIVE --
            // the case that would drag the average upward if it were costed.
            counted_quantity_micro: 300 * KG,
            note: None,
        },
    )
    .expect("count a line");
    db.complete_stock_count("count-guard", OUTLET, "2026-08-29T09:30:00Z")
        .expect("complete the count");

    let adjustments: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM stock_ledger_entry WHERE origin = 'COUNT_ADJUSTMENT'",
            [],
            |r| r.get(0),
        )
        .expect("count adjustment rows");
    assert!(
        adjustments > 0,
        "fixture assertion: the count must have posted an adjustment, or every assertion below is green on absent data"
    );

    let costed_adjustments: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM stock_ledger_entry \
             WHERE origin = 'COUNT_ADJUSTMENT' AND unit_cost_paise IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .expect("count costed adjustments");
    assert_eq!(
        costed_adjustments, 0,
        "no path may write a cost onto a count adjustment: it is valued AT the average, and costing it would let a count move the purchase price"
    );

    let after = db
        .weighted_average_cost_paise(OUTLET, RICE)
        .expect("read wac")
        .expect("average");
    assert_eq!(before, after);
}

/// The overflow bound, stated and covered.
///
/// `quantity_applied_micro * unit_cost_paise` must fit `i64` wherever it is
/// still formed — the 0030/0031 backfill expression, and the drift relation.
/// Quantity is bounded at 1e15 micro-units by
/// `stock_ledger_entry_quantity_is_bounded`, so the product overflows once the
/// rate passes about 9,223 paise per base unit (i64::MAX / 1e15). The averaging
/// path no longer forms that product at all — it sums money and multiplies by
/// 1e6 inside an `i128` — which is the point: the bound constrains the legacy
/// expression, not the new one.
#[test]
fn the_cost_product_bound_is_documented_and_reachable() {
    const MAX_QUANTITY_MICRO: i128 = 1_000_000_000_000_000;
    let max_safe_rate = i128::from(i64::MAX) / MAX_QUANTITY_MICRO;
    assert_eq!(
        max_safe_rate, 9223,
        "at the 1e15 quantity bound the rate may not exceed 9,223 paise per base unit before quantity x rate overflows i64"
    );

    // One expensive row through the real receipt path: 1 sack of 50 kg at
    // Rs 40,000 is 800 paise/g, and the exact total survives.
    let mut db = configured();
    db.record_goods_receipt(receipt(
        "grn-bound",
        None,
        Some(SUPPLIER),
        vec![line(RICE, "SACK", 1, 4_000_000, "MASS")],
    ))
    .expect("an expensive receipt must land");

    let total: Option<i64> = db
        .connection()
        .query_row(
            "SELECT line_total_paise FROM stock_ledger_entry WHERE source_grn_id = 'grn-bound'",
            [],
            |r| r.get(0),
        )
        .expect("read total");
    assert_eq!(total, Some(4_000_000));

    let average = db
        .weighted_average_cost_paise(OUTLET, RICE)
        .expect("read wac")
        .expect("average");
    assert_eq!(average, 80, "4,000,000 paise over 50,000 g is 80 paise/g");
}

/// THE DRIFT TEST the module doc used to only claim. For every costed receipt
/// row, the stored rate must be exactly what deriving it from the stored total
/// produces — one expression, one place that can be wrong.
///
/// This is what replaces a cross-aggregate trigger: nothing forces
/// `line_total_paise` to equal the invoiced money, but the two columns can
/// never disagree about the arithmetic between them.
#[test]
fn the_stored_rate_is_always_derived_from_the_stored_total() {
    let mut db = configured();
    for (id, price) in [
        ("grn-d1", 237_500),
        ("grn-d2", 262_500),
        ("grn-d3", 250_000),
        ("grn-d4", 1),
    ] {
        db.record_goods_receipt(receipt(
            id,
            None,
            Some(SUPPLIER),
            vec![line(RICE, "SACK", 3, price, "MASS")],
        ))
        .expect("receipt");
    }

    let conn = db.connection();
    let mut stmt = conn
        .prepare(
            "SELECT quantity_applied_micro, unit_cost_paise, line_total_paise \
             FROM stock_ledger_entry WHERE line_total_paise IS NOT NULL",
        )
        .expect("prepare");
    let rows: Vec<(i64, i64, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect();
    assert_eq!(rows.len(), 4, "every receipt must have posted a costed row");

    for (quantity_micro, rate, total) in rows {
        // Half away from zero, matching convert.rs. Written out rather than
        // calling the crate's helper, so the test cannot agree with the code by
        // sharing the function that might be wrong.
        let numerator = i128::from(total) * 1_000_000;
        let denominator = i128::from(quantity_micro);
        let expected = if numerator >= 0 {
            (numerator + denominator / 2) / denominator
        } else {
            (numerator - denominator / 2) / denominator
        };
        assert_eq!(
            i128::from(rate),
            expected,
            "unit_cost_paise must be derived from line_total_paise, never computed independently"
        );
    }
}
