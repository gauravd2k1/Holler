//! ADR-016 0.4.5 §3 / the accompanying track's load-bearing half: an
//! invoice must not issue with a line whose resolved `menu_item.hsn_sac`
//! is NULL or blank. Falsification-style, per the track's own instructions
//! — this file exercises BOTH directions: the rejection actually rejects
//! (naming the offending item), and issuance actually succeeds again once
//! the catalogue gap is fixed.
//!
//! Runtime: `cargo test`, native Windows (this crate has no non-Windows
//! target — ADR-013).

mod support;

use holler_edge_database::model::{
    InvoiceLineShare, InvoiceOutboxMeta, MenuItem, NewOrder, NewOrderItem, NewOutboxEntry,
};
use holler_edge_database::repo;
use holler_edge_database::{Db, DbError};

/// `support::seed` gives `MENU_ITEM_ID` a real SAC code (9963) so every
/// other invoice test can issue at all. This test blanks it back to `NULL`
/// — the state every pre-0.4.5 catalogue is actually in — and confirms
/// issuance is rejected, naming the item, rather than silently printing a
/// blank HSN/SAC field on a GST tax invoice.
#[test]
fn issuance_is_rejected_when_the_resolved_hsn_sac_is_null_and_names_the_item() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    support::seed(&db, "SALES", "NEVER");

    // Blank the seeded item's hsn_sac back to NULL — same upsert path a
    // cloud config sync would use, so this exercises the real read path,
    // not a hand-rolled UPDATE bypassing it.
    repo::upsert_menu_item(
        db.connection(),
        &MenuItem {
            id: support::MENU_ITEM_ID.to_string(),
            outlet_id: support::OUTLET_ID.to_string(),
            category_id: support::CATEGORY_ID.to_string(),
            name: "Thali".to_string(),
            base_price_paise: 20_000,
            is_available: true,
            config_version: 2, // must be >= current so the upsert is not a stale no-op
            tax_profile_id: None,
            hsn_sac: None,
        },
    )
    .expect("blank hsn_sac");

    let order_id = "order-missing-hsn";
    let item_ids = support::create_order(&mut db, order_id, 20_000, &[1]);

    let header = support::header(order_id, "SALES", "2026-08-12", "2026-08-12T10:00:00Z");
    let share = InvoiceLineShare {
        id: "invline-missing-hsn".to_string(),
        order_item_id: item_ids[0].clone(),
        quantity: 1,
        discount_per_unit_paise: 0,
    };
    let meta = InvoiceOutboxMeta {
        outbox_id: "outbox-invoice-missing-hsn".to_string(),
        occurred_at: "2026-08-12T10:00:00Z".to_string(),
    };

    let err = db
        .issue_invoice_with_outbox(
            &header,
            "invoice-missing-hsn".to_string(),
            vec![share],
            &meta,
        )
        .expect_err("issuance must be rejected when the resolved hsn_sac is NULL");

    match &err {
        DbError::MissingHsnSac {
            order_id: oid,
            items,
        } => {
            assert_eq!(oid, order_id);
            assert_eq!(
                items.len(),
                1,
                "exactly the one offending item must be named"
            );
            assert_eq!(
                items[0].name, "Thali",
                "the error must name the offending item"
            );
        }
        other => panic!("expected DbError::MissingHsnSac, got: {other:?}"),
    }
    let rendered = err.to_string();
    assert!(
        rendered.contains("Thali"),
        "the rendered §64 message must name the offending item, got: {rendered}"
    );

    // No invoice/invoice_line row must have been written by the rejected
    // attempt — the same all-or-nothing shape as every other pre-write
    // validation in this crate.
    assert!(
        db.get_invoice("invoice-missing-hsn")
            .expect("read")
            .is_none(),
        "a rejected issuance must leave no invoice row behind"
    );

    // Restore the code (the manager "fixes the catalogue" step the error
    // message points at) and confirm the SAME order now issues cleanly.
    repo::upsert_menu_item(
        db.connection(),
        &MenuItem {
            id: support::MENU_ITEM_ID.to_string(),
            outlet_id: support::OUTLET_ID.to_string(),
            category_id: support::CATEGORY_ID.to_string(),
            name: "Thali".to_string(),
            base_price_paise: 20_000,
            is_available: true,
            config_version: 3,
            tax_profile_id: None,
            hsn_sac: Some("9963".to_string()),
        },
    )
    .expect("restore hsn_sac");

    let header2 = support::header(order_id, "SALES", "2026-08-12", "2026-08-12T10:01:00Z");
    let share2 = InvoiceLineShare {
        id: "invline-restored".to_string(),
        order_item_id: item_ids[0].clone(),
        quantity: 1,
        discount_per_unit_paise: 0,
    };
    let meta2 = InvoiceOutboxMeta {
        outbox_id: "outbox-invoice-restored".to_string(),
        occurred_at: "2026-08-12T10:01:00Z".to_string(),
    };
    let issued = db
        .issue_invoice_with_outbox(
            &header2,
            "invoice-restored".to_string(),
            vec![share2],
            &meta2,
        )
        .expect("issuance must succeed once hsn_sac is restored");

    let lines = db.list_invoice_lines(&issued.id).expect("read lines");
    assert_eq!(lines.len(), 1);
    assert_eq!(
        lines[0].hsn_sac.as_deref(),
        Some("9963"),
        "the issued line must snapshot the resolved hsn_sac"
    );
}

/// A blank (whitespace-only) code is treated the same as NULL — not a
/// separate bug class, just the same guard applied to a different way of
/// spelling "no real code".
#[test]
fn issuance_is_rejected_when_the_resolved_hsn_sac_is_blank() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    support::seed(&db, "SALES", "NEVER");

    repo::upsert_menu_item(
        db.connection(),
        &MenuItem {
            id: support::MENU_ITEM_ID.to_string(),
            outlet_id: support::OUTLET_ID.to_string(),
            category_id: support::CATEGORY_ID.to_string(),
            name: "Thali".to_string(),
            base_price_paise: 20_000,
            is_available: true,
            config_version: 2,
            tax_profile_id: None,
            hsn_sac: Some("   ".to_string()),
        },
    )
    .expect("blank (whitespace) hsn_sac");

    let order_id = "order-blank-hsn";
    let item_ids = support::create_order(&mut db, order_id, 20_000, &[1]);
    let header = support::header(order_id, "SALES", "2026-08-12", "2026-08-12T10:00:00Z");
    let share = InvoiceLineShare {
        id: "invline-blank-hsn".to_string(),
        order_item_id: item_ids[0].clone(),
        quantity: 1,
        discount_per_unit_paise: 0,
    };
    let meta = InvoiceOutboxMeta {
        outbox_id: "outbox-invoice-blank-hsn".to_string(),
        occurred_at: "2026-08-12T10:00:00Z".to_string(),
    };

    let err = db
        .issue_invoice_with_outbox(&header, "invoice-blank-hsn".to_string(), vec![share], &meta)
        .expect_err("issuance must be rejected when the resolved hsn_sac is blank");
    assert!(matches!(err, DbError::MissingHsnSac { .. }));
}

/// The gate found this gap: every shipped test exercised the guard only via
/// `issue_invoice_with_outbox` (the unsplit path). `issue_split_invoices_with_outbox`
/// has its own loop over parts and its own transaction — a guard that only
/// ran in the unsplit path would leave split-bill issuance able to print a
/// GST invoice with a blank HSN/SAC field.
///
/// Builds an order with two distinct menu items (so one part can carry a
/// real code while the other's is blanked), splits it two ways — one item
/// per part — and asserts all three of what the task calls for: the
/// rejection happens, the error names the offending item, and NEITHER
/// part's invoice row nor its invoice number survives, so a rejected split
/// never burns a number and never leaves gapless numbering with a hole in
/// it.
#[test]
fn split_issuance_is_rejected_when_one_parts_item_is_missing_hsn_sac_and_burns_no_number() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    support::seed(&db, "SALES", "NEVER");

    // A second menu item, sharing the seeded category, with NO hsn_sac —
    // the state a catalogue gap actually looks like.
    const BAD_ITEM_ID: &str = "menu-item-bad-hsn";
    repo::upsert_menu_item(
        db.connection(),
        &MenuItem {
            id: BAD_ITEM_ID.to_string(),
            outlet_id: support::OUTLET_ID.to_string(),
            category_id: support::CATEGORY_ID.to_string(),
            name: "Butter Naan".to_string(),
            base_price_paise: 4_000,
            is_available: true,
            config_version: 1,
            tax_profile_id: None,
            hsn_sac: None,
        },
    )
    .expect("seed second menu item with no hsn_sac");

    let order_id = "order-split-missing-hsn";
    let good_item_id = format!("{order_id}-item-good");
    let bad_item_id = format!("{order_id}-item-bad");

    let order = NewOrder {
        id: order_id.to_string(),
        outlet_id: support::OUTLET_ID.to_string(),
        device_id: support::DEVICE_ID.to_string(),
        order_type: "DINE_IN".to_string(),
        status: "DRAFT".to_string(),
        table_id: None,
        subtotal_paise: 0,
        discount_paise: 0,
        taxes_paise: 0,
        total_paise: 0,
        source: "POS".to_string(),
        external_order_id: None,
        payment_status: "UNPAID".to_string(),
        payment_source: None,
        confirmed_at: None,
        source_payload_json: None,
        schema_version: 1,
        created_at: "2026-08-12T10:00:00Z".to_string(),
        updated_at: "2026-08-12T10:00:00Z".to_string(),
    };
    let items = vec![
        NewOrderItem {
            id: good_item_id.clone(),
            order_id: order_id.to_string(),
            menu_item_id: support::MENU_ITEM_ID.to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 20_000,
            line_total_paise: 20_000,
            notes: None,
            created_at: "2026-08-12T10:00:00Z".to_string(),
        },
        NewOrderItem {
            id: bad_item_id.clone(),
            order_id: order_id.to_string(),
            menu_item_id: BAD_ITEM_ID.to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 4_000,
            line_total_paise: 4_000,
            notes: None,
            created_at: "2026-08-12T10:00:00Z".to_string(),
        },
    ];
    let outbox = NewOutboxEntry {
        id: format!("outbox-{order_id}"),
        aggregate_type: "order".to_string(),
        aggregate_id: order_id.to_string(),
        event_type: "OrderCreated".to_string(),
        payload_json: "{}".to_string(),
        created_at: "2026-08-12T10:00:00Z".to_string(),
    };
    db.create_order_with_outbox(&order, &items, &outbox)
        .expect("create order");

    // Snapshot invoice_sequence before the attempt — used below to prove
    // the guard fires before any number is minted, and that a rejected
    // split leaves gapless numbering intact rather than burning a number
    // per failed part.
    let sequence_before: i64 = db
        .connection()
        .query_row(
            "SELECT COALESCE(SUM(last_value), 0) FROM invoice_sequence",
            [],
            |row| row.get(0),
        )
        .expect("read invoice_sequence before");

    let header = support::header(order_id, "SALES", "2026-08-12", "2026-08-12T10:00:00Z");
    let parts = vec![
        (
            "invoice-split-good".to_string(),
            vec![InvoiceLineShare {
                id: "line-split-good".to_string(),
                order_item_id: good_item_id.clone(),
                quantity: 1,
                discount_per_unit_paise: 0,
            }],
        ),
        (
            "invoice-split-bad".to_string(),
            vec![InvoiceLineShare {
                id: "line-split-bad".to_string(),
                order_item_id: bad_item_id.clone(),
                quantity: 1,
                discount_per_unit_paise: 0,
            }],
        ),
    ];
    let metas = vec![
        InvoiceOutboxMeta {
            outbox_id: "outbox-split-good".to_string(),
            occurred_at: "2026-08-12T10:00:00Z".to_string(),
        },
        InvoiceOutboxMeta {
            outbox_id: "outbox-split-bad".to_string(),
            occurred_at: "2026-08-12T10:00:00Z".to_string(),
        },
    ];

    let err = db
        .issue_split_invoices_with_outbox(
            &header,
            "split-missing-hsn-group".to_string(),
            parts,
            &metas,
        )
        .expect_err("split issuance must be rejected when any part's resolved hsn_sac is NULL");

    match &err {
        DbError::MissingHsnSac {
            order_id: oid,
            items,
        } => {
            assert_eq!(oid, order_id);
            assert_eq!(
                items.len(),
                1,
                "exactly the one offending item must be named"
            );
            assert_eq!(
                items[0].name, "Butter Naan",
                "the error must name the offending item"
            );
        }
        other => panic!("expected DbError::MissingHsnSac, got: {other:?}"),
    }

    // Neither part's invoice row was written — not even the part whose own
    // item had a real hsn_sac.
    assert!(
        db.get_invoice("invoice-split-good")
            .expect("read")
            .is_none(),
        "the good part's invoice row must not survive a rejected split"
    );
    assert!(
        db.get_invoice("invoice-split-bad").expect("read").is_none(),
        "the bad part's invoice row must not survive a rejected split"
    );
    assert!(
        db.list_invoices_for_order(order_id)
            .expect("list")
            .is_empty(),
        "a rejected split must leave no invoice row behind for this order"
    );
    assert!(
        db.list_invoices_for_split_group("split-missing-hsn-group")
            .expect("list by split group")
            .is_empty(),
        "a rejected split must leave no invoice reachable by its split_group_id"
    );

    // Gapless numbering: the guard fires (in build_invoice) before
    // numbering::mint_invoice_number is ever called for the rejected part,
    // and the whole call runs in one transaction, so even the number that
    // WOULD have been minted for the good part (which built successfully
    // before the bad part failed) is rolled back with it.
    let sequence_after: i64 = db
        .connection()
        .query_row(
            "SELECT COALESCE(SUM(last_value), 0) FROM invoice_sequence",
            [],
            |row| row.get(0),
        )
        .expect("read invoice_sequence after");
    assert_eq!(
        sequence_before, sequence_after,
        "a rejected split issuance must not consume any invoice number, \
         for either part — burning a number on every failure would break \
         gapless numbering"
    );
}
