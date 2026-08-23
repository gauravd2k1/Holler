//! Order lifecycle commands (docs/spec/ordering.md). Every write goes
//! through `holler_edge_database::Db::create_order_with_outbox`/
//! `Db::create_order_with_outbox_and_modifiers`, the only entry points that
//! crate exposes for creating an order — each writes the order row, its
//! items (and, for the latter, their modifier selections) and the
//! `local_outbox` row in one SQLite transaction (ADR-007), so there is no
//! path here that can produce an order without its outbox entry.
//!
//! Add-item/remove-item on an already-persisted order are wired through
//! `Db::add_order_item_with_outbox`/`Db::remove_order_item_with_outbox`.
//! Removal stays DRAFT-only. Addition is wider — `#132-A`
//! (docs/spec/kitchen.md's #132 -> #132-A change history,
//! docs/m3-planning.md Track B): a line may be added to an order that has
//! already left DRAFT (through CONFIRMED/SENT_TO_KITCHEN/PREPARING), so a
//! cashier can amend an order the kitchen already has. Both reject a
//! terminal-status order with `ORDER_NOT_DRAFT` rather than a silent no-op.
//!
//! `update_order_item_quantity` wraps
//! `Db::update_order_item_quantity_with_outbox` — the single durable write
//! behind the frozen `SET_ORDER_ITEM_QUANTITY` command (contracts 0.4.0,
//! ADR-016). Deliberately not remove-then-add (docs/backlog-m2.md P1,
//! docs/retro.md 2026-08-10): two durable writes with a crash window between
//! them is precisely the loss the durable-cart work eliminated. It shares
//! `add_order_item`'s widened `#132-A` gate with one further restriction: a
//! line some `kot` row has already frozen a snapshot of rejects with
//! `ORDER_ITEM_ALREADY_TICKETED` rather than silently drifting from what the
//! kitchen holds — the message names the sanctioned alternative (`#132-C`
//! cancel, then re-add at the corrected quantity). See
//! `holler_edge_database::DbError::OrderItemAlreadyTicketed`'s doc comment.
//!
//! `get_active_draft_order` is the other half of "POS cart persistence"
//! (docs/backlog-m2.md, reopened 2026-08-10): the frontend cart now writes
//! every line through as it happens rather than buffering in memory until
//! Send, and this command is what lets a freshly (re)started app recover
//! whatever was durable at the moment it stopped.
//!
//! Every order read path (`get_order`/`list_orders`/`get_active_draft_order`
//! and the return value of every mutation below) fills in each line's real
//! `modifiers` from `order_item_modifier` via
//! `holler_edge_database::repo::list_order_item_modifiers_for_order` — this
//! closes the M3 Track B gap where a modifier's `price_delta_paise` reached
//! storage and the outbox event but never came back to a caller after the
//! write (`docs/m3-planning.md` Track B).
//!
//! Each `#[tauri::command]` here is a one-line wrapper around an `*_impl`
//! function that takes `&AppState` directly — the thin-boundary rule
//! (CLAUDE.md) plus it lets integration tests call the exact same logic the
//! IPC layer calls without needing a real Tauri runtime (see `tests/`).

use std::collections::HashMap;

use holler_edge_database::model::{NewOutboxEntry, OrderItemModifier as DbOrderItemModifier};
use tauri::State;

use crate::domain::order::{
    build_new_draft_order, DraftOrderInput, DraftOrderItemInput, DraftOrderItemModifierInput,
};
use crate::dto::CanonicalOrder;
use crate::error::{AppError, AppResult};
use crate::ids::{new_id, now_iso};
use crate::state::AppState;

/// One modifier selection submitted alongside a cart line — mirrors
/// `packages/contracts/src/types/order.ts` `OrderItemModifierSchema` minus
/// the storage-only fields (`id`/`created_at`, minted here;
/// `order_item_id`, implicit from the line it is submitted with).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewOrderItemModifierRequest {
    pub modifier_id: String,
    pub group_name: String,
    pub option_name: String,
    pub price_delta_paise: i64,
}

/// One cart line as submitted by the POS UI. `unit_price_paise` is the
/// snapshot the frontend took from the live menu when the item was added to
/// the cart (ordering.md: line items are never recomputed from the live
/// menu once created). `modifiers` defaults to empty so existing callers
/// that predate Track B keep working unchanged.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewOrderItemRequest {
    pub menu_item_id: String,
    pub variant_id: Option<String>,
    pub quantity: i64,
    pub unit_price_paise: i64,
    pub notes: Option<String>,
    #[serde(default)]
    pub modifiers: Vec<NewOrderItemModifierRequest>,
}

fn lock_db(state: &AppState) -> AppResult<std::sync::MutexGuard<'_, holler_edge_database::Db>> {
    state.db.lock().map_err(|_| AppError {
        code: "LOCK_POISONED",
        message: "database lock poisoned".into(),
    })
}

/// Mints the storage-layer `order_item_modifier` rows for one line's
/// requested modifiers — the boundary where ids/timestamps get generated
/// (this crate's convention: `edge/database` never mints ids for rows a
/// caller can fully describe, only for the cases documented on
/// `SendToKitchenMeta`/`KotTransitionMeta`).
fn build_order_item_modifiers(
    order_item_id: &str,
    requests: Vec<NewOrderItemModifierRequest>,
    created_at: &str,
) -> Vec<DbOrderItemModifier> {
    requests
        .into_iter()
        .map(|m| DbOrderItemModifier {
            id: new_id(),
            order_item_id: order_item_id.to_string(),
            modifier_id: m.modifier_id,
            group_name: m.group_name,
            option_name: m.option_name,
            price_delta_paise: m.price_delta_paise,
            created_at: created_at.to_string(),
        })
        .collect()
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
            .iter()
            .map(|i| DraftOrderItemInput {
                menu_item_id: i.menu_item_id.clone(),
                variant_id: i.variant_id.clone(),
                quantity: i.quantity,
                unit_price_paise: i.unit_price_paise,
                notes: i.notes.clone(),
                modifiers: i
                    .modifiers
                    .iter()
                    .map(|m| DraftOrderItemModifierInput {
                        modifier_id: m.modifier_id.clone(),
                        group_name: m.group_name.clone(),
                        option_name: m.option_name.clone(),
                        price_delta_paise: m.price_delta_paise,
                    })
                    .collect(),
            })
            .collect(),
    };

    let order_id = new_id();
    let item_ids: Vec<String> = input.items.iter().map(|_| new_id()).collect();
    let now = now_iso();

    let (new_order, new_items) =
        build_new_draft_order(order_id.clone(), item_ids, &input, &now).map_err(AppError::from)?;

    // Mint the storage-layer modifier rows for each line, index-aligned with
    // `new_items`/`input.items` (build_new_draft_order preserves order).
    let item_modifiers: Vec<Vec<DbOrderItemModifier>> = new_items
        .iter()
        .zip(items)
        .map(|(new_item, request)| {
            build_order_item_modifiers(&new_item.id, request.modifiers, &now)
        })
        .collect();

    let canonical =
        CanonicalOrder::from_new_order_and_items(&new_order, &new_items, &item_modifiers);

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
        aggregate_id: order_id.clone(),
        event_type: "OrderCreated".to_string(),
        payload_json,
        created_at: now,
    };

    let mut db = lock_db(state)?;
    db.create_order_with_outbox_and_modifiers(&new_order, &new_items, &item_modifiers, &outbox)?;

    // The DTO built above (`canonical`) predates the insert and so cannot
    // carry the `display_number` `Db::create_order_with_outbox_and_modifiers`
    // minted transactionally — re-read the persisted row rather than return
    // a response with a null number a screen would otherwise have to paper
    // over (contracts 0.4.0, ADR-016 §6).
    let persisted_order = db.get_order(&order_id)?.ok_or_else(|| AppError {
        code: "NOT_FOUND",
        message: format!("order {order_id} not found immediately after create"),
    })?;
    let persisted_items = holler_edge_database::repo::list_order_items(db.connection(), &order_id)?;
    let modifiers_map = modifiers_by_item(&db, &order_id)?;
    Ok(CanonicalOrder::from_order_and_items(
        persisted_order,
        persisted_items,
        &modifiers_map,
    ))
}

/// Reads back one order together with every line's real modifiers, keyed by
/// `order_item.id` — the single grouped query every read path in this file
/// uses instead of one `list_order_item_modifiers` call per line.
fn modifiers_by_item(
    db: &holler_edge_database::Db,
    order_id: &str,
) -> AppResult<HashMap<String, Vec<DbOrderItemModifier>>> {
    Ok(holler_edge_database::repo::list_order_item_modifiers_for_order(db.connection(), order_id)?)
}

pub fn get_order_impl(state: &AppState, order_id: &str) -> AppResult<Option<CanonicalOrder>> {
    let db = lock_db(state)?;
    let order = match db.get_order(order_id)? {
        Some(o) => o,
        None => return Ok(None),
    };
    let items = holler_edge_database::repo::list_order_items(db.connection(), order_id)?;
    let modifiers = modifiers_by_item(&db, order_id)?;
    Ok(Some(CanonicalOrder::from_order_and_items(
        order, items, &modifiers,
    )))
}

pub fn list_orders_impl(state: &AppState) -> AppResult<Vec<CanonicalOrder>> {
    let db = lock_db(state)?;
    let orders =
        holler_edge_database::repo::list_orders_for_outlet(db.connection(), &state.outlet_id)?;
    let mut out = Vec::with_capacity(orders.len());
    for order in orders {
        let items = holler_edge_database::repo::list_order_items(db.connection(), &order.id)?;
        let modifiers = modifiers_by_item(&db, &order.id)?;
        out.push(CanonicalOrder::from_order_and_items(
            order, items, &modifiers,
        ));
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
    let modifiers = modifiers_by_item(&db, &order.id)?;
    Ok(Some(CanonicalOrder::from_order_and_items(
        order, items, &modifiers,
    )))
}

/// Adds one line item — with its modifier selections, if any — to an
/// already-persisted order (see `Db::add_order_item_with_outbox`). Legal
/// through DRAFT/CONFIRMED/SENT_TO_KITCHEN/PREPARING (`#132-A`, see the
/// module doc comment); rejected with `ORDER_NOT_DRAFT` once the order is
/// terminal.
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
    let new_item_id = new_id();
    let modifiers = build_order_item_modifiers(&new_item_id, item.modifiers, &now);
    let modifier_delta_sum: i64 = modifiers.iter().map(|m| m.price_delta_paise).sum();
    let new_item = holler_edge_database::model::NewOrderItem {
        id: new_item_id,
        order_id: order_id.to_string(),
        menu_item_id: item.menu_item_id,
        variant_id: item.variant_id,
        quantity: item.quantity,
        unit_price_paise: item.unit_price_paise,
        // Recomputed inside the crate from unit_price_paise/quantity/
        // modifiers — this value is never trusted, but the field must be
        // populated to construct the struct; matching the crate's own money
        // invariant here rather than leaving it visibly wrong in transit.
        line_total_paise: (item.unit_price_paise + modifier_delta_sum) * item.quantity,
        notes: item.notes,
        created_at: now.clone(),
    };
    let meta = holler_edge_database::model::OrderItemAddedMeta {
        outbox_id: new_id(),
        occurred_at: now,
    };

    let mut db = lock_db(state)?;
    db.add_order_item_with_outbox(&new_item, &modifiers, &meta)?;

    let order = db.get_order(order_id)?.ok_or_else(|| AppError {
        code: "NOT_FOUND",
        message: format!("order {order_id} not found after add-item"),
    })?;
    let items = holler_edge_database::repo::list_order_items(db.connection(), order_id)?;
    let modifiers_map = modifiers_by_item(&db, order_id)?;
    Ok(CanonicalOrder::from_order_and_items(
        order,
        items,
        &modifiers_map,
    ))
}

/// Sets an existing order line's `quantity` — the frozen
/// `SET_ORDER_ITEM_QUANTITY` command (contracts 0.4.0, ADR-016), wrapping
/// `Db::update_order_item_quantity_with_outbox`. A **single** durable write:
/// see that method's doc comment for why remove-then-add is deliberately
/// not how this is implemented. Legal in the same statuses as
/// `add_order_item` (`#132-A`).
pub fn update_order_item_quantity_impl(
    state: &AppState,
    order_id: &str,
    order_item_id: &str,
    quantity: i64,
) -> AppResult<CanonicalOrder> {
    if quantity <= 0 {
        return Err(AppError {
            code: "INVALID_QUANTITY",
            message: "quantity must be a positive integer".into(),
        });
    }

    let meta = holler_edge_database::model::OrderItemQuantitySetMeta {
        outbox_id: new_id(),
        occurred_at: now_iso(),
    };

    let mut db = lock_db(state)?;
    db.update_order_item_quantity_with_outbox(order_item_id, quantity, &meta)?;

    let order = db.get_order(order_id)?.ok_or_else(|| AppError {
        code: "NOT_FOUND",
        message: format!("order {order_id} not found after quantity update"),
    })?;
    let items = holler_edge_database::repo::list_order_items(db.connection(), order_id)?;
    let modifiers_map = modifiers_by_item(&db, order_id)?;
    Ok(CanonicalOrder::from_order_and_items(
        order,
        items,
        &modifiers_map,
    ))
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
    let modifiers_map = modifiers_by_item(&db, order_id)?;
    Ok(CanonicalOrder::from_order_and_items(
        order,
        items,
        &modifiers_map,
    ))
}

/// Sets `order_type`/`table_id` on an already-persisted `DRAFT` order — the
/// fix for the M2 P0 regression (docs/retro.md, task T14): a DRAFT order is
/// created on the first cart line (crash durability), and its shape must
/// stay editable for the order's whole DRAFT lifetime, not just before it
/// existed. Rejects with `ORDER_NOT_DRAFT` once the order has left DRAFT —
/// this command stays DRAFT-only (unlike `add_order_item`'s widened
/// `#132-A` gate): correcting order-type/table after the kitchen already has
/// tickets is a different, unbuilt operation, not this one.
///
/// Also best-effort corrects this order's still-unpublished `OrderCreated`
/// `local_outbox` payload in place, so a cloud that has not yet observed
/// this order sees the corrected shape rather than whatever it was at the
/// first tap. See `holler_edge_database::Db::update_order_shape_with_outbox`'s
/// doc comment for the reasoning and the residual gap it leaves (an
/// already-*published* `OrderCreated` event is not retroactively
/// correctable without a new contract event — out of this crate's
/// authority).
pub fn update_order_shape_impl(
    state: &AppState,
    order_id: &str,
    order_type: String,
    table_id: Option<String>,
) -> AppResult<CanonicalOrder> {
    let now = now_iso();
    let mut db = lock_db(state)?;

    let corrected_payload = holler_edge_database::repo::get_unpublished_outbox_payload(
        db.connection(),
        order_id,
        "OrderCreated",
    )?
    .and_then(|payload_json| {
        correct_order_created_payload(&payload_json, &order_type, table_id.as_deref(), &now)
    });

    db.update_order_shape_with_outbox(
        order_id,
        &order_type,
        table_id.as_deref(),
        &now,
        corrected_payload.as_deref(),
    )?;

    let order = db.get_order(order_id)?.ok_or_else(|| AppError {
        code: "NOT_FOUND",
        message: format!("order {order_id} not found after shape update"),
    })?;
    let items = holler_edge_database::repo::list_order_items(db.connection(), order_id)?;
    let modifiers_map = modifiers_by_item(&db, order_id)?;
    Ok(CanonicalOrder::from_order_and_items(
        order,
        items,
        &modifiers_map,
    ))
}

/// Rewrites `data.order.order_type`/`data.order.table_id`/
/// `data.order.timestamps.updated_at` inside an already-serialized
/// `OrderCreated` envelope, leaving `event_id`/`occurred_at`/every other
/// field untouched — a correction of that one still-pending fact, not a
/// fresh event. Returns `None` (and the caller then leaves the queued
/// payload alone) if the stored payload does not parse as the expected
/// shape; a malformed queued payload is a pre-existing data problem this
/// command must not compound by writing something worse over it.
fn correct_order_created_payload(
    payload_json: &str,
    order_type: &str,
    table_id: Option<&str>,
    updated_at: &str,
) -> Option<String> {
    let mut value: serde_json::Value = serde_json::from_str(payload_json).ok()?;
    let order_value = value.get_mut("data")?.get_mut("order")?;
    let order_object = order_value.as_object_mut()?;
    order_object.insert(
        "order_type".to_string(),
        serde_json::Value::String(order_type.to_string()),
    );
    order_object.insert(
        "table_id".to_string(),
        table_id.map_or(serde_json::Value::Null, |t| {
            serde_json::Value::String(t.to_string())
        }),
    );
    if let Some(timestamps) = order_object
        .get_mut("timestamps")
        .and_then(|t| t.as_object_mut())
    {
        timestamps.insert(
            "updated_at".to_string(),
            serde_json::Value::String(updated_at.to_string()),
        );
    }
    serde_json::to_string(&value).ok()
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
    let modifiers_map = modifiers_by_item(&db, order_id)?;
    Ok(CanonicalOrder::from_order_and_items(
        order,
        items,
        &modifiers_map,
    ))
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

/// The Tauri boundary for `SET_ORDER_ITEM_QUANTITY`. Frontend call shape:
/// `invoke("update_order_item_quantity", { orderId, orderItemId, quantity })`.
#[tauri::command]
pub fn update_order_item_quantity(
    state: State<'_, AppState>,
    order_id: String,
    order_item_id: String,
    quantity: i64,
) -> AppResult<CanonicalOrder> {
    update_order_item_quantity_impl(&state, &order_id, &order_item_id, quantity)
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

#[tauri::command]
pub fn update_order_shape(
    state: State<'_, AppState>,
    order_id: String,
    order_type: String,
    table_id: Option<String>,
) -> AppResult<CanonicalOrder> {
    update_order_shape_impl(&state, &order_id, order_type, table_id)
}
