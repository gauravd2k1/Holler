//! Holler POS Tauri Rust core (Milestone 1). Wires the React/TS frontend
//! shell in `apps/pos/src` to `edge/database` through a thin command layer;
//! everything the cashier needs to create a restaurant order works fully
//! offline (ADR-002, ADR-011, sync.md §50.1).

pub mod captain;
pub mod commands;
pub mod domain;
pub mod dto;
pub mod error;
pub mod ids;
pub mod state;

use state::{periodic_drain_interval, run_periodic_drain_loop, AppState, SHUTDOWN_DRAIN_BUDGET};
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

            // M6 A5: THE PERIODIC SYNC PUMP.
            //
            // Before this, `drain_outbox` had exactly two callers -- startup
            // and `RunEvent::Exit` -- so a till that exited abnormally never
            // drained at all, and the day's orders waited for whenever
            // somebody next launched the application. A power cut at an
            // outlet is not an edge case; it is Tuesday.
            //
            // The timer calls the SAME bounded drain, so nothing about
            // classification, blocking, the retry budget or the deadline is
            // restated here -- restating any of it is how the three block
            // mechanisms ADR-023 records came to differ. This function's
            // whole job is WHEN, never WHAT.
            let pump_handle = app.handle().clone();
            let interval = periodic_drain_interval();
            std::thread::Builder::new()
                .name("holler-sync-pump".to_string())
                .spawn(move || {
                    let stop = pump_handle.state::<AppState>().inner().pump_stop_flag();
                    run_periodic_drain_loop(&stop, interval, || {
                        let state: &AppState = pump_handle.state::<AppState>().inner();
                        state.drain_outbox("periodic", SHUTDOWN_DRAIN_BUDGET);
                    });
                })
                // A till whose pump thread will not spawn still sells, prints
                // and bills; it just syncs at both ends of the day as it did
                // before A5. Loud, and NOT fatal -- the ADR-020 rule that a
                // sync failure never takes down the POS. The handle is
                // dropped deliberately: the thread runs until the stop flag
                // is set, and joining it on exit would trade a till that
                // will not close for a thread that is about to be reaped.
                .map(drop)
                .unwrap_or_else(|e| {
                    eprintln!(
                        "holler-pos: could not start the periodic sync pump ({e});                          this till will drain at startup and shutdown only"
                    );
                });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::auth::login,
            commands::aggregator::list_unaccepted_aggregator_orders,
            commands::aggregator::accept_aggregator_order,
            commands::menu::list_menu_items,
            commands::menu::list_menu_categories,
            commands::menu::list_menu_item_modifiers,
            commands::menu::list_menu_item_variants,
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
            commands::kitchen::cancel_kitchen_items,
            commands::kitchen::list_kots_for_order,
            commands::kitchen::transition_kot_status,
            commands::kitchen::list_stations,
            commands::kitchen::list_failed_print_jobs,
            commands::kitchen::retry_failed_print_jobs,
            commands::kitchen::print_invoice,
            commands::billing::issue_invoice,
            commands::billing::issue_split_invoices,
            commands::billing::list_invoices_for_order,
            commands::billing::list_invoices_for_split_group,
            commands::billing::list_discount_definitions,
            commands::billing::record_payment,
            commands::billing::list_payments_for_order,
            commands::billing::open_cash_shift,
            commands::billing::close_cash_shift,
            commands::billing::record_paid_in_out,
            commands::billing::get_cash_shift,
            commands::billing::find_open_cash_shift,
            commands::inventory::list_current_stock,
            commands::inventory::list_stock_deduction_gaps,
            commands::inventory::list_blocked_replays,
            commands::inventory::list_blocked_outbox_rows,
            commands::inventory::list_persistently_failing_outbox_rows,
            commands::inventory::record_wastage,
            commands::inventory::open_stock_count,
            commands::inventory::add_or_update_stock_count_line,
            commands::inventory::list_stock_count_lines,
            commands::inventory::get_stock_count,
            commands::inventory::complete_stock_count,
            commands::inventory::get_stock_count_variance_report,
            // Milestone 5 procurement (ADR-019, T4). Receiving, returns
            // and the human-visible gap report; a GRN never blocks on a PO.
            commands::procurement::list_grn_gaps,
            commands::procurement::purchase_order_receipt_progress,
            commands::procurement::weighted_average_cost_paise,
            commands::procurement::grn_entry_intent_echo,
            commands::procurement::record_goods_receipt,
            commands::procurement::record_purchase_return,
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
                // M6 A5: STOP THE PUMP FIRST, before the drain and well
                // before the seal. A pump that starts after
                // `shutdown_in_place` is draining a closed database, and the
                // failure mode is the silent one -- it would find nothing to
                // send and report success.
                state.stop_periodic_drain();
                state.shutdown_lan_server();

                // ADR-020: DRAIN THE OUTBOX BEFORE THE SEAL, NOT AFTER.
                //
                // Ordering is load-bearing and the failure is silent. The
                // drain needs a live database connection; `shutdown_in_place`
                // below seals the file and closes it, so a drain moved after
                // that call finds nothing to send, reports success, and
                // replays nothing forever -- and reads perfectly in review.
                //
                // Bounded on purpose: an outlet closing with no uplink is the
                // NORMAL case, so this attempts, gives up on
                // SHUTDOWN_DRAIN_BUDGET, leaves the rows and lets the process
                // exit. A till that will not close is worse than a day that
                // replays tomorrow morning instead.
                state.drain_outbox("shutdown", SHUTDOWN_DRAIN_BUDGET);
                match state.db.lock() {
                    Ok(mut db) => {
                        if let Err(e) = db.shutdown_in_place() {
                            eprintln!("failed to seal the edge database on exit: {e}");
                        }
                    }
                    Err(_) => {
                        eprintln!("edge database lock poisoned on exit; relying on seal-on-drop");
                    }
                }
            }
        });
}
