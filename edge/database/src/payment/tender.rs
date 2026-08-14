//! `payment` — append-only tender recording (ADR-016 §1, docs/spec/payments.md
//! §Conflict policy). Every money field here is a plain `i64` paise value
//! supplied by the caller (this module does no tax/pricing computation of
//! its own, unlike `crate::invoice`) — its job is enforcing the append-only
//! shape, not deriving amounts.

use rusqlite::Transaction;

use crate::error::{DbError, DbResult};
use crate::model::{CashMovement, NewCashMovement, NewPayment, Payment, PaymentOutboxMeta};
use crate::repo;

/// Rejects a forward tender (`reverses_payment_id == None`) whose
/// `amount_paise` is not strictly positive. Checked BEFORE any write, so a
/// caller gets [`DbError::ForwardPaymentAmountNotPositive`] rather than a
/// generic SQLite constraint failure.
fn validate_forward(new_payment: &NewPayment) -> DbResult<()> {
    if new_payment.amount_paise <= 0 {
        return Err(DbError::ForwardPaymentAmountNotPositive {
            amount_paise: new_payment.amount_paise,
        });
    }
    Ok(())
}

/// Rejects a reversal (`reverses_payment_id == Some(_)`) whose shape is
/// invalid: a positive amount, a missing original, an original that is
/// already fully reversed, or a reversal that would exceed what remains to
/// be reversed. Checked BEFORE any write — same discipline as
/// [`crate::invoice::assemble::validate_conservation`].
fn validate_reversal(
    tx: &Transaction,
    new_payment: &NewPayment,
    reverses_payment_id: &str,
) -> DbResult<()> {
    if new_payment.amount_paise > 0 {
        return Err(DbError::ReversalAmountNotNonPositive {
            reverses_payment_id: reverses_payment_id.to_string(),
            amount_paise: new_payment.amount_paise,
        });
    }

    let original = repo::get_payment_in_tx(tx, reverses_payment_id)?.ok_or_else(|| {
        DbError::ReversedPaymentNotFound {
            payment_id: reverses_payment_id.to_string(),
        }
    })?;

    let existing_reversals = repo::list_reversals_for_payment_in_tx(tx, reverses_payment_id)?;
    let already_reversed: i64 = existing_reversals.iter().map(|r| r.amount_paise).sum();
    // `original.amount_paise` is positive (every forward row is), and every
    // reversal so far is non-positive, so `remaining` monotonically shrinks
    // toward zero as reversals accumulate — never negative unless a bug
    // upstream already let one through, which the exceeds-remaining check
    // below still catches.
    let remaining = original.amount_paise + already_reversed;

    if remaining <= 0 {
        return Err(DbError::PaymentAlreadyFullyReversed {
            payment_id: reverses_payment_id.to_string(),
        });
    }

    let requested = new_payment.amount_paise.abs();
    if requested > remaining {
        return Err(DbError::ReversalExceedsRemaining {
            payment_id: reverses_payment_id.to_string(),
            requested_paise: requested,
            remaining_paise: remaining,
        });
    }

    Ok(())
}

/// The `cash_movement` this tender produces against its shift, if any.
/// `None` for a non-cash tender or a cash tender not tied to an open shift
/// (`cash_shift_id == None`, e.g. a manual cash sale recorded before a
/// shift is opened — legal at the schema layer, just invisible to the
/// drawer trail). A forward tender posts `CASH_SALE`; a reversal posts
/// `CASH_REFUND` — both already signed correctly by `amount_paise` itself
/// (positive / non-positive respectively), so no re-signing happens here.
fn cash_movement_for(new_payment: &NewPayment, movement_id: &str) -> Option<NewCashMovement> {
    if new_payment.method != "CASH" {
        return None;
    }
    let cash_shift_id = new_payment.cash_shift_id.clone()?;
    let kind = if new_payment.reverses_payment_id.is_some() {
        "CASH_REFUND"
    } else {
        "CASH_SALE"
    };
    Some(NewCashMovement {
        id: movement_id.to_string(),
        cash_shift_id,
        kind: kind.to_string(),
        amount_paise: new_payment.amount_paise,
        reason: None,
        payment_id: None, // stamped by the caller once the payment id is known; see record_payment
        created_by_user_id: new_payment.created_by_user_id.clone(),
        created_at: new_payment.created_at.clone(),
    })
}

/// Records ONE tender — forward or reversal, decided by
/// `new_payment.reverses_payment_id` — plus its `local_outbox` row and (for
/// a CASH tender tied to an open shift) its `cash_movement` row, all inside
/// `tx`. This is the ONLY writer of the `payment` table in this crate: it
/// never issues an UPDATE or DELETE against it, matching every append-only
/// guarantee `docs/spec/payments.md` requires.
///
/// `cash_movement_id` is caller-supplied (UUIDv7) and only consumed when
/// this tender actually produces a movement (CASH + an open shift) — the
/// same "caller mints every id, this crate decides whether it is used"
/// shape [`crate::invoice::assemble::persist_invoice`] uses for line ids.
pub(crate) fn record_payment(
    tx: &Transaction,
    new_payment: NewPayment,
    cash_movement_id: &str,
    outbox_meta: &PaymentOutboxMeta,
) -> DbResult<(Payment, Option<CashMovement>)> {
    match &new_payment.reverses_payment_id {
        None => validate_forward(&new_payment)?,
        Some(original_id) => validate_reversal(tx, &new_payment, original_id)?,
    }

    let movement_request = cash_movement_for(&new_payment, cash_movement_id);

    repo::insert_payment(tx, &new_payment)?;
    let stored = repo::get_payment_in_tx(tx, &new_payment.id)?
        .expect("just inserted this exact row above");

    let stored_movement = if let Some(mut m) = movement_request {
        m.payment_id = Some(stored.id.clone());
        repo::insert_cash_movement(tx, &m)?;
        Some(CashMovement {
            id: m.id,
            cash_shift_id: m.cash_shift_id,
            kind: m.kind,
            amount_paise: m.amount_paise,
            reason: m.reason,
            payment_id: m.payment_id,
            created_by_user_id: m.created_by_user_id,
            created_at: m.created_at,
        })
    } else {
        None
    };

    match &stored.reverses_payment_id {
        None => repo::insert_payment_received_outbox(tx, &stored, outbox_meta)?,
        Some(original_id) => {
            repo::insert_payment_refunded_outbox(tx, &stored, original_id, outbox_meta)?
        }
    }

    Ok((stored, stored_movement))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AppUser, Device, NewCashShift, NewOrder, Outlet};
    use crate::Db;

    /// Seeds `outlet-1` / `device-1` / `user-1` / `order-1` — the FK targets
    /// `payment.outlet_id`/`order_id` and `cash_shift.outlet_id`/`device_id`/
    /// `cashier_user_id` require, matching `tests/support/mod.rs`'s `seed`
    /// but scoped to this file's unit tests (no menu/billing-config needed
    /// here, unlike the invoice tests).
    fn seed_deps(db: &mut Db) {
        repo::upsert_outlet(
            db.connection(),
            &Outlet {
                id: "outlet-1".to_string(),
                brand_id: "brand-1".to_string(),
                name: "Pune".to_string(),
                timezone: "Asia/Kolkata".to_string(),
                config_version: 1,
                created_at: "2026-08-01T00:00:00Z".to_string(),
                updated_at: "2026-08-01T00:00:00Z".to_string(),
            },
        )
        .expect("seed outlet");
        repo::upsert_device(
            db.connection(),
            &Device {
                id: "device-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                kind: "POS".to_string(),
                name: "Till 1".to_string(),
                last_seen_at: None,
                created_at: "2026-08-01T00:00:00Z".to_string(),
            },
        )
        .expect("seed device");
        repo::replace_app_user(
            db.connection(),
            &AppUser {
                id: "user-1".to_string(),
                tenant_id: "tenant-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                email: "cashier@example.in".to_string(),
                full_name: "Cashier".to_string(),
                password_hash: "argon2id$dummy".to_string(),
                pin_hash: None,
                is_active: true,
                permissions_json: "[]".to_string(),
                config_version: 1,
                updated_at: "2026-08-01T00:00:00Z".to_string(),
            },
        )
        .expect("seed app_user");

        let tx = db.connection_mut().transaction().expect("seed tx");
        repo::insert_order(
            &tx,
            &NewOrder {
                id: "order-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                device_id: "device-1".to_string(),
                order_type: "DINE_IN".to_string(),
                status: "CONFIRMED".to_string(),
                table_id: None,
                subtotal_paise: 1000,
                discount_paise: 0,
                taxes_paise: 0,
                total_paise: 1000,
                source: "POS".to_string(),
                external_order_id: None,
                payment_status: "UNPAID".to_string(),
                payment_source: None,
                confirmed_at: None,
                source_payload_json: None,
                schema_version: 1,
                created_at: "2026-08-14T09:00:00Z".to_string(),
                updated_at: "2026-08-14T09:00:00Z".to_string(),
            },
        )
        .expect("seed order");
        tx.commit().expect("commit seed tx");
    }

    fn base_payment(id: &str, amount_paise: i64) -> NewPayment {
        NewPayment {
            id: id.to_string(),
            outlet_id: "outlet-1".to_string(),
            order_id: "order-1".to_string(),
            cash_shift_id: None,
            method: "CASH".to_string(),
            status: "CAPTURED".to_string(),
            amount_paise,
            tendered_paise: Some(amount_paise.max(0)),
            change_paise: Some(0),
            reference: None,
            external_id: None,
            reverses_payment_id: None,
            captured_at: Some("2026-08-14T10:00:00Z".to_string()),
            created_by_user_id: "user-1".to_string(),
            created_at: "2026-08-14T10:00:00Z".to_string(),
            updated_at: "2026-08-14T10:00:00Z".to_string(),
        }
    }

    fn meta(id: &str) -> PaymentOutboxMeta {
        PaymentOutboxMeta {
            outbox_id: id.to_string(),
            occurred_at: "2026-08-14T10:00:00Z".to_string(),
        }
    }

    fn open_db() -> Db {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_deps(&mut db);
        db
    }

    #[test]
    fn a_forward_payment_with_zero_amount_is_rejected() {
        let mut db = open_db();
        let tx = db.connection_mut().transaction().expect("tx");
        let err = record_payment(&tx, base_payment("p-1", 0), "m-1", &meta("o-1"))
            .expect_err("zero-amount forward payment must be rejected");
        assert!(matches!(err, DbError::ForwardPaymentAmountNotPositive { amount_paise: 0 }));
    }

    #[test]
    fn a_forward_payment_with_negative_amount_is_rejected() {
        let mut db = open_db();
        let tx = db.connection_mut().transaction().expect("tx");
        let err = record_payment(&tx, base_payment("p-1", -500), "m-1", &meta("o-1"))
            .expect_err("negative forward payment must be rejected");
        assert!(matches!(err, DbError::ForwardPaymentAmountNotPositive { amount_paise: -500 }));
    }

    #[test]
    fn a_reversal_with_positive_amount_is_rejected() {
        let mut db = open_db();
        let tx = db.connection_mut().transaction().expect("tx");
        record_payment(&tx, base_payment("p-1", 1000), "m-1", &meta("o-1")).expect("forward");

        let mut reversal = base_payment("p-2", 500);
        reversal.reverses_payment_id = Some("p-1".to_string());
        let err = record_payment(&tx, reversal, "m-2", &meta("o-2"))
            .expect_err("positive-amount reversal must be rejected");
        assert!(matches!(err, DbError::ReversalAmountNotNonPositive { .. }));
    }

    #[test]
    fn a_reversal_without_a_real_original_is_rejected() {
        let mut db = open_db();
        let tx = db.connection_mut().transaction().expect("tx");
        let mut reversal = base_payment("p-2", -500);
        reversal.reverses_payment_id = Some("does-not-exist".to_string());
        let err = record_payment(&tx, reversal, "m-2", &meta("o-2"))
            .expect_err("a reversal against a nonexistent payment must be rejected");
        assert!(matches!(err, DbError::ReversedPaymentNotFound { .. }));
    }

    #[test]
    fn a_full_reversal_then_settles_to_zero_and_a_second_reversal_is_rejected() {
        let mut db = open_db();
        let tx = db.connection_mut().transaction().expect("tx");
        record_payment(&tx, base_payment("p-1", 1000), "m-1", &meta("o-1")).expect("forward");

        let mut reversal = base_payment("p-2", -1000);
        reversal.reverses_payment_id = Some("p-1".to_string());
        record_payment(&tx, reversal, "m-2", &meta("o-2")).expect("full reversal");

        let mut second = base_payment("p-3", -1);
        second.reverses_payment_id = Some("p-1".to_string());
        let err = record_payment(&tx, second, "m-3", &meta("o-3"))
            .expect_err("reversing an already-fully-reversed payment must be rejected");
        assert!(matches!(err, DbError::PaymentAlreadyFullyReversed { .. }));
    }

    #[test]
    fn a_reversal_larger_than_what_remains_is_rejected() {
        let mut db = open_db();
        let tx = db.connection_mut().transaction().expect("tx");
        record_payment(&tx, base_payment("p-1", 1000), "m-1", &meta("o-1")).expect("forward");

        let mut over = base_payment("p-2", -1001);
        over.reverses_payment_id = Some("p-1".to_string());
        let err = record_payment(&tx, over, "m-2", &meta("o-2"))
            .expect_err("over-refund must be rejected");
        assert!(matches!(err, DbError::ReversalExceedsRemaining { .. }));
    }

    #[test]
    fn a_partial_then_a_matching_second_reversal_settles_exactly_to_zero() {
        let mut db = open_db();
        let tx = db.connection_mut().transaction().expect("tx");
        record_payment(&tx, base_payment("p-1", 1000), "m-1", &meta("o-1")).expect("forward");

        let mut r1 = base_payment("p-2", -400);
        r1.reverses_payment_id = Some("p-1".to_string());
        record_payment(&tx, r1, "m-2", &meta("o-2")).expect("partial reversal 1");

        let mut r2 = base_payment("p-3", -600);
        r2.reverses_payment_id = Some("p-1".to_string());
        record_payment(&tx, r2, "m-3", &meta("o-3")).expect("partial reversal 2 settles to zero");

        let mut r3 = base_payment("p-4", -1);
        r3.reverses_payment_id = Some("p-1".to_string());
        let err = record_payment(&tx, r3, "m-4", &meta("o-4"))
            .expect_err("a third reversal after settling to zero must be rejected");
        assert!(matches!(err, DbError::PaymentAlreadyFullyReversed { .. }));
    }

    #[test]
    fn a_cash_payment_tied_to_a_shift_posts_a_cash_sale_movement() {
        let mut db = open_db();
        let tx = db.connection_mut().transaction().expect("tx");
        repo::insert_cash_shift(
            &tx,
            &NewCashShift {
                id: "shift-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                device_id: "device-1".to_string(),
                cashier_user_id: "user-1".to_string(),
                opened_at: "2026-08-14T09:00:00Z".to_string(),
                opening_cash_paise: 200_000,
                business_date: "2026-08-14".to_string(),
                created_at: "2026-08-14T09:00:00Z".to_string(),
                updated_at: "2026-08-14T09:00:00Z".to_string(),
            },
        )
        .expect("seed shift");

        let mut p = base_payment("p-1", 1500);
        p.cash_shift_id = Some("shift-1".to_string());
        let (_stored, movement) =
            record_payment(&tx, p, "m-1", &meta("o-1")).expect("record cash payment");
        let movement = movement.expect("a CASH payment on an open shift must post a movement");
        assert_eq!(movement.kind, "CASH_SALE");
        assert_eq!(movement.amount_paise, 1500);
        assert_eq!(movement.payment_id.as_deref(), Some("p-1"));
    }

    #[test]
    fn a_non_cash_payment_posts_no_movement() {
        let mut db = open_db();
        let tx = db.connection_mut().transaction().expect("tx");
        let mut p = base_payment("p-1", 1500);
        p.method = "UPI".to_string();
        p.tendered_paise = None;
        p.change_paise = None;
        let (_stored, movement) =
            record_payment(&tx, p, "m-1", &meta("o-1")).expect("record UPI payment");
        assert!(movement.is_none());
    }
}
