//! Order lifecycle commands (docs/spec/ordering.md). Every write goes
//! through `holler_edge_database::Db::create_order_with_outbox`, the only
//! entry point that crate exposes for creating an order — it writes the
//! order row, its items and the `local_outbox` row in one SQLite transaction
//! (ADR-007), so there is no path here that can produce an order without its
//! outbox entry.
//!
//! Add-item/remove-item on an already-persisted DRAFT order are wired
//! through `Db::add_order_item_with_outbox`/`Db::remove_order_item_with_outbox`:
//! both reject a non-DRAFT order with `ORDER_NOT_DRAFT` rather than a silent
//! no-op, matching `confirm_order`'s enforcement.
//!
//! `get_active_draft_order` is the other half of "POS cart persistence"
//! (docs/backlog-m2.md, reopened 2026-08-10): the frontend cart now writes
//! every line through as it happens rather than buffering in memory until
//! Send, and this command is what lets a freshly (re)started app recover
//! whatever was durable at the moment it stopped.
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

/// Recovers the active in-progress order for *this device* — the crash
/// recovery entry point (docs/backlog-m2.md "POS cart persistence",
/// reopened 2026-08-10: the acceptance bar is "kill the POS with lines in
/// the cart, reopen it, and the in-progress order is still there", not
/// merely that a command exists). Called once at startup so the frontend
/// can restore its cart from whatever is actually durable in SQLite instead
/// of starting from empty in-memory state.
///
/// Scoped to `state.device_id`, not just `state.outlet_id`: `device_id` is
/// on `holler_edge_database::model::Order` but deliberately not on the wire
/// `CanonicalOrder` (no contract shape carries it), so the filter has to
/// happen here against the raw row, before conversion. Most recent DRAFT
/// order wins if more than one somehow exists for this device — normal
/// operation only ever leaves at most one, since the POS clears its active
/// order id once it is hitherto handed off (Send), but this must not panic
/// or error if that invariant is ever violated.
pub fn get_active_draft_order_impl(state: &AppState) -> AppResult<Option<CanonicalOrder>> {
    let db = lock_db(state)?;
    let orders =
        holler_edge_database::repo::list_orders_for_outlet(db.connection(), &state.outlet_id)?;
    let draft = orders
        .into_iter()
        .find(|o| o.device_id == state.device_id && o.status == "DRAFT");
    let order = match draft {
        Some(o) => o,
        None => return Ok(None),
    };
    let items = holler_edge_database::repo::list_order_items(db.connection(), &order.id)?;
    Ok(Some(CanonicalOrder::from_order_and_items(order, items)))
}

/// Adds one line item to an already-persisted `DRAFT` order (see
/// `Db::add_order_item_with_outbox`). Milestone 1/2 scope: this app's cart
/// carries no modifiers yet, so `modifiers` is always empty here — the crate
/// still recomputes `line_total_paise` itself rather than trusting a caller
/// value either way.
pub fn add_order_item_impl(
    state: &AppState,
    order_id: &str,
    item: NewOrderItemRequest,
) -> AppResult<CanonicalOrder> {
    if item.quantity <= 0 {
        return Err(AppError {
            code: "INVALID_QUANTITY",
            message: "quantity must be a positive integer".into(),
        });
    }

    let now = now_iso();
    let new_item = holler_edge_database::model::NewOrderItem {
        id: new_id(),
        order_id: order_id.to_string(),
        menu_item_id: item.menu_item_id,
        variant_id: item.variant_id,
        quantity: item.quantity,
        unit_price_paise: item.unit_price_paise,
        // Recomputed inside the crate from unit_price_paise/quantity/
        // modifiers — this value is never trusted, but the field must be
        // populated to construct the struct.
        line_total_paise: item.unit_price_paise * item.quantity,
        notes: item.notes,
        created_at: now.clone(),
    };
    let meta = holler_edge_database::model::OrderItemAddedMeta {
        outbox_id: new_id(),
        occurred_at: now,
    };

    let mut db = lock_db(state)?;
    db.add_order_item_with_outbox(&new_item, &[], &meta)?;

    let order = db.get_order(order_id)?.ok_or_else(|| AppError {
        code: "NOT_FOUND",
        message: format!("order {order_id} not found after add-item"),
    })?;
    let items = holler_edge_database::repo::list_order_items(db.connection(), order_id)?;
    Ok(CanonicalOrder::from_order_and_items(order, items))
}

/// Confirms a `DRAFT` order (the cashier's DRAFT -> CONFIRMED transition).
/// A thin boundary over `holler_edge_database::Db::confirm_order_with_outbox`
/// — the transaction, the DRAFT-only enforcement and the derived
/// `OrderConfirmed` outbox payload all live in that crate; this function
/// only sources what the domain cannot: the outbox row's own id and the
/// current moment, both minted locally on the edge (sync.md §50.1 — the
/// edge, not the cloud, is authoritative for `confirmed_at`).
pub fn confirm_order_impl(state: &AppState, order_id: &str) -> AppResult<CanonicalOrder> {
    let now = now_iso();
    let meta = holler_edge_database::model::OrderConfirmedMeta {
        outbox_id: new_id(),
        occurred_at: now.clone(),
        confirmed_at: now,
    };

    let mut db = lock_db(state)?;
    db.confirm_order_with_outbox(order_id, &meta)?;

    let order = db.get_order(order_id)?.ok_or_else(|| AppError {
        code: "NOT_FOUND",
        message: format!("order {order_id} not found after confirm"),
    })?;
    let items = holler_edge_database::repo::list_order_items(db.connection(), order_id)?;
    Ok(CanonicalOrder::from_order_and_items(order, items))
}

/// Removes one line item from an already-persisted `DRAFT` order (see
/// `Db::remove_order_item_with_outbox`). `order_id` is caller-supplied
/// (rather than derived from the deleted row) purely so this function can
/// re-fetch the order afterwards for the return value — the crate itself
/// resolves and enforces the owning order's status independently.
pub fn remove_order_item_impl(
    state: &AppState,
    order_id: &str,
    order_item_id: &str,
) -> AppResult<CanonicalOrder> {
    let meta = holler_edge_database::model::OrderItemRemovedMeta {
        outbox_id: new_id(),
        occurred_at: now_iso(),
    };

    let mut db = lock_db(state)?;
    db.remove_order_item_with_outbox(order_item_id, &meta)?;

    let order = db.get_order(order_id)?.ok_or_else(|| AppError {
        code: "NOT_FOUND",
        message: format!("order {order_id} not found after remove-item"),
    })?;
    let items = holler_edge_database::repo::list_order_items(db.connection(), order_id)?;
    Ok(CanonicalOrder::from_order_and_items(order, items))
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
pub fn get_active_draft_order(state: State<'_, AppState>) -> AppResult<Option<CanonicalOrder>> {
    get_active_draft_order_impl(&state)
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
pub fn confirm_order(state: State<'_, AppState>, order_id: String) -> AppResult<CanonicalOrder> {
    confirm_order_impl(&state, &order_id)
}

#[tauri::command]
pub fn remove_order_item(
    state: State<'_, AppState>,
    order_id: String,
    order_item_id: String,
) -> AppResult<CanonicalOrder> {
    remove_order_item_impl(&state, &order_id, &order_item_id)
}
