//! Milestone 4 inventory surfaces (ADR-018, T5): the bounded current-stock
//! read (what the low-stock signal and every item picker read from), wastage
//! recording, physical stock counts and their variance report. Thin IPC
//! wrapper over `holler_edge_database`'s shipped M4 write path — no
//! arithmetic of its own beyond converting the cashier's typed human-unit
//! quantity (grams/millilitres/pieces) into that item's own dimension of
//! micro-units at the boundary, using the edge crate's own typed
//! constructors (`holler_edge_database::inventory::{grams, millilitres,
//! pieces}`) rather than a raw `* 1_000_000` literal anywhere in this file.
//!
//! **Permission gating is NOT enforced here.** No Tauri command in this
//! crate enforces a backend permission check today — see
//! `commands::billing`'s module doc comment for the same pre-existing gap
//! recorded against billing. The frontend gates `inventory.manage`/
//! `inventory.count` before invoking these commands (see
//! `apps/pos/src/domain/permissions.ts`); this module does not widen that
//! gap, but it does not close it either.
//!
//! **Two `Db` surface gaps found while wiring this task, neither worked
//! around — reported rather than routed around (task instruction):**
//!
//! 1. `Db` exposes no way to list `stock_deduction_gap` rows. The only
//!    existing reader is a private `#[cfg(test)] mod tests` helper in
//!    `edge/database/src/lib.rs`, unreachable from this crate. Requirement 4
//!    ("items sold with no recipe" report) therefore has no data source this
//!    crate can call — not implemented, rather than hand-rolling a raw SQL
//!    query against a table this crate has no sanctioned read path for.
//! 2. `Db::open_stock_count`/`Db::complete_stock_count` have no
//!    `_with_outbox` sibling, and `Db` exposes no transaction handle a
//!    caller outside the crate could attach a second write to
//!    (`Db::connection()` is `&Connection`, read-only; there is no
//!    `connection_mut`/`transaction` accessor). `StockCountOpened`/
//!    `StockCountCompleted` cannot be emitted from this crate without
//!    either a second, non-atomic transaction (explicitly forbidden by the
//!    task) or an edge-side `_with_outbox` addition (`edge/` is not this
//!    task's directory). Requirement 5 is therefore not implemented, and
//!    both entries stay in `scripts/check-event-type-drift.mjs`'s
//!    `NOT_YET_EMITTED`, unchanged.

use holler_edge_database::model::{NewStockCount, NewStockCountLine, NewWastageEntry};
use holler_edge_database::Db;
use tauri::State;

use crate::dto::{
    CurrentStockLine, StockCount, StockCountLine, StockCountVarianceReport, StockLedgerEntry,
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

/// Converts a cashier-entered human-unit quantity — whole grams for MASS,
/// whole millilitres for VOLUME, whole pieces for COUNT — into that item's
/// own micro-units, via the edge crate's typed constructors. This is the
/// ONLY place in this crate a human-facing inventory quantity is converted;
/// no raw micro literal appears anywhere else in this module.
fn human_quantity_to_micro(dimension: &str, human_quantity: i64) -> AppResult<i64> {
    match dimension {
        "MASS" => Ok(holler_edge_database::inventory::grams(human_quantity)),
        "VOLUME" => Ok(holler_edge_database::inventory::millilitres(human_quantity)),
        "COUNT" => Ok(holler_edge_database::inventory::pieces(human_quantity)),
        other => Err(AppError {
            code: "UNKNOWN_DIMENSION",
            message: format!("inventory item has an unrecognised dimension '{other}'"),
        }),
    }
}

fn require_inventory_item(
    db: &Db,
    inventory_item_id: &str,
) -> AppResult<holler_edge_database::model::InventoryItem> {
    holler_edge_database::repo::get_inventory_item(db.connection(), inventory_item_id)?.ok_or_else(
        || AppError {
            code: "NOT_FOUND",
            message: format!("inventory item {inventory_item_id} not found"),
        },
    )
}

// ------------------------------------------------------------ stock reads --

pub fn list_current_stock_impl(state: &AppState) -> AppResult<Vec<CurrentStockLine>> {
    let db = lock_db(state)?;
    let lines = db.list_current_stock(&state.outlet_id)?;
    Ok(lines.into_iter().map(CurrentStockLine::from).collect())
}

// ---------------------------------------------------------------- wastage --

pub fn record_wastage_impl(
    state: &AppState,
    inventory_item_id: &str,
    quantity: i64,
    reason_code: &str,
    note: Option<String>,
    created_by_user_id: &str,
) -> AppResult<StockLedgerEntry> {
    let mut db = lock_db(state)?;
    let item = require_inventory_item(&db, inventory_item_id)?;
    let quantity_micro = human_quantity_to_micro(&item.dimension, quantity)?;

    let stored = db.record_wastage(NewWastageEntry {
        outlet_id: state.outlet_id.clone(),
        inventory_item_id: inventory_item_id.to_string(),
        quantity_micro,
        reason_code: reason_code.to_string(),
        note,
        occurred_at: now_iso(),
        created_by_user_id: Some(created_by_user_id.to_string()),
    })?;
    Ok(StockLedgerEntry::from(stored))
}

// ------------------------------------------------------------ stock count --

pub fn open_stock_count_impl(
    state: &AppState,
    counted_by_user_id: Option<String>,
    note: Option<String>,
) -> AppResult<StockCount> {
    let mut db = lock_db(state)?;
    let stored = db.open_stock_count(NewStockCount {
        id: new_id(),
        outlet_id: state.outlet_id.clone(),
        started_at: now_iso(),
        counted_by_user_id,
        note,
    })?;
    Ok(StockCount::from(stored))
}

pub fn add_or_update_stock_count_line_impl(
    state: &AppState,
    stock_count_id: &str,
    inventory_item_id: &str,
    quantity: i64,
    note: Option<String>,
) -> AppResult<StockCountLine> {
    let mut db = lock_db(state)?;
    let item = require_inventory_item(&db, inventory_item_id)?;
    let counted_quantity_micro = human_quantity_to_micro(&item.dimension, quantity)?;

    let stored = db.add_or_update_stock_count_line(
        stock_count_id,
        &state.outlet_id,
        NewStockCountLine {
            inventory_item_id: inventory_item_id.to_string(),
            counted_quantity_micro,
            note,
        },
    )?;
    Ok(StockCountLine::from(stored))
}

pub fn list_stock_count_lines_impl(
    state: &AppState,
    stock_count_id: &str,
) -> AppResult<Vec<StockCountLine>> {
    let db = lock_db(state)?;
    let lines = db.list_stock_count_lines(stock_count_id)?;
    Ok(lines.into_iter().map(StockCountLine::from).collect())
}

pub fn get_stock_count_impl(
    state: &AppState,
    stock_count_id: &str,
) -> AppResult<Option<StockCount>> {
    let db = lock_db(state)?;
    Ok(db.get_stock_count(stock_count_id)?.map(StockCount::from))
}

pub fn complete_stock_count_impl(state: &AppState, stock_count_id: &str) -> AppResult<StockCount> {
    let mut db = lock_db(state)?;
    let stored = db.complete_stock_count(stock_count_id, &state.outlet_id, &now_iso())?;
    Ok(StockCount::from(stored))
}

pub fn get_stock_count_variance_report_impl(
    state: &AppState,
    stock_count_id: &str,
) -> AppResult<StockCountVarianceReport> {
    let mut db = lock_db(state)?;
    let report = db.get_stock_count_variance_report(stock_count_id, &state.outlet_id)?;
    Ok(StockCountVarianceReport::from(report))
}

// -------------------------------------------------------------- commands --

#[tauri::command]
pub fn list_current_stock(state: State<'_, AppState>) -> AppResult<Vec<CurrentStockLine>> {
    list_current_stock_impl(&state)
}

#[tauri::command]
pub fn record_wastage(
    state: State<'_, AppState>,
    inventory_item_id: String,
    quantity: i64,
    reason_code: String,
    note: Option<String>,
    created_by_user_id: String,
) -> AppResult<StockLedgerEntry> {
    record_wastage_impl(
        &state,
        &inventory_item_id,
        quantity,
        &reason_code,
        note,
        &created_by_user_id,
    )
}

#[tauri::command]
pub fn open_stock_count(
    state: State<'_, AppState>,
    counted_by_user_id: Option<String>,
    note: Option<String>,
) -> AppResult<StockCount> {
    open_stock_count_impl(&state, counted_by_user_id, note)
}

#[tauri::command]
pub fn add_or_update_stock_count_line(
    state: State<'_, AppState>,
    stock_count_id: String,
    inventory_item_id: String,
    quantity: i64,
    note: Option<String>,
) -> AppResult<StockCountLine> {
    add_or_update_stock_count_line_impl(&state, &stock_count_id, &inventory_item_id, quantity, note)
}

#[tauri::command]
pub fn list_stock_count_lines(
    state: State<'_, AppState>,
    stock_count_id: String,
) -> AppResult<Vec<StockCountLine>> {
    list_stock_count_lines_impl(&state, &stock_count_id)
}

#[tauri::command]
pub fn get_stock_count(
    state: State<'_, AppState>,
    stock_count_id: String,
) -> AppResult<Option<StockCount>> {
    get_stock_count_impl(&state, &stock_count_id)
}

#[tauri::command]
pub fn complete_stock_count(
    state: State<'_, AppState>,
    stock_count_id: String,
) -> AppResult<StockCount> {
    complete_stock_count_impl(&state, &stock_count_id)
}

#[tauri::command]
pub fn get_stock_count_variance_report(
    state: State<'_, AppState>,
    stock_count_id: String,
) -> AppResult<StockCountVarianceReport> {
    get_stock_count_variance_report_impl(&state, &stock_count_id)
}
