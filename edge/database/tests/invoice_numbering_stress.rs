//! T7b task 2 (ADR-016 §2) verification: numbers minted under repeated
//! issuance are unique and correctly formatted, and a reset boundary starts
//! a fresh sequence rather than colliding with the previous bucket.
//!
//! Runtime: `cargo test`, native Windows (this crate has no non-Windows
//! target — ADR-013).

mod support;

use std::collections::HashSet;

use holler_edge_database::model::{InvoiceLineShare, InvoiceOutboxMeta};
use holler_edge_database::Db;

/// Issues `n` invoices, one per freshly created single-item order, all
/// under the same series/business_date, and asserts every rendered
/// `invoice_number` is unique and matches the expected zero-padded shape.
#[test]
fn many_invoices_under_one_series_never_collide_and_are_correctly_padded() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    support::seed(&db, "SALES", "NEVER");

    let n = 250; // "many" without making this test slow
    let mut seen = HashSet::new();

    for i in 1..=n {
        let order_id = format!("order-{i}");
        let item_ids = support::create_order(&mut db, &order_id, 20_000, &[1]);

        let header = support::header(&order_id, "SALES", "2026-08-12", "2026-08-12T10:00:00Z");
        let share = InvoiceLineShare {
            id: format!("invline-{i}"),
            order_item_id: item_ids[0].clone(),
            quantity: 1,
            discount_per_unit_paise: 0,
        };
        let meta = InvoiceOutboxMeta {
            outbox_id: format!("outbox-invoice-{i}"),
            occurred_at: "2026-08-12T10:00:00Z".to_string(),
        };

        let issued = db
            .issue_invoice_with_outbox(&header, format!("invoice-{i}"), vec![share], &meta)
            .unwrap_or_else(|e| panic!("issue invoice {i}: {e}"));

        assert_eq!(
            issued.invoice_number,
            format!("INV-{i:06}"),
            "invoice {i} must render as INV-<6-digit padded sequence>"
        );
        assert!(
            seen.insert(issued.invoice_number.clone()),
            "invoice number {} (issuance {i}) collided with an earlier one",
            issued.invoice_number
        );
    }

    assert_eq!(seen.len(), n, "every issued number must be unique");
}

/// `reset_policy = 'DAY'`: a new `business_date` must start a fresh
/// `#...000001`-style sequence rather than continuing the previous day's
/// count — proving the reset boundary works, not just that numbers within
/// one bucket are unique.
#[test]
fn day_reset_policy_starts_a_fresh_sequence_at_the_boundary() {
    use holler_edge_database::model::InvoiceSeries;
    use holler_edge_database::repo;

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    support::seed(&db, "SALES", "NEVER"); // base fixtures
    // Its own series with a DATE-INCLUSIVE prefix: a DAY reset that leaves a
    // static prefix unchanged would (correctly) render the SAME
    // invoice_number on two different days once both buckets reach the same
    // counter value — that is a config-template mismatch, not a numbering
    // bug, so this test's series is built the way ADR-016 §2 intends a DAY
    // series to look.
    repo::upsert_invoice_series(
        db.connection(),
        &InvoiceSeries {
            id: "series-DAY-SALES".to_string(),
            outlet_id: support::OUTLET_ID.to_string(),
            code: "DAY-SALES".to_string(),
            prefix_template: "{YYYY}{MM}{DD}-".to_string(),
            reset_policy: "DAY".to_string(),
            padding_width: 4,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed DAY series");

    let issue = |db: &mut Db, order_id: &str, business_date: &str, invoice_date: &str| {
        let item_ids = support::create_order(db, order_id, 20_000, &[1]);
        let header = support::header(order_id, "DAY-SALES", business_date, invoice_date);
        let share = InvoiceLineShare {
            id: format!("{order_id}-line"),
            order_item_id: item_ids[0].clone(),
            quantity: 1,
            discount_per_unit_paise: 0,
        };
        let meta = InvoiceOutboxMeta {
            outbox_id: format!("{order_id}-outbox-invoice"),
            occurred_at: invoice_date.to_string(),
        };
        db.issue_invoice_with_outbox(&header, format!("{order_id}-invoice"), vec![share], &meta)
            .expect("issue")
    };

    let day1_a = issue(&mut db, "d1-order-1", "2026-08-12", "2026-08-12T09:00:00Z");
    let day1_b = issue(&mut db, "d1-order-2", "2026-08-12", "2026-08-12T20:00:00Z");
    assert_eq!(day1_a.invoice_number, "20260812-0001");
    assert_eq!(day1_b.invoice_number, "20260812-0002");

    // A new business_date: the sequence resets AND the rendered prefix
    // changes, so the two days' numbers cannot collide even at the same
    // counter value.
    let day2_a = issue(&mut db, "d2-order-1", "2026-08-13", "2026-08-13T09:00:00Z");
    assert_eq!(
        day2_a.invoice_number, "20260813-0001",
        "a new business_date under reset_policy=DAY must restart the sequence"
    );

    // Returning to day 1's bucket later (a late-settled bill, or a resend)
    // must continue day 1's OWN count, not day 2's — buckets are keyed by
    // period, not by call order.
    let day1_c = issue(&mut db, "d1-order-3", "2026-08-12", "2026-08-12T22:00:00Z");
    assert_eq!(day1_c.invoice_number, "20260812-0003");
}

/// `reset_policy = 'FY'`: crossing the 1 April fiscal-year boundary must
/// both reset the counter AND change the rendered `{FY}` token, matching
/// ADR-016's own worked example (`'FY{FY}/{OUTLET}/'` -> `'FY26/PNQ/...'`).
/// Uses its own series (with a `{FY}` token in the prefix) rather than
/// `support::seed`'s plain `"INV-"` template, so the token substitution
/// itself is under test, not just the reset boundary.
#[test]
fn fy_reset_policy_resets_and_renders_the_fy_token_across_the_1_april_boundary() {
    use holler_edge_database::model::InvoiceSeries;
    use holler_edge_database::repo;

    let mut db = Db::open_in_memory_for_tests().expect("open db");
    support::seed(&db, "SALES", "NEVER"); // base fixtures (outlet/menu/tax config)
    repo::upsert_invoice_series(
        db.connection(),
        &InvoiceSeries {
            id: "series-FY-SALES".to_string(),
            outlet_id: support::OUTLET_ID.to_string(),
            code: "FY-SALES".to_string(),
            prefix_template: "FY{FY}/PUN/".to_string(),
            reset_policy: "FY".to_string(),
            padding_width: 4,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed FY series");

    let issue = |db: &mut Db, order_id: &str, business_date: &str, invoice_date: &str| {
        let item_ids = support::create_order(db, order_id, 20_000, &[1]);
        let header = support::header(order_id, "FY-SALES", business_date, invoice_date);
        let share = InvoiceLineShare {
            id: format!("{order_id}-line"),
            order_item_id: item_ids[0].clone(),
            quantity: 1,
            discount_per_unit_paise: 0,
        };
        let meta = InvoiceOutboxMeta {
            outbox_id: format!("{order_id}-outbox-invoice"),
            occurred_at: invoice_date.to_string(),
        };
        db.issue_invoice_with_outbox(&header, format!("{order_id}-invoice"), vec![share], &meta)
            .expect("issue")
    };

    // Late in FY26 (Apr 2025 - Mar 2026): two bills.
    let fy26_a = issue(&mut db, "fy26-order-1", "2026-03-30", "2026-03-30T10:00:00Z");
    let fy26_b = issue(&mut db, "fy26-order-2", "2026-03-31", "2026-03-31T23:00:00Z");
    assert_eq!(fy26_a.invoice_number, "FY26/PUN/0001");
    assert_eq!(fy26_b.invoice_number, "FY26/PUN/0002");

    // Crossing 1 April into FY27: both the token AND the counter reset.
    let fy27_a = issue(&mut db, "fy27-order-1", "2026-04-01", "2026-04-01T00:00:01Z");
    assert_eq!(
        fy27_a.invoice_number, "FY27/PUN/0001",
        "1 April must both change the {{FY}} token to FY27 and restart the counter at 1"
    );

    // FY26's bucket, revisited later, continues where it left off — proving
    // the reset is keyed by fiscal-year bucket, not merely by "most recent
    // business_date seen".
    let fy26_c = issue(&mut db, "fy26-order-3", "2026-03-15", "2026-03-15T10:00:00Z");
    assert_eq!(fy26_c.invoice_number, "FY26/PUN/0003");
}
