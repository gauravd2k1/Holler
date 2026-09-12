//! Table definitions and open sessions (docs/spec/tables.md, ADR-011).

use tauri::State;

use crate::dto::{OutletIdentity, RestaurantTable, TableSession};
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

/// The outlet's own name and id, for a header that says whose restaurant this
/// is.
///
/// WHITE-LABEL: HOLLER IS THE PRODUCT, THE OUTLET IS THE BUSINESS. Every
/// surface reads this rather than holding its own copy of the name, so
/// renaming the restaurant is one seed value and a re-seed -- not a search
/// across four frontends. The captain page has resolved its header this way
/// since it was built (`captain.rs::handle_session`); this is the same read,
/// exposed to the till.
pub fn get_outlet_identity_impl(state: &AppState) -> AppResult<OutletIdentity> {
    let db = state.db.lock().map_err(|_| AppError {
        code: "LOCK_POISONED",
        message: "database lock poisoned".into(),
    })?;
    let outlet = holler_edge_database::repo::get_outlet(db.connection(), &state.outlet_id)?;
    Ok(OutletIdentity {
        // An outlet row that is missing is a broken bootstrap, not a reason to
        // fail the screen: the till still sells. The header falls back to the
        // product name alone rather than printing the id, which is a UUID and
        // never belongs on a screen a human reads.
        name: outlet.map(|o| o.name),
        outlet_id: state.outlet_id.clone(),
    })
}

#[tauri::command]
pub fn get_outlet_identity(state: State<'_, AppState>) -> AppResult<OutletIdentity> {
    get_outlet_identity_impl(&state)
}
