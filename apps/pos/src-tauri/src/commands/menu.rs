//! Menu reads for the POS grid (docs/spec/ordering.md "CENTER menu grid").
//!
//! `edge/database`'s public `repo` module exposes a list function only for
//! `menu_item` — there is no `list_menu_categories_for_outlet`,
//! `list_menu_item_variants_for_item` or `list_menu_item_modifiers_for_item`
//! (or any outlet-scoped equivalent). Categories/variants/modifiers cannot
//! be read through this crate's owned code without either reaching past
//! `edge/database`'s API (forbidden — it owns all SQLite access) or adding
//! functions to a directory this task does not own. This is reported as a
//! contract/API gap rather than worked around.

use tauri::State;

use crate::dto::MenuItem;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub fn list_menu_items_impl(state: &AppState) -> AppResult<Vec<MenuItem>> {
    let db = state.db.lock().map_err(|_| AppError {
        code: "LOCK_POISONED",
        message: "database lock poisoned".into(),
    })?;

    let items =
        holler_edge_database::repo::list_menu_items_for_outlet(db.connection(), &state.outlet_id)?;
    Ok(items.into_iter().map(MenuItem::from).collect())
}

#[tauri::command]
pub fn list_menu_items(state: State<'_, AppState>) -> AppResult<Vec<MenuItem>> {
    list_menu_items_impl(&state)
}
