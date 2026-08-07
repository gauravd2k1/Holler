//! Order lifecycle commands (docs/spec/ordering.md). Every write goes
//! through `holler_edge_database::Db::create_order_with_outbox`, the only
//! entry point that crate exposes for creating an order — it writes the
//! order row, its items and the `local_outbox` row in one SQLite transaction
//! (ADR-007), so there is no path here that can produce an order without its
//! outbox entry.
//!
//! Add-item/remove-item on an already-persisted DRAFT order are part of
//! this task's assigned deliverables, but `holler_edge_database` exposes no
//! way to mutate `order_item` rows on an existing order together with an
//! outbox entry: `repo::insert_order_item` is `pub(crate)`-only inside that
//! crate, and there is no delete/remove function for `order_item` at all.
//! Reaching around that (raw SQL, or making the function public from here)
//! would violate "nothing outside this crate touches the SQLite file
//! directly" (edge/database's own doc comment) and this task's directory
//! boundary. These two commands are therefore not implemented; see the task
//! report for the exact gap and the two ways to close it.
//!
//! Each `#[tauri::command]` here is a one-line wrapper around an `*_impl`
//! function that takes `&AppState` directly — the thin-boundary rule
//! (CLAUDE.md) plus it lets integration tests call the exact same logic the
//! IPC layer calls without needing a real Tauri runtime (see `tests/`).

use holler_edge_database::model::NewOutboxEntry;
use tauri::State;

use crate::domain::order::{build_new_draft_order, DraftOrderInput, DraftOrderItemInput};
use crate::dto::CanonicalOrder;
use crate::error::{AppError, AppResult};
use crate::ids::{new_id, now_iso};
use crate::state::AppState;

/// One cart line as submitted by the POS UI. `unit_price_paise` is the
/// snapshot the frontend took from the live menu when the item was added to
/// the cart (ordering.md: line items are never recomputed from the live
/// menu once created).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewOrderItemRequest {
    pub menu_item_id: String,
    pub variant_id: Option<String>,
    pub quantity: i64,
    pub unit_price_paise: i64,
    pub notes: Option<String>,
}

fn lock_db(state: &AppState) -> AppResult<std::sync::MutexGuard<'_, holler_edge_database::Db>> {
    state.db.lock().map_err(|_| AppError {
        code: "LOCK_POISONED",
        message: "database lock poisoned".into(),
    })
}

pub fn create_order_impl(
    state: &AppState,
    order_type: String,
    table_id: Option<String>,
    items: Vec<NewOrderItemRequest>,
) -> AppResult<CanonicalOrder> {
    let input = DraftOrderInput {
        outlet_id: state.outlet_id.clone(),
        device_id: state.device_id.clone(),
        order_type,
        table_id,
        items: items
            .into_iter()
            .map(|i| DraftOrderItemInput {
                menu_item_id: i.menu_item_id,
                variant_id: i.variant_id,
                quantity: i.quantity,
                unit_price_paise: i.unit_price_paise,
                notes: i.notes,
            })
            .collect(),
    };

    let order_id = new_id();
    let item_ids: Vec<String> = input.items.iter().map(|_| new_id()).collect();
    let now = now_iso();

    let (new_order, new_items) =
        build_new_draft_order(order_id.clone(), item_ids, &input, &now).map_err(AppError::from)?;

    let canonical = CanonicalOrder::from_new_order_and_items(&new_order, &new_items);

    let event = serde_json::json!({
        "event_id": new_id(),
        "event_type": "OrderCreated",
        "occurred_at": now,
        "outlet_id": state.outlet_id,
        "schema_version": 1,
        "data": { "order": canonical },
    });
    let payload_json = serde_json::to_string(&event).map_err(|e| AppError {
        code: "SERIALIZATION_ERROR",
        message: e.to_string(),
    })?;

    let outbox = NewOutboxEntry {
        id: new_id(),
        aggregate_type: "order".to_string(),
        aggregate_id: order_id,
        event_type: "OrderCreated".to_string(),
        payload_json,
        created_at: now,
    };

    let mut db = lock_db(state)?;
    db.create_order_with_outbox(&new_order, &new_items, &outbox)?;

    Ok(canonical)
}

pub fn get_order_impl(state: &AppState, order_id: &str) -> AppResult<Option<CanonicalOrder>> {
    let db = lock_db(state)?;
    let order = match db.get_order(order_id)? {
        Some(o) => o,
        None => return Ok(None),
    };
    let items = holler_edge_database::repo::list_order_items(db.connection(), order_id)?;
    Ok(Some(CanonicalOrder::from_order_and_items(order, items)))
}

pub fn list_orders_impl(state: &AppState) -> AppResult<Vec<CanonicalOrder>> {
    let db = lock_db(state)?;
    let orders =
        holler_edge_database::repo::list_orders_for_outlet(db.connection(), &state.outlet_id)?;
    let mut out = Vec::with_capacity(orders.len());
    for order in orders {
        let items = holler_edge_database::repo::list_order_items(db.connection(), &order.id)?;
        out.push(CanonicalOrder::from_order_and_items(order, items));
    }
    Ok(out)
}

/// Not implemented — see module doc comment. Returns a typed, honest error
/// rather than silently no-op'ing or panicking.
pub fn add_order_item_impl(
    _state: &AppState,
    _order_id: &str,
    _item: NewOrderItemRequest,
) -> AppResult<CanonicalOrder> {
    Err(AppError {
        code: "UNSUPPORTED_DB_OPERATION",
        message:
            "holler_edge_database exposes no add-item-with-outbox API for an existing order; see task report"
                .to_string(),
    })
}

/// Not implemented — see module doc comment.
pub fn remove_order_item_impl(
    _state: &AppState,
    _order_id: &str,
    _order_item_id: &str,
) -> AppResult<CanonicalOrder> {
    Err(AppError {
        code: "UNSUPPORTED_DB_OPERATION",
        message:
            "holler_edge_database exposes no remove-item-with-outbox API for an existing order; see task report"
                .to_string(),
    })
}

#[tauri::command]
pub fn create_order(
    state: State<'_, AppState>,
    order_type: String,
    table_id: Option<String>,
    items: Vec<NewOrderItemRequest>,
) -> AppResult<CanonicalOrder> {
    create_order_impl(&state, order_type, table_id, items)
}

#[tauri::command]
pub fn get_order(
    state: State<'_, AppState>,
    order_id: String,
) -> AppResult<Option<CanonicalOrder>> {
    get_order_impl(&state, &order_id)
}

#[tauri::command]
pub fn list_orders(state: State<'_, AppState>) -> AppResult<Vec<CanonicalOrder>> {
    list_orders_impl(&state)
}

#[tauri::command]
pub fn add_order_item(
    state: State<'_, AppState>,
    order_id: String,
    item: NewOrderItemRequest,
) -> AppResult<CanonicalOrder> {
    add_order_item_impl(&state, &order_id, item)
}

#[tauri::command]
pub fn remove_order_item(
    state: State<'_, AppState>,
    order_id: String,
    order_item_id: String,
) -> AppResult<CanonicalOrder> {
    remove_order_item_impl(&state, &order_id, &order_item_id)
}
