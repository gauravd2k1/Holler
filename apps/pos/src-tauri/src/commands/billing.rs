//! Billing commands (docs/spec/payments.md, docs/spec/compliance.md,
//! ADR-016, T9). Every money field returned here is copied verbatim from
//! `holler_edge_database` — this crate resolves the caller-supplied context
//! (which invoice series, which fiscal profile, which cash shift) and mints
//! ids/timestamps, but performs no tax or tender arithmetic of its own
//! (CLAUDE.md: the edge computes, the UI formats). Every write goes through
//! one of `holler_edge_database::Db::issue_invoice_with_outbox`/
//! `record_payment_with_outbox`/`open_cash_shift_with_outbox`/
//! `close_cash_shift_with_outbox`/`record_paid_in_out_with_outbox` — the
//! same "one crate, one transaction, one outbox row" discipline every other
//! command module in this crate follows.
//!
//! Milestone 3 EXCLUDES split bills from this surface (docs/m3-planning.md):
//! `issue_invoice` always bills every line on the order at full quantity
//! (`split_count == 1`), the unsplit case `Db::issue_invoice_with_outbox`
//! itself documents. `Db::issue_split_invoices_with_outbox` exists in the
//! edge crate but has no command here.
//!
//! No dedicated billing permission exists in `packages/contracts`'
//! `PermissionSchema` (ADR-016 0.4.4 addendum records the same gap for the
//! compliance config routes) — the frontend gates these actions on
//! `order.modify` for the forward path (issue/tender/shift open-close) and
//! `order.void` for a reversal (refund/void a tender), the closest existing
//! permissions, not a new claim on the frozen contract.

use holler_edge_database::model::{
    CashShiftOutboxMeta, CloseCashShiftRequest, InvoiceLineShare, InvoiceOutboxMeta,
    IssueInvoiceHeader, NewCashShift, NewPayment, PaidInOutRequest, PaymentOutboxMeta,
};
use holler_edge_database::Db;
use tauri::State;

use crate::dto::{CashMovement, CashShift, Invoice, Payment};
use crate::error::{AppError, AppResult};
use crate::ids::{new_id, now_iso};
use crate::state::AppState;

fn lock_db(state: &AppState) -> AppResult<std::sync::MutexGuard<'_, Db>> {
    state.db.lock().map_err(|_| AppError {
        code: "LOCK_POISONED",
        message: "database lock poisoned".into(),
    })
}

/// Outlet-local business day. Truncates the UTC invoice moment to its date
/// part rather than resolving `outlet.timezone` — the same known limitation
/// already disclosed on `edge/database/src/repo.rs`'s display-number reset
/// bucketing (docs/RESUME.md "Display-number reset buckets by UTC calendar
/// day, not outlet-local business day"), not a new one introduced here.
fn business_date_from(instant_iso: &str) -> String {
    instant_iso.get(0..10).unwrap_or(instant_iso).to_string()
}

fn build_invoice_lines(
    items: &[holler_edge_database::model::OrderItem],
) -> Vec<InvoiceLineShare> {
    items
        .iter()
        .map(|item| InvoiceLineShare {
            id: new_id(),
            order_item_id: item.id.clone(),
            quantity: item.quantity,
            discount_per_unit_paise: 0,
        })
        .collect()
}

/// The `outlet_fiscal_profile` effective for this outlet right now — the
/// latest row whose `effective_from` is not in the future. ISO8601 UTC
/// timestamps with a fixed-width fractional second (`ids::now_iso`) compare
/// correctly as strings, so this needs no date parsing, mirroring
/// `edge/database/src/invoice/assemble.rs`'s `resolve_fiscal_profile` (which
/// is `pub(crate)` to that crate and not reachable from here).
fn resolve_fiscal_profile(
    profiles: &[holler_edge_database::model::OutletFiscalProfile],
    at: &str,
) -> Option<holler_edge_database::model::OutletFiscalProfile> {
    profiles
        .iter()
        .filter(|p| p.effective_from.as_str() <= at)
        .max_by(|a, b| a.effective_from.cmp(&b.effective_from))
        .cloned()
}

// ---------------------------------------------------------------- invoice --

pub fn issue_invoice_impl(
    state: &AppState,
    order_id: &str,
    created_by_user_id: &str,
) -> AppResult<Invoice> {
    let now = now_iso();
    let mut db = lock_db(state)?;

    let order = db.get_order(order_id)?.ok_or_else(|| AppError {
        code: "NOT_FOUND",
        message: format!("order {order_id} not found"),
    })?;
    let items = holler_edge_database::repo::list_order_items(db.connection(), order_id)?;
    if items.is_empty() {
        return Err(AppError {
            code: "NOTHING_TO_BILL",
            message: "this order has no lines to bill".into(),
        });
    }

    let fiscal_profiles = holler_edge_database::repo::list_outlet_fiscal_profiles_for_outlet(
        db.connection(),
        &state.outlet_id,
    )?;
    let fiscal_profile = resolve_fiscal_profile(&fiscal_profiles, &now).ok_or_else(|| AppError {
        code: "NO_FISCAL_PROFILE_CONFIGURED",
        message: format!(
            "no outlet_fiscal_profile is effective for outlet {} yet — billing cannot resolve a \
             GSTIN or address to print",
            state.outlet_id
        ),
    })?;

    let series = holler_edge_database::repo::list_invoice_series_for_outlet(
        db.connection(),
        &state.outlet_id,
    )?
    .into_iter()
    .find(|s| s.is_active && s.code == "SALES")
    .ok_or_else(|| AppError {
        code: "NO_ACTIVE_INVOICE_SERIES",
        message: format!(
            "no active 'SALES' invoice_series is configured for outlet {}",
            state.outlet_id
        ),
    })?;

    let header = IssueInvoiceHeader {
        outlet_id: state.outlet_id.clone(),
        order_id: order.id.clone(),
        series_code: series.code,
        invoice_date: now.clone(),
        business_date: business_date_from(&now),
        customer_name: None,
        customer_phone: None,
        customer_gstin: None,
        place_of_supply_state_code: fiscal_profile.state_code,
        channel: "POS".to_string(),
        tax_liability_party: "RESTAURANT".to_string(),
        eco_operator_name: None,
        eco_operator_gstin: None,
        supply_classification: None,
        created_by_user_id: created_by_user_id.to_string(),
    };

    let lines = build_invoice_lines(&items);
    let invoice_id = new_id();
    let meta = InvoiceOutboxMeta {
        outbox_id: new_id(),
        occurred_at: now,
    };

    let stored = db.issue_invoice_with_outbox(&header, invoice_id, lines, &meta)?;
    let stored_lines = db.list_invoice_lines(&stored.id)?;
    Invoice::from_db(stored, stored_lines).map_err(|e| AppError {
        code: "SERIALIZATION_ERROR",
        message: e.to_string(),
    })
}

pub fn list_invoices_for_order_impl(state: &AppState, order_id: &str) -> AppResult<Vec<Invoice>> {
    let db = lock_db(state)?;
    let invoices = db.list_invoices_for_order(order_id)?;
    let mut out = Vec::with_capacity(invoices.len());
    for invoice in invoices {
        let lines = db.list_invoice_lines(&invoice.id)?;
        out.push(Invoice::from_db(invoice, lines).map_err(|e| AppError {
            code: "SERIALIZATION_ERROR",
            message: e.to_string(),
        })?);
    }
    Ok(out)
}

// ---------------------------------------------------------------- payment --

/// One tender against `order_id`. `method != "CASH"` forces
/// `tendered_paise`/`change_paise` to `None` regardless of what was passed,
/// so the returned row can never violate `PaymentSchema`'s own
/// `.refine()` ("tendered_paise is meaningful only on a CASH tender") — a
/// wire shape the frontend's Zod parse would otherwise reject outright.
///
/// `invoice_id` names which invoice a FORWARD tender settles — required the
/// moment a bill has been issued, so `holler_edge_database` can reject a
/// tender that would exceed the invoice's remaining due (T9 retry, the
/// double-settlement defect: `FORWARD_PAYMENT_EXCEEDS_REMAINING_DUE`). It is
/// ignored for a reversal (`reverses_payment_id.is_some()`), which always
/// derives its own target from the original payment's allocation instead.
#[allow(clippy::too_many_arguments)]
pub fn record_payment_impl(
    state: &AppState,
    order_id: &str,
    method: &str,
    amount_paise: i64,
    tendered_paise: Option<i64>,
    change_paise: Option<i64>,
    reference: Option<String>,
    cash_shift_id: Option<String>,
    reverses_payment_id: Option<String>,
    invoice_id: Option<String>,
    created_by_user_id: &str,
) -> AppResult<Payment> {
    let now = now_iso();
    let is_cash = method == "CASH";

    let new_payment = NewPayment {
        id: new_id(),
        outlet_id: state.outlet_id.clone(),
        order_id: order_id.to_string(),
        cash_shift_id,
        method: method.to_string(),
        status: "CAPTURED".to_string(),
        amount_paise,
        tendered_paise: if is_cash { tendered_paise } else { None },
        change_paise: if is_cash { change_paise } else { None },
        reference,
        external_id: None,
        reverses_payment_id,
        captured_at: Some(now.clone()),
        created_by_user_id: created_by_user_id.to_string(),
        created_at: now.clone(),
        updated_at: now,
    };

    let cash_movement_id = new_id();
    let allocation_id = new_id();
    let meta = PaymentOutboxMeta {
        outbox_id: new_id(),
        occurred_at: new_payment.created_at.clone(),
    };

    let mut db = lock_db(state)?;
    let (stored, _movement) = db.record_payment_with_outbox(
        new_payment,
        &cash_movement_id,
        &allocation_id,
        invoice_id.as_deref(),
        &meta,
    )?;
    Ok(Payment::from(stored))
}

pub fn list_payments_for_order_impl(state: &AppState, order_id: &str) -> AppResult<Vec<Payment>> {
    let db = lock_db(state)?;
    let payments = db.list_payments_for_order(order_id)?;
    Ok(payments.into_iter().map(Payment::from).collect())
}

// -------------------------------------------------------------- cash shift --

pub fn open_cash_shift_impl(
    state: &AppState,
    cashier_user_id: &str,
    opening_cash_paise: i64,
) -> AppResult<CashShift> {
    let now = now_iso();
    let new_shift = NewCashShift {
        id: new_id(),
        outlet_id: state.outlet_id.clone(),
        device_id: state.device_id.clone(),
        cashier_user_id: cashier_user_id.to_string(),
        opened_at: now.clone(),
        opening_cash_paise,
        business_date: business_date_from(&now),
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    let opening_movement_id = new_id();
    let meta = CashShiftOutboxMeta {
        outbox_id: new_id(),
        occurred_at: now,
    };

    let mut db = lock_db(state)?;
    let (stored, movements) =
        db.open_cash_shift_with_outbox(new_shift, &opening_movement_id, &meta)?;
    Ok(CashShift::from_db(stored, movements))
}

pub fn close_cash_shift_impl(
    state: &AppState,
    cash_shift_id: &str,
    actual_cash_paise: i64,
    variance_reason: Option<String>,
) -> AppResult<CashShift> {
    let now = now_iso();
    let req = CloseCashShiftRequest {
        cash_shift_id: cash_shift_id.to_string(),
        actual_cash_paise,
        closed_at: now.clone(),
        updated_at: now.clone(),
        variance_reason,
    };
    let meta = CashShiftOutboxMeta {
        outbox_id: new_id(),
        occurred_at: now,
    };

    let mut db = lock_db(state)?;
    let (stored, movements) = db.close_cash_shift_with_outbox(req, &meta)?;
    Ok(CashShift::from_db(stored, movements))
}

pub fn record_paid_in_out_impl(
    state: &AppState,
    cash_shift_id: &str,
    kind: &str,
    amount_paise: i64,
    reason: &str,
    created_by_user_id: &str,
) -> AppResult<CashMovement> {
    let req = PaidInOutRequest {
        id: new_id(),
        cash_shift_id: cash_shift_id.to_string(),
        kind: kind.to_string(),
        amount_paise,
        reason: reason.to_string(),
        created_by_user_id: created_by_user_id.to_string(),
        created_at: now_iso(),
    };

    let mut db = lock_db(state)?;
    let movement = db.record_paid_in_out_with_outbox(req)?;
    Ok(CashMovement::from(movement))
}

pub fn get_cash_shift_impl(state: &AppState, cash_shift_id: &str) -> AppResult<Option<CashShift>> {
    let db = lock_db(state)?;
    let shift = match db.get_cash_shift(cash_shift_id)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let movements = db.list_cash_movements_for_shift(cash_shift_id)?;
    Ok(Some(CashShift::from_db(shift, movements)))
}

/// Recovers `cashier_user_id`'s currently OPEN shift on THIS device, if any
/// (T9 retry, Defect 2 — "cash shift restart is an operational dead end").
/// The POS calls this on startup, once it knows which cashier is logged in,
/// instead of relying on an in-memory id that a restart erases
/// (`apps/pos/src/store/cashShift.ts`). Automatic recovery, not a manual
/// id-entry box: a cashier never needs to know a shift id exists.
pub fn find_open_cash_shift_impl(
    state: &AppState,
    cashier_user_id: &str,
) -> AppResult<Option<CashShift>> {
    let db = lock_db(state)?;
    let shift = match db.find_open_cash_shift(&state.device_id, cashier_user_id)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let movements = db.list_cash_movements_for_shift(&shift.id)?;
    Ok(Some(CashShift::from_db(shift, movements)))
}

// -------------------------------------------------------------- commands --

#[tauri::command]
pub fn issue_invoice(
    state: State<'_, AppState>,
    order_id: String,
    created_by_user_id: String,
) -> AppResult<Invoice> {
    issue_invoice_impl(&state, &order_id, &created_by_user_id)
}

#[tauri::command]
pub fn list_invoices_for_order(
    state: State<'_, AppState>,
    order_id: String,
) -> AppResult<Vec<Invoice>> {
    list_invoices_for_order_impl(&state, &order_id)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn record_payment(
    state: State<'_, AppState>,
    order_id: String,
    method: String,
    amount_paise: i64,
    tendered_paise: Option<i64>,
    change_paise: Option<i64>,
    reference: Option<String>,
    cash_shift_id: Option<String>,
    reverses_payment_id: Option<String>,
    invoice_id: Option<String>,
    created_by_user_id: String,
) -> AppResult<Payment> {
    record_payment_impl(
        &state,
        &order_id,
        &method,
        amount_paise,
        tendered_paise,
        change_paise,
        reference,
        cash_shift_id,
        reverses_payment_id,
        invoice_id,
        &created_by_user_id,
    )
}

#[tauri::command]
pub fn list_payments_for_order(
    state: State<'_, AppState>,
    order_id: String,
) -> AppResult<Vec<Payment>> {
    list_payments_for_order_impl(&state, &order_id)
}

#[tauri::command]
pub fn open_cash_shift(
    state: State<'_, AppState>,
    cashier_user_id: String,
    opening_cash_paise: i64,
) -> AppResult<CashShift> {
    open_cash_shift_impl(&state, &cashier_user_id, opening_cash_paise)
}

#[tauri::command]
pub fn close_cash_shift(
    state: State<'_, AppState>,
    cash_shift_id: String,
    actual_cash_paise: i64,
    variance_reason: Option<String>,
) -> AppResult<CashShift> {
    close_cash_shift_impl(&state, &cash_shift_id, actual_cash_paise, variance_reason)
}

#[tauri::command]
pub fn record_paid_in_out(
    state: State<'_, AppState>,
    cash_shift_id: String,
    kind: String,
    amount_paise: i64,
    reason: String,
    created_by_user_id: String,
) -> AppResult<CashMovement> {
    record_paid_in_out_impl(
        &state,
        &cash_shift_id,
        &kind,
        amount_paise,
        &reason,
        &created_by_user_id,
    )
}

#[tauri::command]
pub fn get_cash_shift(
    state: State<'_, AppState>,
    cash_shift_id: String,
) -> AppResult<Option<CashShift>> {
    get_cash_shift_impl(&state, &cash_shift_id)
}

#[tauri::command]
pub fn find_open_cash_shift(
    state: State<'_, AppState>,
    cashier_user_id: String,
) -> AppResult<Option<CashShift>> {
    find_open_cash_shift_impl(&state, &cashier_user_id)
}
