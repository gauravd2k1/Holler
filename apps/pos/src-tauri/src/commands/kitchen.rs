//! Send-to-kitchen, KOT/order status, and print-queue commands (ADR-014,
//! docs/spec/kitchen.md, docs/spec/hardware-printing.md).
//!
//! Every write here goes through the same crates the rest of this app uses:
//! `holler_edge_database::Db` for the `kot` aggregate (edge-authoritative,
//! append-only per sync.md), and `holler_edge_printer::adapter` for
//! queueing/attempting the print jobs those tickets need. Nothing outside
//! those two crates touches `kot`/`print_job`/`station`/`printer` SQL
//! directly (both crates' own doc comments).

use chrono::Utc;
use tauri::State;

use holler_edge_database::Db;
use holler_edge_device::contract::{Kot as WireKot, KotStatus as WireKotStatus};
use holler_edge_device::Hub;

use crate::dto::{FailedPrintJob, Kot, Station};
use crate::error::{AppError, AppResult};
use crate::ids::{new_id, now_iso};
use crate::state::AppState;

/// Pushes one KOT's current state to every subscribed KDS screen — the same
/// upsert/removed split `edge/device/src/server.rs::handle_command` uses for
/// a KDS-driven transition, applied here for the POS-driven paths (send to
/// kitchen, POS-driven status transitions). A KOT whose status is terminal
/// (SERVED/CANCELLED) is announced as `kot_removed`, never `kot_upserted`
/// (ADR-014 §6 / lan.ts) — it must leave the active set on every screen, not
/// appear to update in place.
///
/// `hub` is `None` when the embedded LAN server never bound (see
/// `state.rs`'s module doc) — this is a silent no-op in that case, by
/// design: a missing KDS screen must never block or fail a kitchen command.
fn notify_kot(hub: Option<&Hub>, outlet_id: &str, kot: &holler_edge_database::model::Kot) {
    let Some(hub) = hub else {
        return;
    };
    let sent_at = now_iso();
    let Some(status) = WireKotStatus::from_db_str(&kot.status) else {
        // Unknown status: fail closed on wire conversion (this is exactly
        // what WireKot::from_db already enforces below), not on notifying.
        return;
    };
    if status.is_terminal() {
        hub.notify_kot_removed(outlet_id, &kot.id, &sent_at);
        return;
    }
    match WireKot::from_db(kot) {
        Ok(wire) => hub.notify_kot_upserted(outlet_id, &wire, &sent_at),
        Err(e) => eprintln!("holler-pos: could not convert kot {} for LAN notify: {e}", kot.id),
    }
}

fn lock_db(state: &AppState) -> AppResult<std::sync::MutexGuard<'_, Db>> {
    state.db.lock().map_err(|_| AppError {
        code: "LOCK_POISONED",
        message: "database lock poisoned".into(),
    })
}

fn to_kot_dtos(kots: Vec<holler_edge_database::model::Kot>) -> AppResult<Vec<Kot>> {
    kots.into_iter()
        .map(|k| {
            Kot::try_from(k).map_err(|e| AppError {
                code: "SERIALIZATION_ERROR",
                message: format!("malformed kot.items_json: {e}"),
            })
        })
        .collect()
}

/// Resolves the human-facing context a print template needs for one order:
/// its display number (this app has no separate short order number yet —
/// M1/M2 identify an order by its own id, so that id is what prints) and its
/// table label, if any. Read-only, best-effort: a table lookup miss leaves
/// `table_label` `None` rather than failing the whole print attempt.
fn build_order_ctx(
    db: &Db,
    order_id: &str,
) -> holler_edge_printer::PrinterResult<holler_edge_printer::adapter::KotOrderContext> {
    let order = db
        .get_order(order_id)
        .map_err(holler_edge_printer::PrinterError::Db)?
        .ok_or(holler_edge_printer::PrinterError::NotFound(
            "order not found while building print context",
        ))?;

    let table_label = match &order.table_id {
        Some(table_id) => {
            let tables = holler_edge_database::repo::list_restaurant_tables(
                db.connection(),
                &order.outlet_id,
            )
            .map_err(holler_edge_printer::PrinterError::Db)?;
            tables
                .into_iter()
                .find(|t| &t.id == table_id)
                .map(|t| format!("{} / {}", t.section, t.label))
        }
        None => None,
    };

    Ok(holler_edge_printer::adapter::KotOrderContext {
        order_display_number: order.id,
        table_label,
    })
}

/// Sends an order to the kitchen: generates the station tickets
/// (`Db::send_order_to_kitchen_with_outbox`), then queues each one for print
/// and makes one immediate best-effort attempt to actually print it
/// (`adapter::queue_kot_for_print` / `adapter::sweep_due_jobs`) so a cashier
/// sees a failure right away rather than only on the next scheduled sweep.
/// A print-queue/print-attempt failure never rolls back or hides the KOTs
/// themselves — the tickets are already committed and the kitchen is the
/// priority; the failure surfaces separately through `list_failed_print_jobs`
/// (docs/spec/hardware-printing.md: "Print failures must be visible to
/// staff").
pub fn send_order_to_kitchen_impl(state: &AppState, order_id: &str) -> AppResult<Vec<Kot>> {
    let meta = holler_edge_database::model::SendToKitchenMeta {
        device_id: state.device_id.clone(),
        occurred_at: now_iso(),
    };

    let mut db = lock_db(state)?;
    let kots = db.send_order_to_kitchen_with_outbox(order_id, &meta)?;

    let now = now_iso();
    for kot in &kots {
        // A station with no active printer routed is a config gap, not a
        // reason to hide the ticket from the kitchen screen — logged, not
        // propagated as a command failure.
        if let Err(e) =
            holler_edge_printer::adapter::queue_kot_for_print(db.connection(), &state.outlet_id, &kot.id, &now, new_id)
        {
            eprintln!("failed to queue KOT {} for print: {e}", kot.id);
        }
    }

    if let Err(e) = holler_edge_printer::adapter::sweep_due_jobs(db.connection(), Utc::now(), |oid| {
        build_order_ctx(&db, oid)
    }) {
        eprintln!("print sweep after send-to-kitchen failed: {e}");
    }
    drop(db);

    // New tickets: every one just created is, by construction, freshly
    // active (never terminal), so this always reaches KDS screens as
    // `kot_upserted` — the case a cashier pressing send-to-kitchen exists to
    // produce.
    for kot in &kots {
        notify_kot(state.hub.as_deref(), &state.outlet_id, kot);
    }

    to_kot_dtos(kots)
}

/// All KOTs for one order, oldest sequence first — what the POS shows the
/// cashier as "which stations this order routed to" and each ticket's
/// status.
pub fn list_kots_for_order_impl(state: &AppState, order_id: &str) -> AppResult<Vec<Kot>> {
    let db = lock_db(state)?;
    let kots = db.list_kots_for_order(order_id)?;
    to_kot_dtos(kots)
}

/// Transitions one KOT's status and returns the order's refreshed ticket
/// list (`order_id` is caller-supplied because `holler_edge_database`
/// exposes no single-KOT read by id — the POS already has the order's KOTs
/// on screen when it offers this action).
pub fn transition_kot_status_impl(
    state: &AppState,
    order_id: &str,
    kot_id: &str,
    new_status: &str,
) -> AppResult<Vec<Kot>> {
    let meta = holler_edge_database::model::KotTransitionMeta {
        status_history_id: new_id(),
        outbox_id: new_id(),
        changed_by_device_id: state.device_id.clone(),
        occurred_at: now_iso(),
    };

    let mut db = lock_db(state)?;
    db.transition_kot_status_with_outbox(kot_id, new_status, &meta)?;

    let kots = db.list_kots_for_order(order_id)?;
    drop(db);

    // A POS-driven transition must reach every KDS screen exactly like a
    // KDS-driven one does (edge/device/src/server.rs::handle_command) — a
    // screen showing a stale status after another station/device moved it
    // on is the failure mode this exists to close. Terminal statuses
    // (SERVED/CANCELLED) fall out as `kot_removed` inside `notify_kot`.
    if let Some(kot) = kots.iter().find(|k| k.id == kot_id) {
        notify_kot(state.hub.as_deref(), &state.outlet_id, kot);
    }

    to_kot_dtos(kots)
}

/// Stations configured for this outlet — used to label a KOT's `station`
/// code with its human-facing name.
pub fn list_stations_impl(state: &AppState) -> AppResult<Vec<Station>> {
    let db = lock_db(state)?;
    let stations = db.list_stations_for_outlet(&state.outlet_id)?;
    Ok(stations.into_iter().map(Station::from).collect())
}

/// Print jobs currently `FAILED` — the staff-visible failure view
/// (docs/spec/hardware-printing.md). Not paged/filtered: Milestone 2 scope
/// is "make it noticeable", and an outlet's failed-print count is never
/// large enough to need paging.
pub fn list_failed_print_jobs_impl(state: &AppState) -> AppResult<Vec<FailedPrintJob>> {
    let db = lock_db(state)?;
    let failed = holler_edge_printer::spool::list_failed_jobs(db.connection())?;
    Ok(failed.into_iter().map(FailedPrintJob::from).collect())
}

/// Manually re-attempts every job currently due (queued, or failed and past
/// its backoff window) — the staff-facing "retry" action next to the
/// failure banner. Jobs still inside their backoff window are left alone;
/// this does not reset or bypass backoff.
pub fn retry_failed_print_jobs_impl(state: &AppState) -> AppResult<Vec<FailedPrintJob>> {
    let db = lock_db(state)?;
    holler_edge_printer::adapter::sweep_due_jobs(db.connection(), Utc::now(), |oid| {
        build_order_ctx(&db, oid)
    })?;
    let failed = holler_edge_printer::spool::list_failed_jobs(db.connection())?;
    Ok(failed.into_iter().map(FailedPrintJob::from).collect())
}

#[tauri::command]
pub fn send_order_to_kitchen(state: State<'_, AppState>, order_id: String) -> AppResult<Vec<Kot>> {
    send_order_to_kitchen_impl(&state, &order_id)
}

#[tauri::command]
pub fn list_kots_for_order(state: State<'_, AppState>, order_id: String) -> AppResult<Vec<Kot>> {
    list_kots_for_order_impl(&state, &order_id)
}

#[tauri::command]
pub fn transition_kot_status(
    state: State<'_, AppState>,
    order_id: String,
    kot_id: String,
    new_status: String,
) -> AppResult<Vec<Kot>> {
    transition_kot_status_impl(&state, &order_id, &kot_id, &new_status)
}

#[tauri::command]
pub fn list_stations(state: State<'_, AppState>) -> AppResult<Vec<Station>> {
    list_stations_impl(&state)
}

#[tauri::command]
pub fn list_failed_print_jobs(state: State<'_, AppState>) -> AppResult<Vec<FailedPrintJob>> {
    list_failed_print_jobs_impl(&state)
}

#[tauri::command]
pub fn retry_failed_print_jobs(state: State<'_, AppState>) -> AppResult<Vec<FailedPrintJob>> {
    retry_failed_print_jobs_impl(&state)
}
