//! T7c verification: payments are append-only (docs/spec/payments.md
//! §Conflict policy) and a cash shift close rejects a non-zero variance
//! without a reason (§39) — exercised through the PUBLIC `Db` API, not the
//! module-internal unit tests in `src/payment/*.rs`.
//!
//! Runtime: `cargo test`, native Windows (this crate has no non-Windows
//! target — ADR-013).

mod support;

use holler_edge_database::model::{
    CashShiftOutboxMeta, CloseCashShiftRequest, NewCashShift, NewPayment, PaymentOutboxMeta,
};
use holler_edge_database::{DbError, Db};

fn open_seeded() -> Db {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    support::seed(&db, "SALES", "NEVER");
    support::create_order(&mut db, "order-1", 10_000, &[1]);
    db
}

fn forward(id: &str, amount_paise: i64) -> NewPayment {
    NewPayment {
        id: id.to_string(),
        outlet_id: support::OUTLET_ID.to_string(),
        order_id: "order-1".to_string(),
        cash_shift_id: None,
        method: "UPI".to_string(),
        status: "CAPTURED".to_string(),
        amount_paise,
        tendered_paise: None,
        change_paise: None,
        reference: Some("UTR123".to_string()),
        external_id: None,
        reverses_payment_id: None,
        captured_at: Some("2026-08-14T10:00:00Z".to_string()),
        created_by_user_id: support::USER_ID.to_string(),
        created_at: "2026-08-14T10:00:00Z".to_string(),
        updated_at: "2026-08-14T10:00:00Z".to_string(),
    }
}

fn reversal(id: &str, reverses: &str, amount_paise: i64) -> NewPayment {
    let mut p = forward(id, amount_paise);
    p.reverses_payment_id = Some(reverses.to_string());
    p
}

fn pay_meta(id: &str) -> PaymentOutboxMeta {
    PaymentOutboxMeta {
        outbox_id: id.to_string(),
        occurred_at: "2026-08-14T10:00:00Z".to_string(),
    }
}

/// The binding property: a payment row, once written, is never updated or
/// deleted. Reads the row back byte-for-field-equal before and after two
/// reversals are posted against it, then proves settlement is arithmetic
/// over the append-only rows, never a rewrite of the original.
#[test]
fn a_payment_row_is_never_mutated_by_its_own_reversals() {
    let mut db = open_seeded();

    let (original, _) = db
        .record_payment_with_outbox(forward("pay-1", 200_000), "mv-1", "a-x", None, &pay_meta("out-1"))
        .expect("forward payment");
    let snapshot_before = db.get_payment("pay-1").expect("read").expect("exists");

    let (_r1, _) = db
        .record_payment_with_outbox(reversal("pay-2", "pay-1", -50_000), "mv-2", "a-x", None, &pay_meta("out-2"))
        .expect("partial reversal 1");
    let (_r2, _) = db
        .record_payment_with_outbox(reversal("pay-3", "pay-1", -30_000), "mv-3", "a-x", None, &pay_meta("out-3"))
        .expect("partial reversal 2");

    let snapshot_after = db.get_payment("pay-1").expect("read").expect("still exists");

    assert_eq!(snapshot_before.amount_paise, snapshot_after.amount_paise);
    assert_eq!(snapshot_before.updated_at, snapshot_after.updated_at);
    assert_eq!(snapshot_before.version, snapshot_after.version);
    assert_eq!(original.amount_paise, 200_000);
    assert_eq!(snapshot_after.amount_paise, 200_000, "the original row's own amount never changes");

    // Settlement is Σ(forward) + Σ(reversals) — never a value stored on the
    // original row itself.
    let all = db.list_payments_for_order("order-1").expect("list");
    let settled: i64 = all.iter().map(|p| p.amount_paise).sum();
    assert_eq!(settled, 200_000 - 50_000 - 30_000);
    assert_eq!(all.len(), 3, "three append-only rows, never fewer");
}

/// A reversal missing `reverses_payment_id` is just a second forward
/// payment as far as this crate is concerned — the append-only guarantee is
/// about `reverses_payment_id`, not inferred from a negative amount alone.
/// This test instead proves the mirror case named by the task: a reversal
/// row (`reverses_payment_id` set) with a POSITIVE amount is rejected, and
/// with a non-positive amount it is accepted — through the public API.
#[test]
fn reversal_amount_sign_is_enforced_through_the_public_api() {
    let mut db = open_seeded();
    db.record_payment_with_outbox(forward("pay-1", 100_000), "mv-1", "a-x", None, &pay_meta("out-1"))
        .expect("forward payment");

    let err = db
        .record_payment_with_outbox(reversal("pay-2", "pay-1", 10_000), "mv-2", "a-x", None, &pay_meta("out-2"))
        .expect_err("a positive-amount reversal must be rejected");
    assert!(matches!(err, DbError::ReversalAmountNotNonPositive { .. }));

    db.record_payment_with_outbox(reversal("pay-3", "pay-1", -10_000), "mv-3", "a-x", None, &pay_meta("out-3"))
        .expect("a non-positive reversal must be accepted");
}

/// Reversing an already-fully-reversed payment through the public API must
/// be rejected, not silently doubled — the task's own example, checked at
/// the `Db` boundary rather than only in the module-internal unit test.
#[test]
fn already_fully_reversed_payment_cannot_be_reversed_again_via_public_api() {
    let mut db = open_seeded();
    db.record_payment_with_outbox(forward("pay-1", 100_000), "mv-1", "a-x", None, &pay_meta("out-1"))
        .expect("forward payment");
    db.record_payment_with_outbox(reversal("pay-2", "pay-1", -100_000), "mv-2", "a-x", None, &pay_meta("out-2"))
        .expect("full reversal");

    let err = db
        .record_payment_with_outbox(reversal("pay-3", "pay-1", -1), "mv-3", "a-x", None, &pay_meta("out-3"))
        .expect_err("reversing an already-fully-reversed payment must be rejected");
    assert!(matches!(err, DbError::PaymentAlreadyFullyReversed { .. }));

    // And the row set is unchanged by the rejected attempt.
    let all = db.list_payments_for_order("order-1").expect("list");
    assert_eq!(all.len(), 2, "a rejected reversal writes nothing");
}

fn shift_meta(id: &str) -> CashShiftOutboxMeta {
    CashShiftOutboxMeta {
        outbox_id: id.to_string(),
        occurred_at: "2026-08-14T09:00:00Z".to_string(),
    }
}

/// §39, through the public API: a non-zero variance close without a reason
/// is rejected outright, and the shift stays OPEN — a cashier can retry
/// with a reason, but the close never silently records an unexplained
/// shortfall.
#[test]
fn cash_shift_close_with_non_zero_variance_requires_a_reason() {
    let mut db = open_seeded();
    let new_shift = NewCashShift {
        id: "shift-1".to_string(),
        outlet_id: support::OUTLET_ID.to_string(),
        device_id: support::DEVICE_ID.to_string(),
        cashier_user_id: support::USER_ID.to_string(),
        opened_at: "2026-08-14T09:00:00Z".to_string(),
        opening_cash_paise: 300_000,
        business_date: "2026-08-14".to_string(),
        created_at: "2026-08-14T09:00:00Z".to_string(),
        updated_at: "2026-08-14T09:00:00Z".to_string(),
    };
    db.open_cash_shift_with_outbox(new_shift, "mv-open-1", &shift_meta("shift-out-1"))
        .expect("open shift");

    // A cash payment against this shift, via the payment API, to shift
    // expected cash away from the opening float alone.
    let mut cash_payment = forward("pay-cash-1", 70_000);
    cash_payment.method = "CASH".to_string();
    cash_payment.cash_shift_id = Some("shift-1".to_string());
    cash_payment.tendered_paise = Some(70_000);
    cash_payment.change_paise = Some(0);
    db.record_payment_with_outbox(cash_payment, "mv-sale-1", "a-x", None, &pay_meta("pay-out-1"))
        .expect("cash payment");
    // Expected cash is now 300_000 (opening) + 70_000 (sale) = 370_000.

    let bad_close = CloseCashShiftRequest {
        cash_shift_id: "shift-1".to_string(),
        actual_cash_paise: 365_000, // short of expected
        closed_at: "2026-08-14T20:00:00Z".to_string(),
        updated_at: "2026-08-14T20:00:00Z".to_string(),
        variance_reason: None,
    };
    let err = db
        .close_cash_shift_with_outbox(bad_close, &shift_meta("shift-out-2"))
        .expect_err("a non-zero variance close without a reason must be rejected");
    match err {
        DbError::CashVarianceReasonRequired { variance_paise, expected_paise, actual_paise, .. } => {
            assert_eq!(expected_paise, 370_000);
            assert_eq!(actual_paise, 365_000);
            assert_eq!(variance_paise, -5_000);
        }
        other => panic!("expected CashVarianceReasonRequired, got {other}"),
    }

    let still_open = db.get_cash_shift("shift-1").expect("read").expect("exists");
    assert_eq!(still_open.status, "OPEN", "a rejected close leaves the shift OPEN");

    let good_close = CloseCashShiftRequest {
        cash_shift_id: "shift-1".to_string(),
        actual_cash_paise: 365_000,
        closed_at: "2026-08-14T20:05:00Z".to_string(),
        updated_at: "2026-08-14T20:05:00Z".to_string(),
        variance_reason: Some("float miscounted at open".to_string()),
    };
    let (closed, _) = db
        .close_cash_shift_with_outbox(good_close, &shift_meta("shift-out-3"))
        .expect("close with a reason succeeds");
    assert_eq!(closed.status, "CLOSED");
    assert_eq!(closed.variance_paise, Some(-5_000));
}

/// A tiny deterministic PRNG (xorshift32), the `invoice_split_conservation`
/// precedent — this crate takes no `proptest`/`quickcheck` dependency, so
/// "property-style over generated sequences" is hand-rolled and
/// reproducible rather than pulled in for one test file.
struct Xorshift32(u32);
impl Xorshift32 {
    fn new(seed: u32) -> Self {
        Xorshift32(seed | 1)
    }
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    fn range1(&mut self, max: i64) -> i64 {
        1 + (self.next_u32() as i64) % max
    }
}

/// Property: for many randomly generated (forward amount, sequence of
/// partial reversals that sum to no more than the forward amount)
/// scenarios, the settled total (Σ every stored row's `amount_paise`)
/// always equals forward + Σ(reversals) exactly, and the LAST reversal that
/// exactly zeroes the remainder always succeeds while one more paisa of
/// reversal after that is always rejected.
#[test]
fn settled_total_always_equals_forward_plus_reversals_across_many_generated_sequences() {
    for seed in 0..25u32 {
        let mut rng = Xorshift32::new(seed * 131 + 7);
        let mut db = open_seeded();

        let forward_amount = rng.range1(50_000) * 100; // whole rupees, up to 500,000.00
        db.record_payment_with_outbox(forward("pay-fwd", forward_amount), "mv-fwd", "a-x", None, &pay_meta("out-fwd"))
            .expect("forward payment");

        let n_reversals = rng.range1(5);
        let mut remaining = forward_amount;
        let mut reversal_ids = Vec::new();
        for i in 0..n_reversals {
            if remaining <= 0 {
                break;
            }
            // Reverse a random slice of what remains, sometimes the exact
            // remainder to exercise the zeroing case.
            let take = if i == n_reversals - 1 { remaining } else { rng.range1(remaining) };
            let id = format!("pay-rev-{seed}-{i}");
            db.record_payment_with_outbox(
                reversal(&id, "pay-fwd", -take),
                &format!("mv-rev-{seed}-{i}"),
                "a-x",
                None,
                &pay_meta(&format!("out-rev-{seed}-{i}")),
            )
            .unwrap_or_else(|e| panic!("seed {seed}: reversal {i} of {take} (remaining {remaining}) failed: {e}"));
            remaining -= take;
            reversal_ids.push(id);
        }

        let all = db.list_payments_for_order("order-1").expect("list");
        let settled: i64 = all.iter().map(|p| p.amount_paise).sum();
        assert_eq!(
            settled, remaining,
            "seed {seed}: settled total must equal what the property loop computed as remaining"
        );
        assert_eq!(all.len(), 1 + reversal_ids.len());

        // One paisa more than what remains must always be rejected, whatever
        // is left (including exactly zero).
        let over_id = format!("pay-over-{seed}");
        let err = db
            .record_payment_with_outbox(
                reversal(&over_id, "pay-fwd", -(remaining + 1)),
                &format!("mv-over-{seed}"),
                "a-x",
                None,
                &pay_meta(&format!("out-over-{seed}")),
            )
            .expect_err("one paisa beyond what remains must always be rejected");
        if remaining == 0 {
            assert!(matches!(err, DbError::PaymentAlreadyFullyReversed { .. }));
        } else {
            assert!(matches!(err, DbError::ReversalExceedsRemaining { .. }));
        }
    }
}
