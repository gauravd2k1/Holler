//! Milestone 5 procurement surfaces (ADR-019, track T4): goods receipt,
//! purchase return, and the human-visible `grn_gap` report. A thin IPC
//! wrapper over `holler_edge_database`'s shipped M5 write path
//! (`edge/database/src/procurement/`) — this module resolves nothing,
//! converts no purchase unit and computes no cost. Every one of those is the
//! edge's, deliberately: ADR-019 §3 makes the conversion happen EXACTLY ONCE,
//! at the edge, and an echo computed here could disagree with the write.
//!
//! ============================================================================
//! A GRN NEVER BLOCKS ON A PO
//! ============================================================================
//!
//! Nothing in this module rejects a receipt for a business or configuration
//! reason. A missing purchase order, a PO this edge never synced, an item the
//! PO does not list, an unconfigured supplier, an unconvertible unit — each
//! records a `grn_gap` inside the edge's own transaction and ACCEPTS the
//! receipt. The only rejections that reach a caller are malformed input (a
//! non-positive quantity, an unparseable decimal), which is a caller defect,
//! not a shop-floor condition.
//!
//! **Permission gating is NOT enforced here.** No Tauri command in this crate
//! enforces a backend permission check today — see `commands::inventory` and
//! `commands::billing` for the same pre-existing gap, recorded against M3 and
//! M4. The frontend gates `procurement.manage` before invoking these commands
//! (`apps/pos/src/domain/procurement.ts`). This module does not widen that
//! gap and does not close it either.
//!
//! **A `Db` READ-SURFACE GAP IS REPORTED, NOT ROUTED AROUND.** There is no
//! sanctioned read for `supplier`, `supplier_item` or `purchase_order` on
//! `holler_edge_database::Db` — no `list_suppliers`, no `list_supplier_items`,
//! no `list_purchase_orders`. This crate is deliberately built without
//! `rusqlite` (see its `Cargo.toml`: "consumes that API only — never touches
//! the SQLite file directly"), so a raw SELECT from here is not merely
//! discouraged, it is structurally impossible. The receiving screen therefore
//! takes the supplier and purchase-order identifiers as typed references
//! rather than pickers — which is the only possible behaviour for the "PO
//! that never synced" case (M5 acceptance criterion 3), and a stopgap for the
//! case where the row IS present locally. Reported.
//!
//! **The `_with_outbox` forms are the only ones called here**, exactly as
//! `commands::inventory` calls only the `_with_outbox` stock-count forms: the
//! plain `Db::record_goods_receipt`/`record_purchase_return` write the rows
//! and emit no replay event at all, which would leave the cloud permanently
//! unaware of a receipt the outlet is certain about.

use holler_edge_database::model::{
    NewGoodsReceiptNote, NewGrnLine, NewPurchaseReturn, NewPurchaseReturnLine,
    ProcurementOutboxMeta,
};
use holler_edge_database::Db;
use tauri::State;

use crate::dto::{
    GoodsReceiptNote, GrnEntryIntentEcho, GrnGap, PurchaseOrderReceiptProgress, PurchaseReturn,
};
use crate::error::{AppError, AppResult};
use crate::ids::{new_id, now_iso};
use crate::state::AppState;

fn lock_db(state: &AppState) -> AppResult<std::sync::MutexGuard<'_, Db>> {
    state.db.lock().map_err(|_| AppError {
        code: "LOCK_POISONED",
        message: "database lock poisoned".into(),
    })
}

/// Micro-units per whole purchase unit. `entered_quantity_micro` counts the
/// SUPPLIER'S OWN unit — "3 sacks = 3_000_000" (`model::NewGrnLine`) — so the
/// scale is the canonical ×10^6 fixed point every count-like quantity uses,
/// and is NOT the item's dimensional scale (a millilitre is 1_000, a gram
/// 1_000_000; neither applies to a sack).
///
/// Expressed through the edge crate's own typed constructor rather than a raw
/// literal, for the reason `edge/database/src/inventory/units.rs` gives: a
/// hand-written micro multiplier is silently wrong by 1000× and no CHECK can
/// catch it, because both answers are just integers. `pieces` is the right
/// constructor here because an entered receipt quantity IS a count of
/// purchase units.
const MICRO_PER_PURCHASE_UNIT: i64 = holler_edge_database::inventory::pieces(1);

/// The largest number of fractional digits a typed receipt quantity may
/// carry — the scale `entered_quantity_micro` can represent exactly.
const MAX_QUANTITY_DECIMALS: usize = 6;

/// Parses an operator-typed purchase-unit quantity ("4", "12.5", "0.750")
/// into integer micro-units of that purchase unit.
///
/// **Exact integer parsing, no floating point anywhere.** A delivery note
/// reads "12.5 kg" and the operator types exactly that; rounding it to 12 or
/// fat-fingering it to 125 is the failure this milestone's echo exists to
/// catch, so the entry path has to be able to represent it in the first
/// place. The fractional part is right-padded to the micro scale and parsed
/// as its own integer — `12.5` becomes `12 * 1_000_000 + 500_000`, never
/// `12.5 * 1e6`.
///
/// Rejects — these are typing defects, never shop-floor conditions, and are
/// the ONLY rejections this module has:
/// * anything that is not `digits` or `digits.digits`
/// * more than [`MAX_QUANTITY_DECIMALS`] fractional digits (it would be
///   silently truncated, and a silently truncated quantity is the exact
///   class of error being guarded against)
/// * zero, or a magnitude past `i64`
pub fn parse_purchase_quantity_micro(entered: &str) -> AppResult<i64> {
    let trimmed = entered.trim();
    let invalid = || AppError {
        code: "INVALID_RECEIPT_QUANTITY",
        message: format!(
            "\"{trimmed}\" is not a quantity. Enter a number of purchase units, \
             for example 4 or 12.5 (at most {MAX_QUANTITY_DECIMALS} decimal places)."
        ),
    };
    if trimmed.is_empty() {
        return Err(invalid());
    }
    let (whole_str, frac_str) = match trimmed.split_once('.') {
        Some((w, f)) => (w, f),
        None => (trimmed, ""),
    };
    if whole_str.is_empty() && frac_str.is_empty() {
        return Err(invalid());
    }
    if !whole_str.chars().all(|c| c.is_ascii_digit())
        || !frac_str.chars().all(|c| c.is_ascii_digit())
    {
        return Err(invalid());
    }
    if frac_str.len() > MAX_QUANTITY_DECIMALS {
        return Err(AppError {
            code: "INVALID_RECEIPT_QUANTITY",
            message: format!(
                "\"{trimmed}\" has more than {MAX_QUANTITY_DECIMALS} decimal places, which \
                 cannot be recorded exactly. Re-enter it rounded to \
                 {MAX_QUANTITY_DECIMALS} decimal places."
            ),
        });
    }
    let whole: i64 = if whole_str.is_empty() {
        0
    } else {
        whole_str.parse().map_err(|_| invalid())?
    };
    let mut padded = frac_str.to_string();
    while padded.len() < MAX_QUANTITY_DECIMALS {
        padded.push('0');
    }
    let frac: i64 = padded.parse().map_err(|_| invalid())?;
    let micro = whole
        .checked_mul(MICRO_PER_PURCHASE_UNIT)
        .and_then(|w| w.checked_add(frac))
        .ok_or_else(|| AppError {
            code: "INVALID_RECEIPT_QUANTITY",
            message: format!("\"{trimmed}\" is too large to record as a receipt quantity."),
        })?;
    if micro <= 0 {
        return Err(AppError {
            code: "INVALID_RECEIPT_QUANTITY",
            message: "Enter a quantity greater than zero.".to_string(),
        });
    }
    Ok(micro)
}

/// One receiving line as the screen submits it.
///
/// **`quantity_dimension` IS THE UNIT THE OPERATOR CHOSE.** It is a required
/// field on this struct — not an `Option`, not defaulted, and never looked up
/// from `inventory_item.dimension` here or in the screen above. If any layer
/// filled it from the referent, the edge's `DIMENSION_MISMATCH` comparison
/// would become `x == x`, the guard could never fire, and it would look
/// entirely correct in review (contracts 0.5.2/0.6.0, ADR-019 §6).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewGrnLineRequest {
    pub inventory_item_id: String,
    /// The supplier's own label off the delivery note — "SACK", "kg",
    /// "CRATE". Free text by contract; the CONVERSION is what must be exact.
    pub entered_purchase_unit: String,
    /// What the operator typed, verbatim, as a decimal string. Parsed to
    /// micro-units by [`parse_purchase_quantity_micro`] — see its doc comment
    /// for why the UI does not do this arithmetic itself.
    pub entered_quantity: String,
    /// `"MASS" | "VOLUME" | "COUNT"`. See the struct doc comment.
    pub quantity_dimension: String,
    /// Paise for ONE `entered_purchase_unit`, off the delivery note. The
    /// per-base-unit cost the ledger carries is derived from this AT THE EDGE
    /// and is never computed here (CLAUDE.md: money is computed by the edge,
    /// formatted by the layers above it).
    pub purchase_price_paise: i64,
    pub batch_code: Option<String>,
    pub expiry_date: Option<String>,
    /// The PO line this answers, when the caller already knows it. `None` is
    /// ordinary: the edge matches it and gaps when there is none.
    pub purchase_order_line_id: Option<String>,
}

impl NewGrnLineRequest {
    fn into_model(self) -> AppResult<NewGrnLine> {
        let entered_quantity_micro = parse_purchase_quantity_micro(&self.entered_quantity)?;
        Ok(NewGrnLine {
            inventory_item_id: self.inventory_item_id,
            entered_purchase_unit: self.entered_purchase_unit,
            entered_quantity_micro,
            quantity_dimension: self.quantity_dimension,
            purchase_price_paise: self.purchase_price_paise,
            batch_code: empty_to_none(self.batch_code),
            expiry_date: empty_to_none(self.expiry_date),
            purchase_order_line_id: empty_to_none(self.purchase_order_line_id),
        })
    }
}

/// One purchase-return line as the screen submits it. Same
/// author-chose-the-dimension discipline as [`NewGrnLineRequest`].
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewPurchaseReturnLineRequest {
    pub inventory_item_id: String,
    pub grn_line_id: Option<String>,
    pub entered_purchase_unit: String,
    pub entered_quantity: String,
    pub quantity_dimension: String,
    /// Paise per BASE unit. `None` means "value it at what this outlet
    /// actually paid" — the weighted average cost derived from the ledger by
    /// the edge. Never a silent zero: a blank field and a zero cost are
    /// different statements and stay different here.
    pub unit_cost_paise: Option<i64>,
}

impl NewPurchaseReturnLineRequest {
    fn into_model(self) -> AppResult<NewPurchaseReturnLine> {
        let entered_quantity_micro = parse_purchase_quantity_micro(&self.entered_quantity)?;
        Ok(NewPurchaseReturnLine {
            inventory_item_id: self.inventory_item_id,
            grn_line_id: empty_to_none(self.grn_line_id),
            entered_purchase_unit: self.entered_purchase_unit,
            entered_quantity_micro,
            quantity_dimension: self.quantity_dimension,
            unit_cost_paise: self.unit_cost_paise,
        })
    }
}

/// A blank string from a form field is an ABSENT value, not the empty string.
/// Applied to the supplier and purchase-order references above all: a blank
/// PO box means "no purchase order", which is an accepted receipt with a
/// `NO_PURCHASE_ORDER` gap — never a refusal, and never a `Some("")` the edge
/// would look up and fail to find under a different reason.
fn empty_to_none(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

// ----------------------------------------------------------------- reads --

/// The `grn_gap` report behind **M5 acceptance criterion 3**: the gap must be
/// VISIBLE TO A HUMAN ON THE POS, not merely present in a table. Bounded and
/// newest-first at the edge; this wrapper adds no filtering of its own, so
/// what the screen shows is what the edge sanctioned.
pub fn list_grn_gaps_impl(state: &AppState) -> AppResult<Vec<GrnGap>> {
    let db = lock_db(state)?;
    let gaps = db.list_grn_gaps(&state.outlet_id)?;
    Ok(gaps.into_iter().map(GrnGap::from).collect())
}

/// **THIS OUTLET's** receipt progress for one purchase order.
///
/// The cloud's figure for the same PO will differ and BOTH ARE RIGHT
/// (ADR-019 §4) — the cloud sums every outlet's receipts, this sums one
/// outlet's. The DTO's field name says `_at_this_outlet` for that reason, and
/// the screen labels it. Never reconciled against a cloud figure anywhere.
pub fn purchase_order_receipt_progress_impl(
    state: &AppState,
    purchase_order_id: &str,
) -> AppResult<Vec<PurchaseOrderReceiptProgress>> {
    let db = lock_db(state)?;
    let rows = db.purchase_order_receipt_progress(purchase_order_id)?;
    Ok(rows
        .into_iter()
        .map(PurchaseOrderReceiptProgress::from)
        .collect())
}

/// Weighted average cost per BASE unit, in paise, derived from the ledger by
/// the edge on every call. `None` means this outlet has never recorded a
/// costed receipt for the item — **which is not the same as zero**, and no
/// layer above may coerce it to one.
pub fn weighted_average_cost_paise_impl(
    state: &AppState,
    inventory_item_id: &str,
) -> AppResult<Option<i64>> {
    let db = lock_db(state)?;
    Ok(db.weighted_average_cost_paise(&state.outlet_id, inventory_item_id)?)
}

/// The `entryIntentEcho` behind **M5 acceptance criterion 4**. Read-only, and
/// it runs the SAME resolution the write runs — see
/// `Db::grn_entry_intent_echo`. This wrapper adds no arithmetic; it parses the
/// typed quantity and hands the line straight to the edge.
pub fn grn_entry_intent_echo_impl(
    state: &AppState,
    supplier_id: Option<String>,
    line: NewGrnLineRequest,
) -> AppResult<GrnEntryIntentEcho> {
    let mut db = lock_db(state)?;
    let model = line.into_model()?;
    let echo = db.grn_entry_intent_echo(empty_to_none(supplier_id).as_deref(), &model)?;
    Ok(GrnEntryIntentEcho::from(echo))
}

// ---------------------------------------------------------------- writes --

/// Records one goods receipt, its gaps and its `PURCHASE` ledger entries —
/// all in ONE edge transaction, alongside the outbox rows. **M5 acceptance
/// criterion 2 (kill the POS between the GRN write and the ledger post) is
/// met by that transaction, not by anything in this function.**
///
/// `purchase_order_id` and `supplier_id` are `Option` and stay `Option`: a
/// blank one is not an error and must never become one.
pub fn record_goods_receipt_impl(
    state: &AppState,
    purchase_order_id: Option<String>,
    supplier_id: Option<String>,
    delivery_note_ref: Option<String>,
    notes: Option<String>,
    received_by_user_id: &str,
    lines: Vec<NewGrnLineRequest>,
) -> AppResult<GoodsReceiptNote> {
    let mut db = lock_db(state)?;
    // One instant for the receipt and its outbox event, so "when did this
    // arrive" cannot disagree between the row and the event that replays it.
    let occurred_at = now_iso();
    let model_lines = lines
        .into_iter()
        .map(NewGrnLineRequest::into_model)
        .collect::<AppResult<Vec<_>>>()?;
    let stored = db.record_goods_receipt_with_outbox(
        NewGoodsReceiptNote {
            id: new_id(),
            outlet_id: state.outlet_id.clone(),
            purchase_order_id: empty_to_none(purchase_order_id),
            supplier_id: empty_to_none(supplier_id),
            delivery_note_ref: empty_to_none(delivery_note_ref),
            received_at: occurred_at.clone(),
            received_by_user_id: received_by_user_id.to_string(),
            notes: empty_to_none(notes),
            lines: model_lines,
        },
        &ProcurementOutboxMeta {
            outbox_id: new_id(),
            occurred_at,
        },
    )?;
    Ok(GoodsReceiptNote::from(stored))
}

/// Records one purchase return and its `RETURN_TO_VENDOR` ledger entries, in
/// one edge transaction with its outbox row.
///
/// `return_number` is caller-supplied because contracts 0.6.0 mints a counter
/// for the GRN (`grn_sequence`) and none for this document — reported by
/// `edge/database/src/procurement/numbering.rs` as a contract asymmetry
/// rather than worked around, and not invented here either. The screen asks
/// the operator for their own reference.
#[allow(clippy::too_many_arguments)]
pub fn record_purchase_return_impl(
    state: &AppState,
    supplier_id: Option<String>,
    grn_id: Option<String>,
    return_number: &str,
    reason: &str,
    notes: Option<String>,
    returned_by_user_id: &str,
    lines: Vec<NewPurchaseReturnLineRequest>,
) -> AppResult<PurchaseReturn> {
    let mut db = lock_db(state)?;
    let occurred_at = now_iso();
    let model_lines = lines
        .into_iter()
        .map(NewPurchaseReturnLineRequest::into_model)
        .collect::<AppResult<Vec<_>>>()?;
    let stored = db.record_purchase_return_with_outbox(
        NewPurchaseReturn {
            id: new_id(),
            outlet_id: state.outlet_id.clone(),
            supplier_id: empty_to_none(supplier_id),
            grn_id: empty_to_none(grn_id),
            return_number: return_number.trim().to_string(),
            reason: reason.to_string(),
            returned_at: occurred_at.clone(),
            returned_by_user_id: returned_by_user_id.to_string(),
            notes: empty_to_none(notes),
            lines: model_lines,
        },
        &ProcurementOutboxMeta {
            outbox_id: new_id(),
            occurred_at,
        },
    )?;
    Ok(PurchaseReturn::from(stored))
}

// -------------------------------------------------------------- commands --

#[tauri::command]
pub fn list_grn_gaps(state: State<'_, AppState>) -> AppResult<Vec<GrnGap>> {
    list_grn_gaps_impl(&state)
}

#[tauri::command]
pub fn purchase_order_receipt_progress(
    state: State<'_, AppState>,
    purchase_order_id: String,
) -> AppResult<Vec<PurchaseOrderReceiptProgress>> {
    purchase_order_receipt_progress_impl(&state, &purchase_order_id)
}

#[tauri::command]
pub fn weighted_average_cost_paise(
    state: State<'_, AppState>,
    inventory_item_id: String,
) -> AppResult<Option<i64>> {
    weighted_average_cost_paise_impl(&state, &inventory_item_id)
}

#[tauri::command]
pub fn grn_entry_intent_echo(
    state: State<'_, AppState>,
    supplier_id: Option<String>,
    line: NewGrnLineRequest,
) -> AppResult<GrnEntryIntentEcho> {
    grn_entry_intent_echo_impl(&state, supplier_id, line)
}

#[tauri::command]
pub fn record_goods_receipt(
    state: State<'_, AppState>,
    purchase_order_id: Option<String>,
    supplier_id: Option<String>,
    delivery_note_ref: Option<String>,
    notes: Option<String>,
    received_by_user_id: String,
    lines: Vec<NewGrnLineRequest>,
) -> AppResult<GoodsReceiptNote> {
    record_goods_receipt_impl(
        &state,
        purchase_order_id,
        supplier_id,
        delivery_note_ref,
        notes,
        &received_by_user_id,
        lines,
    )
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn record_purchase_return(
    state: State<'_, AppState>,
    supplier_id: Option<String>,
    grn_id: Option<String>,
    return_number: String,
    reason: String,
    notes: Option<String>,
    returned_by_user_id: String,
    lines: Vec<NewPurchaseReturnLineRequest>,
) -> AppResult<PurchaseReturn> {
    record_purchase_return_impl(
        &state,
        supplier_id,
        grn_id,
        &return_number,
        &reason,
        notes,
        &returned_by_user_id,
        lines,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_purchase_units_scale_to_micro() {
        assert_eq!(parse_purchase_quantity_micro("4").unwrap(), 4_000_000);
        assert_eq!(parse_purchase_quantity_micro(" 50 ").unwrap(), 50_000_000);
    }

    #[test]
    fn a_decimal_delivery_note_quantity_is_exact() {
        // "12.5 kg" off a delivery note. Parsed as 12 * 1e6 + 500_000, never
        // through a float.
        assert_eq!(parse_purchase_quantity_micro("12.5").unwrap(), 12_500_000);
        assert_eq!(parse_purchase_quantity_micro("0.750").unwrap(), 750_000);
        assert_eq!(parse_purchase_quantity_micro("0.000001").unwrap(), 1);
    }

    #[test]
    fn more_decimals_than_the_scale_is_refused_not_truncated() {
        // A silently truncated quantity is exactly the class of error the
        // echo exists to catch, so it must not be produced here.
        let err = parse_purchase_quantity_micro("1.0000005").unwrap_err();
        assert_eq!(err.code, "INVALID_RECEIPT_QUANTITY");
    }

    #[test]
    fn zero_blank_and_nonsense_are_refused() {
        for bad in ["", "  ", "0", "0.0", "abc", "-4", "1.2.3", "4kg"] {
            assert!(
                parse_purchase_quantity_micro(bad).is_err(),
                "expected {bad:?} to be refused"
            );
        }
    }

    #[test]
    fn the_micro_scale_comes_from_the_edge_constructor() {
        assert_eq!(MICRO_PER_PURCHASE_UNIT, 1_000_000);
    }

    #[test]
    fn a_blank_form_field_is_absent_not_empty() {
        assert_eq!(empty_to_none(Some("  ".to_string())), None);
        assert_eq!(empty_to_none(Some(String::new())), None);
        assert_eq!(empty_to_none(None), None);
        assert_eq!(
            empty_to_none(Some("po-1".to_string())),
            Some("po-1".to_string())
        );
    }
}
