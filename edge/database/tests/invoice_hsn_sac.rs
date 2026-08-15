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

use holler_edge_database::model::{InvoiceLineShare, InvoiceOutboxMeta, MenuItem};
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
