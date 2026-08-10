//! Menu reads for the POS grid (docs/spec/ordering.md "CENTER menu grid").
//!
//! `edge/database`'s public `repo` module now exposes
//! `list_menu_categories_for_outlet` alongside `list_menu_items_for_outlet`
//! — `list_menu_categories` below closes the M1 backlog item ("POS renders
//! categories by raw UUID", docs/backlog-m2.md). `variant`/`modifier` reads
//! are still not needed by any screen this task owns.

use tauri::State;

use crate::dto::{MenuCategory, MenuItem};
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

pub fn list_menu_categories_impl(state: &AppState) -> AppResult<Vec<MenuCategory>> {
    let db = state.db.lock().map_err(|_| AppError {
        code: "LOCK_POISONED",
        message: "database lock poisoned".into(),
    })?;

    let categories = holler_edge_database::repo::list_menu_categories_for_outlet(
        db.connection(),
        &state.outlet_id,
    )?;
    Ok(categories.into_iter().map(MenuCategory::from).collect())
}

#[tauri::command]
pub fn list_menu_items(state: State<'_, AppState>) -> AppResult<Vec<MenuItem>> {
    list_menu_items_impl(&state)
}

#[tauri::command]
pub fn list_menu_categories(state: State<'_, AppState>) -> AppResult<Vec<MenuCategory>> {
    list_menu_categories_impl(&state)
}
