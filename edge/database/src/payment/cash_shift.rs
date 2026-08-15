//! `cash_shift` — cashier register open/close with mandatory variance
//! reason (ADR-016 §1, docs/spec/payments.md §39). Unlike `payment`, a
//! shift is a workflow row with exactly one legal in-place transition
//! (OPEN -> CLOSED); [`close_cash_shift`] is the ONLY place this crate
//! updates a `cash_shift` row after insert.

use rusqlite::Transaction;

use crate::error::{DbError, DbResult};
use crate::model::{
    CashMovement, CashShift, CashShiftOutboxMeta, CloseCashShiftRequest, NewCashMovement,
    NewCashShift, PaidInOutRequest,
};
use crate::repo;

/// Opens a new shift plus its `OPENING_FLOAT` `cash_movement` and its
/// `CashShiftOpened` `local_outbox` row, all inside `tx`. Rejects with
/// [`DbError::CashShiftAlreadyOpen`] if the cashier already has one open on
/// this device — `idx_cash_shift_open_device_cashier`'s own rule, checked
/// here first so the caller gets a specific message (§64) rather than a
/// raw unique-constraint failure.
pub(crate) fn open_cash_shift(
    tx: &Transaction,
    new_shift: NewCashShift,
    opening_movement_id: &str,
    outbox_meta: &CashShiftOutboxMeta,
) -> DbResult<(CashShift, Vec<CashMovement>)> {
    if let Some(existing_id) = repo::count_open_cash_shifts_for_device_cashier(
        tx,
        &new_shift.device_id,
        &new_shift.cashier_user_id,
    )? {
        return Err(DbError::CashShiftAlreadyOpen {
            device_id: new_shift.device_id.clone(),
            cashier_user_id: new_shift.cashier_user_id.clone(),
            existing_shift_id: existing_id,
        });
    }

    repo::insert_cash_shift(tx, &new_shift)?;
    let stored =
        repo::get_cash_shift_in_tx(tx, &new_shift.id)?.expect("just inserted this exact row above");

    let opening_movement = NewCashMovement {
        id: opening_movement_id.to_string(),
        cash_shift_id: stored.id.clone(),
        kind: "OPENING_FLOAT".to_string(),
        amount_paise: stored.opening_cash_paise,
        reason: None,
        payment_id: None,
        created_by_user_id: new_shift.cashier_user_id.clone(),
        created_at: new_shift.opened_at.clone(),
    };
    repo::insert_cash_movement(tx, &opening_movement)?;
    let movements = repo::list_cash_movements_for_shift_in_tx(tx, &stored.id)?;

    repo::insert_cash_shift_opened_outbox(tx, &stored, &movements, outbox_meta)?;

    Ok((stored, movements))
}

/// Closes an open shift (§39): derives `expected_cash_paise` from the
/// shift's own posted `cash_movement` rows (never from a caller-supplied
/// total), computes `variance_paise = actual - expected`, and REJECTS the
/// close outright — no write at all — if the variance is non-zero and
/// `req.variance_reason` is `None` or whitespace-only. A zero variance
/// needs no reason.
pub(crate) fn close_cash_shift(
    tx: &Transaction,
    req: CloseCashShiftRequest,
    outbox_meta: &CashShiftOutboxMeta,
) -> DbResult<(CashShift, Vec<CashMovement>)> {
    let existing = repo::get_cash_shift_in_tx(tx, &req.cash_shift_id)?.ok_or_else(|| {
        DbError::CashShiftNotOpen {
            cash_shift_id: req.cash_shift_id.clone(),
            status: "NOT_FOUND".to_string(),
        }
    })?;
    if existing.status != "OPEN" {
        return Err(DbError::CashShiftNotOpen {
            cash_shift_id: req.cash_shift_id.clone(),
            status: existing.status,
        });
    }

    let expected_cash_paise = repo::sum_cash_movements_for_shift_in_tx(tx, &req.cash_shift_id)?;
    let variance_paise = req.actual_cash_paise - expected_cash_paise;

    if variance_paise != 0 {
        let blank = req
            .variance_reason
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty();
        if blank {
            return Err(DbError::CashVarianceReasonRequired {
                cash_shift_id: req.cash_shift_id.clone(),
                expected_paise: expected_cash_paise,
                actual_paise: req.actual_cash_paise,
                variance_paise,
            });
        }
    }
    // A zero variance closes cleanly even if the caller supplied a reason
    // anyway (harmless) — the CHECK this mirrors only requires a reason
    // when the variance is non-zero, never the other way round.
    let variance_reason = if variance_paise == 0 {
        None
    } else {
        req.variance_reason.as_deref()
    };

    let affected = repo::close_cash_shift_in_tx(
        tx,
        &req.cash_shift_id,
        &req.closed_at,
        expected_cash_paise,
        req.actual_cash_paise,
        variance_paise,
        variance_reason,
        &req.updated_at,
    )?;
    if affected != 1 {
        // The `status == OPEN` check above already ran inside this same
        // transaction, so this can only mean a logic error in this
        // function, not a legitimate race (SQLite serializes writers).
        return Err(DbError::CashShiftNotOpen {
            cash_shift_id: req.cash_shift_id.clone(),
            status: "OPEN".to_string(),
        });
    }

    let stored = repo::get_cash_shift_in_tx(tx, &req.cash_shift_id)?
        .expect("just updated this exact row above");
    let movements = repo::list_cash_movements_for_shift_in_tx(tx, &stored.id)?;

    repo::insert_cash_shift_closed_outbox(tx, &stored, &movements, outbox_meta)?;

    Ok((stored, movements))
}

/// Posts a `PAID_IN`/`PAID_OUT` cash movement against an OPEN shift (§39).
/// Rejects a blank reason before writing — mirrors the `cash_movement`
/// table's own `CHECK`. Not its own `AggregateType`/event: `cash_movement`
/// travels only inside the `CashShiftClosed`/`CashShiftOpened` payload's
/// `movements` array, the same "child row, not an aggregate" shape
/// `payment_allocation` and `invoice_line` already use.
pub(crate) fn record_paid_in_out(
    tx: &Transaction,
    req: PaidInOutRequest,
) -> DbResult<CashMovement> {
    if req.kind != "PAID_IN" && req.kind != "PAID_OUT" {
        return Err(DbError::InvalidInput(format!(
            "record_paid_in_out only accepts PAID_IN or PAID_OUT, got {}",
            req.kind
        )));
    }
    if req.reason.trim().is_empty() {
        return Err(DbError::CashMovementReasonRequired {
            kind: req.kind.clone(),
        });
    }
    let shift = repo::get_cash_shift_in_tx(tx, &req.cash_shift_id)?.ok_or_else(|| {
        DbError::CashShiftNotOpen {
            cash_shift_id: req.cash_shift_id.clone(),
            status: "NOT_FOUND".to_string(),
        }
    })?;
    if shift.status != "OPEN" {
        return Err(DbError::CashShiftNotOpen {
            cash_shift_id: req.cash_shift_id.clone(),
            status: shift.status,
        });
    }

    // PAID_OUT is a cash outflow; the caller-supplied amount is a positive
    // magnitude in both cases, signed here so the shift's expected-cash sum
    // never needs to know which kind it summed.
    let signed_amount = if req.kind == "PAID_OUT" {
        -req.amount_paise.abs()
    } else {
        req.amount_paise.abs()
    };

    let movement = NewCashMovement {
        id: req.id.clone(),
        cash_shift_id: req.cash_shift_id.clone(),
        kind: req.kind.clone(),
        amount_paise: signed_amount,
        reason: Some(req.reason.clone()),
        payment_id: None,
        created_by_user_id: req.created_by_user_id.clone(),
        created_at: req.created_at.clone(),
    };
    repo::insert_cash_movement(tx, &movement)?;

    Ok(CashMovement {
        id: movement.id,
        cash_shift_id: movement.cash_shift_id,
        kind: movement.kind,
        amount_paise: movement.amount_paise,
        reason: movement.reason,
        payment_id: movement.payment_id,
        created_by_user_id: movement.created_by_user_id,
        created_at: movement.created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AppUser, Device, Outlet};
    use crate::Db;

    /// Seeds `outlet-1`/`device-1`/`user-1` — the FK targets
    /// `cash_shift.outlet_id`/`device_id`/`cashier_user_id` require.
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
    }

    fn open_db() -> Db {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_deps(&mut db);
        db
    }

    fn new_shift(id: &str) -> NewCashShift {
        NewCashShift {
            id: id.to_string(),
            outlet_id: "outlet-1".to_string(),
            device_id: "device-1".to_string(),
            cashier_user_id: "user-1".to_string(),
            opened_at: "2026-08-14T09:00:00Z".to_string(),
            opening_cash_paise: 500_000,
            business_date: "2026-08-14".to_string(),
            created_at: "2026-08-14T09:00:00Z".to_string(),
            updated_at: "2026-08-14T09:00:00Z".to_string(),
        }
    }

    fn open_meta(id: &str) -> CashShiftOutboxMeta {
        CashShiftOutboxMeta {
            outbox_id: id.to_string(),
            occurred_at: "2026-08-14T09:00:00Z".to_string(),
        }
    }

    #[test]
    fn opening_a_second_shift_for_the_same_cashier_device_is_rejected() {
        let mut db = open_db();
        let tx = db.connection_mut().transaction().expect("tx");
        open_cash_shift(&tx, new_shift("shift-1"), "mv-1", &open_meta("out-1"))
            .expect("first open");

        let err = open_cash_shift(&tx, new_shift("shift-2"), "mv-2", &open_meta("out-2"))
            .expect_err("a second open shift for the same cashier/device must be rejected");
        assert!(matches!(err, DbError::CashShiftAlreadyOpen { .. }));
    }

    #[test]
    fn zero_variance_close_needs_no_reason() {
        let mut db = open_db();
        let tx = db.connection_mut().transaction().expect("tx");
        open_cash_shift(&tx, new_shift("shift-1"), "mv-1", &open_meta("out-1")).expect("open");

        let req = CloseCashShiftRequest {
            cash_shift_id: "shift-1".to_string(),
            actual_cash_paise: 500_000, // exactly the opening float, no sales
            closed_at: "2026-08-14T18:00:00Z".to_string(),
            updated_at: "2026-08-14T18:00:00Z".to_string(),
            variance_reason: None,
        };
        let (stored, _movements) =
            close_cash_shift(&tx, req, &open_meta("out-close-1")).expect("zero-variance close");
        assert_eq!(stored.status, "CLOSED");
        assert_eq!(stored.variance_paise, Some(0));
        assert_eq!(stored.variance_reason, None);
    }

    #[test]
    fn non_zero_variance_close_without_a_reason_is_rejected() {
        let mut db = open_db();
        let tx = db.connection_mut().transaction().expect("tx");
        open_cash_shift(&tx, new_shift("shift-1"), "mv-1", &open_meta("out-1")).expect("open");

        let req = CloseCashShiftRequest {
            cash_shift_id: "shift-1".to_string(),
            actual_cash_paise: 505_000, // 50 rupees over expected
            closed_at: "2026-08-14T18:00:00Z".to_string(),
            updated_at: "2026-08-14T18:00:00Z".to_string(),
            variance_reason: None,
        };
        let err = close_cash_shift(&tx, req, &open_meta("out-close-1"))
            .expect_err("a non-zero variance close without a reason must be rejected");
        match err {
            DbError::CashVarianceReasonRequired { variance_paise, .. } => {
                assert_eq!(variance_paise, 5_000);
            }
            other => panic!("expected CashVarianceReasonRequired, got {other}"),
        }

        // Rejected means rejected: the shift must still be OPEN afterward.
        let still = repo::get_cash_shift_in_tx(&tx, "shift-1")
            .expect("read")
            .expect("shift exists");
        assert_eq!(still.status, "OPEN");
    }

    #[test]
    fn whitespace_only_reason_is_treated_as_blank() {
        let mut db = open_db();
        let tx = db.connection_mut().transaction().expect("tx");
        open_cash_shift(&tx, new_shift("shift-1"), "mv-1", &open_meta("out-1")).expect("open");

        let req = CloseCashShiftRequest {
            cash_shift_id: "shift-1".to_string(),
            actual_cash_paise: 490_000, // 100 rupees short
            closed_at: "2026-08-14T18:00:00Z".to_string(),
            updated_at: "2026-08-14T18:00:00Z".to_string(),
            variance_reason: Some("   ".to_string()),
        };
        let err = close_cash_shift(&tx, req, &open_meta("out-close-1"))
            .expect_err("whitespace-only reason must be rejected as blank");
        assert!(matches!(err, DbError::CashVarianceReasonRequired { .. }));
    }

    #[test]
    fn non_zero_variance_close_with_a_reason_succeeds() {
        let mut db = open_db();
        let tx = db.connection_mut().transaction().expect("tx");
        open_cash_shift(&tx, new_shift("shift-1"), "mv-1", &open_meta("out-1")).expect("open");

        let req = CloseCashShiftRequest {
            cash_shift_id: "shift-1".to_string(),
            actual_cash_paise: 490_000, // 100 rupees short
            closed_at: "2026-08-14T18:00:00Z".to_string(),
            updated_at: "2026-08-14T18:00:00Z".to_string(),
            variance_reason: Some("counted twice, till was short a hundred".to_string()),
        };
        let (stored, _) =
            close_cash_shift(&tx, req, &open_meta("out-close-1")).expect("close with reason");
        assert_eq!(stored.status, "CLOSED");
        assert_eq!(stored.variance_paise, Some(-10_000));
        assert_eq!(
            stored.variance_reason.as_deref(),
            Some("counted twice, till was short a hundred")
        );
    }

    #[test]
    fn closing_an_already_closed_shift_is_rejected() {
        let mut db = open_db();
        let tx = db.connection_mut().transaction().expect("tx");
        open_cash_shift(&tx, new_shift("shift-1"), "mv-1", &open_meta("out-1")).expect("open");
        let req = CloseCashShiftRequest {
            cash_shift_id: "shift-1".to_string(),
            actual_cash_paise: 500_000,
            closed_at: "2026-08-14T18:00:00Z".to_string(),
            updated_at: "2026-08-14T18:00:00Z".to_string(),
            variance_reason: None,
        };
        close_cash_shift(&tx, req.clone(), &open_meta("out-close-1")).expect("first close");

        let err = close_cash_shift(&tx, req, &open_meta("out-close-2"))
            .expect_err("closing an already-closed shift must be rejected");
        assert!(matches!(err, DbError::CashShiftNotOpen { .. }));
    }

    #[test]
    fn paid_out_requires_a_reason_and_shrinks_expected_cash() {
        let mut db = open_db();
        let tx = db.connection_mut().transaction().expect("tx");
        open_cash_shift(&tx, new_shift("shift-1"), "mv-1", &open_meta("out-1")).expect("open");

        let blank_err = record_paid_in_out(
            &tx,
            PaidInOutRequest {
                id: "pio-1".to_string(),
                cash_shift_id: "shift-1".to_string(),
                kind: "PAID_OUT".to_string(),
                amount_paise: 50_000,
                reason: "  ".to_string(),
                created_by_user_id: "user-1".to_string(),
                created_at: "2026-08-14T12:00:00Z".to_string(),
            },
        )
        .expect_err("blank reason must be rejected");
        assert!(matches!(
            blank_err,
            DbError::CashMovementReasonRequired { .. }
        ));

        let movement = record_paid_in_out(
            &tx,
            PaidInOutRequest {
                id: "pio-2".to_string(),
                cash_shift_id: "shift-1".to_string(),
                kind: "PAID_OUT".to_string(),
                amount_paise: 50_000,
                reason: "vegetable supplier, cash on delivery".to_string(),
                created_by_user_id: "user-1".to_string(),
                created_at: "2026-08-14T12:00:00Z".to_string(),
            },
        )
        .expect("paid out with a reason");
        assert_eq!(movement.amount_paise, -50_000);

        let expected = repo::sum_cash_movements_for_shift_in_tx(&tx, "shift-1").expect("sum");
        assert_eq!(expected, 500_000 - 50_000);
    }

    // ------------------------------------------------------- T9 retry: --
    // ---------------------------------- open-shift recovery (Defect 2) --

    /// The query a POS restart uses to recover an orphaned open shift —
    /// through the public `Db` API (mirrors `open_cash_shift_with_outbox`,
    /// which commits internally, rather than this file's `&tx`-scoped
    /// helpers which never commit).
    #[test]
    fn find_open_cash_shift_recovers_the_open_shift_for_device_and_cashier() {
        use crate::model::CashShiftOutboxMeta;

        let mut db = open_db();
        assert!(
            db.find_open_cash_shift("device-1", "user-1")
                .expect("query")
                .is_none(),
            "no shift open yet"
        );

        let (opened, _movements) = db
            .open_cash_shift_with_outbox(
                new_shift("shift-1"),
                "mv-1",
                &CashShiftOutboxMeta {
                    outbox_id: "out-1".to_string(),
                    occurred_at: "2026-08-14T09:00:00Z".to_string(),
                },
            )
            .expect("open shift");

        let recovered = db
            .find_open_cash_shift("device-1", "user-1")
            .expect("query")
            .expect("the just-opened shift must be found");
        assert_eq!(recovered.id, opened.id);
        assert_eq!(recovered.status, "OPEN");

        // A different device/cashier pair finds nothing.
        assert!(db
            .find_open_cash_shift("device-1", "user-2")
            .expect("query")
            .is_none());
        assert!(db
            .find_open_cash_shift("device-2", "user-1")
            .expect("query")
            .is_none());
    }

    /// Once closed, the shift is no longer found by
    /// [`crate::Db::find_open_cash_shift`] — recovery only ever surfaces a
    /// shift the cashier can still act on.
    #[test]
    fn find_open_cash_shift_does_not_return_a_closed_shift() {
        use crate::model::{CashShiftOutboxMeta, CloseCashShiftRequest};

        let mut db = open_db();
        db.open_cash_shift_with_outbox(
            new_shift("shift-1"),
            "mv-1",
            &CashShiftOutboxMeta {
                outbox_id: "out-1".to_string(),
                occurred_at: "2026-08-14T09:00:00Z".to_string(),
            },
        )
        .expect("open shift");

        db.close_cash_shift_with_outbox(
            CloseCashShiftRequest {
                cash_shift_id: "shift-1".to_string(),
                actual_cash_paise: 500_000,
                closed_at: "2026-08-14T18:00:00Z".to_string(),
                updated_at: "2026-08-14T18:00:00Z".to_string(),
                variance_reason: None,
            },
            &CashShiftOutboxMeta {
                outbox_id: "out-2".to_string(),
                occurred_at: "2026-08-14T18:00:00Z".to_string(),
            },
        )
        .expect("close shift");

        assert!(
            db.find_open_cash_shift("device-1", "user-1")
                .expect("query")
                .is_none(),
            "a closed shift must not be recoverable as \"open\""
        );
    }
}
