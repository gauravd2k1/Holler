//! Holler POS Tauri Rust core (Milestone 1). Wires the React/TS frontend
//! shell in `apps/pos/src` to `edge/database` through a thin command layer;
//! everything the cashier needs to create a restaurant order works fully
//! offline (ADR-002, ADR-011, sync.md §50.1).

pub mod commands;
pub mod domain;
pub mod dto;
pub mod error;
pub mod ids;
pub mod state;

use state::AppState;
use tauri::Manager;

/// Builds and runs the Tauri application. Split out of `main.rs` so
/// integration-style tests in this crate can construct the same command set
/// against an in-memory `AppState` without going through a real window.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("app data dir must be resolvable");
            let state = AppState::open(&app_data_dir).unwrap_or_else(|e| {
                panic!(
                    "failed to open edge database: {e} — device is not provisioned; \
                     set HOLLER_OUTLET_ID, HOLLER_DEVICE_ID and HOLLER_DB_KEY_HEX"
                )
            });
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::auth::login,
            commands::menu::list_menu_items,
            commands::tables::list_tables,
            commands::tables::get_open_table_session,
            commands::orders::create_order,
            commands::orders::get_order,
            commands::orders::list_orders,
            commands::orders::add_order_item,
            commands::orders::remove_order_item,
            commands::orders::confirm_order,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Holler POS application");
}
