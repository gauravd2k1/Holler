//! Table definitions and open sessions (docs/spec/tables.md, ADR-011).

use tauri::State;

use crate::dto::{RestaurantTable, TableSession};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub fn list_tables_impl(state: &AppState) -> AppResult<Vec<RestaurantTable>> {
    let db = state.db.lock().map_err(|_| AppError {
        code: "LOCK_POISONED",
        message: "database lock poisoned".into(),
    })?;

    let tables =
        holler_edge_database::repo::list_restaurant_tables(db.connection(), &state.outlet_id)?;
    Ok(tables.into_iter().map(RestaurantTable::from).collect())
}

/// The open session for a table, if any. A table with no open session is
/// AVAILABLE (table.ts `TableDisplayStateSchema`) — `None` here is that
/// state, not an error.
pub fn get_open_table_session_impl(
    state: &AppState,
    table_id: &str,
) -> AppResult<Option<TableSession>> {
    let db = state.db.lock().map_err(|_| AppError {
        code: "LOCK_POISONED",
        message: "database lock poisoned".into(),
    })?;

    let session = holler_edge_database::repo::get_open_table_session(db.connection(), table_id)?;
    Ok(session.map(TableSession::from))
}

#[tauri::command]
pub fn list_tables(state: State<'_, AppState>) -> AppResult<Vec<RestaurantTable>> {
    list_tables_impl(&state)
}

#[tauri::command]
pub fn get_open_table_session(
    state: State<'_, AppState>,
    table_id: String,
) -> AppResult<Option<TableSession>> {
    get_open_table_session_impl(&state, &table_id)
}
