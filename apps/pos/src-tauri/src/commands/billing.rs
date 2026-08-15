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

use std::collections::HashMap;

use holler_edge_database::model::{
    CashShiftOutboxMeta, CloseCashShiftRequest, DiscountDefinition as DbDiscountDefinition,
    InvoiceLineShare, InvoiceOutboxMeta, IssueInvoiceHeader, NewCashShift, NewPayment,
    PaidInOutRequest, PaymentOutboxMeta,
};
use holler_edge_database::Db;
use tauri::State;

use crate::domain::discount::resolve_line_discount_per_unit_paise;
use crate::dto::{CashMovement, CashShift, DiscountDefinition, Invoice, Payment};
use crate::error::{AppError, AppResult};
use crate::ids::{new_id, now_iso};
use crate::state::AppState;

/// One cashier-chosen discount application, submitted alongside `issue_invoice`
/// (docs/spec/compliance.md, ADR-016 §28). `reason` is `None`/blank unless the
/// cashier actually typed one — `domain::discount::resolve_line_discount_per_unit_paise`
/// is what decides whether that is acceptable for the named definition.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct LineDiscountInput {
    pub order_item_id: String,
    pub discount_definition_id: String,
    pub reason: Option<String>,
}

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

/// `discounts_by_item` carries each line's already-resolved
/// `discount_per_unit_paise` (see `resolve_line_discounts` below) — a line
/// with no entry gets `0`, exactly the prior hard-coded behaviour, so an
/// order billed with no discount applied is unaffected byte-for-byte.
fn build_invoice_lines(
    items: &[holler_edge_database::model::OrderItem],
    discounts_by_item: &HashMap<String, i64>,
) -> Vec<InvoiceLineShare> {
    items
        .iter()
        .map(|item| InvoiceLineShare {
            id: new_id(),
            order_item_id: item.id.clone(),
            quantity: item.quantity,
            discount_per_unit_paise: discounts_by_item.get(&item.id).copied().unwrap_or(0),
        })
        .collect()
}

/// Resolves every cashier-supplied `LineDiscountInput` into the per-unit
/// paise figure `build_invoice_lines` needs, per §28/ADR-016. Looks up each
/// named `discount_definition` from this outlet's own config (never trusts
/// the frontend's copy of it) and the caller's real, currently-stored
/// permission set (never trusts a claimed permission) — `requires_reason`/
/// `required_permission` are enforced here, not merely displayed
/// (task requirement: "binding, not advisory").
///
/// This performs no tax arithmetic — it only turns a governance row plus a
/// line's snapshot `unit_price_paise` into ONE input number
/// (`holler_edge_database::tax::engine::compute_line_base`'s own guards
/// still validate that number is non-negative and does not exceed
/// `unit_price_paise`, deliberately left to fire on whatever this produces).
fn resolve_line_discounts(
    items: &[holler_edge_database::model::OrderItem],
    definitions: &[DbDiscountDefinition],
    caller_permissions: &[String],
    now: &str,
    inputs: &[LineDiscountInput],
) -> AppResult<HashMap<String, i64>> {
    let mut resolved = HashMap::with_capacity(inputs.len());
    for input in inputs {
        let item = items
            .iter()
            .find(|i| i.id == input.order_item_id)
            .ok_or_else(|| AppError {
                code: "ORDER_ITEM_NOT_FOUND",
                message: format!(
                    "order item {} is not on this order — cannot apply a discount to it",
                    input.order_item_id
                ),
            })?;
        let def = definitions
            .iter()
            .find(|d| d.id == input.discount_definition_id)
            .ok_or_else(|| AppError {
                code: "DISCOUNT_DEFINITION_NOT_FOUND",
                message: format!(
                    "discount {} is not configured for this outlet",
                    input.discount_definition_id
                ),
            })?;
        if def.effective_from.as_str() > now
            || def.effective_to.as_deref().is_some_and(|to| to < now)
        {
            return Err(AppError {
                code: "DISCOUNT_NOT_ACTIVE",
                message: format!("discount '{}' is not effective right now", def.code),
            });
        }
        let per_unit = resolve_line_discount_per_unit_paise(
            def,
            item.unit_price_paise,
            input.reason.as_deref(),
            caller_permissions,
        )?;
        resolved.insert(input.order_item_id.clone(), per_unit);
    }
    Ok(resolved)
}

/// The real permission set for `user_id` — read fresh from `app_user` rather
/// than trusted from the caller (the same discipline `issue_invoice_impl`
/// already applies to every other billing input). Empty if the row's
/// `permissions_json` fails to parse, which just means "grants nothing"
/// rather than a hard failure — a missing discount permission is a legible
/// `DISCOUNT_PERMISSION_DENIED` from `resolve_line_discounts`, not a crash.
fn caller_permissions(db: &Db, user_id: &str) -> AppResult<Vec<String>> {
    let user = holler_edge_database::repo::get_app_user_by_id(db.connection(), user_id)?;
    Ok(user
        .and_then(|u| serde_json::from_str::<Vec<String>>(&u.permissions_json).ok())
        .unwrap_or_default())
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
    discounts: &[LineDiscountInput],
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

    let discounts_by_item = if discounts.is_empty() {
        HashMap::new()
    } else {
        let definitions = holler_edge_database::repo::list_discount_definitions_for_outlet(
            db.connection(),
            &state.outlet_id,
        )?;
        let permissions = caller_permissions(&db, created_by_user_id)?;
        resolve_line_discounts(&items, &definitions, &permissions, &now, discounts)?
    };

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

    let lines = build_invoice_lines(&items, &discounts_by_item);
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

/// This outlet's discount catalogue (`discount_definition`, CLOUD_TO_EDGE
/// config, ADR-016 §1) — read-only, so the POS can offer a cashier a real
/// choice instead of a free-text discount box. Includes inactive/not-yet-
/// effective rows too; the frontend and `resolve_line_discounts` both apply
/// the effective/active gate at the moment a discount is actually used, not
/// at list time, so a manager can see a future-dated discount already
/// queued up.
pub fn list_discount_definitions_impl(state: &AppState) -> AppResult<Vec<DiscountDefinition>> {
    let db = lock_db(state)?;
    let defs = holler_edge_database::repo::list_discount_definitions_for_outlet(
        db.connection(),
        &state.outlet_id,
    )?;
    Ok(defs.into_iter().map(DiscountDefinition::from).collect())
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
    discounts: Option<Vec<LineDiscountInput>>,
) -> AppResult<Invoice> {
    issue_invoice_impl(
        &state,
        &order_id,
        &created_by_user_id,
        &discounts.unwrap_or_default(),
    )
}

#[tauri::command]
pub fn list_invoices_for_order(
    state: State<'_, AppState>,
    order_id: String,
) -> AppResult<Vec<Invoice>> {
    list_invoices_for_order_impl(&state, &order_id)
}

#[tauri::command]
pub fn list_discount_definitions(state: State<'_, AppState>) -> AppResult<Vec<DiscountDefinition>> {
    list_discount_definitions_impl(&state)
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
