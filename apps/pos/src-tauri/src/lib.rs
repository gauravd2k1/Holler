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
            commands::menu::list_menu_categories,
            commands::tables::list_tables,
            commands::tables::get_open_table_session,
            commands::orders::create_order,
            commands::orders::get_order,
            commands::orders::list_orders,
            commands::orders::get_active_draft_order,
            commands::orders::add_order_item,
            commands::orders::update_order_item_quantity,
            commands::orders::remove_order_item,
            commands::orders::update_order_shape,
            commands::orders::confirm_order,
            commands::kitchen::send_order_to_kitchen,
            commands::kitchen::list_kots_for_order,
            commands::kitchen::transition_kot_status,
            commands::kitchen::list_stations,
            commands::kitchen::list_failed_print_jobs,
            commands::kitchen::retry_failed_print_jobs,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the Holler POS application")
        .run(|app_handle, event| {
            // Seal the edge database before the process goes away (ADR-011).
            // Without this the decrypted SQLite file — which caches Argon2id
            // credential hashes — is left on disk after every normal exit.
            //
            // `Db` also seals on drop as a fallback, but Tauri does not
            // guarantee managed state is dropped on exit, so the shutdown is
            // driven explicitly here. Both paths are idempotent.
            if let tauri::RunEvent::Exit = event {
                // `inner()` reborrows from the app handle rather than from
                // the temporary `State` guard, so the lock may outlive it.
                let state: &AppState = app_handle.state::<AppState>().inner();
                state.shutdown_lan_server();
                match state.db.lock() {
                    Ok(mut db) => {
                        if let Err(e) = db.shutdown_in_place() {
                            eprintln!("failed to seal the edge database on exit: {e}");
                        }
                    }
                    Err(_) => {
                        eprintln!(
                            "edge database lock poisoned on exit; relying on seal-on-drop"
                        );
                    }
                }
            }
        });
}
