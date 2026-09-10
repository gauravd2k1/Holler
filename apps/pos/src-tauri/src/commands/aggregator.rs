//! M6 Phase C, C1: the accept path for an aggregator order the till already
//! holds.
//!
//! ============================================================================
//! WHY THIS MODULE EXISTS AT ALL, AND WHAT IT REPLACES
//! ============================================================================
//!
//! The cloud->edge down-path has mirrored `aggregator_order` into edge SQLite
//! since the start of Phase C, and `repo::list_unaccepted_aggregator_orders`
//! and `repo::aggregator_order_acceptance` were written alongside it -- with
//! **no non-test caller anywhere in the repository**. A document therefore
//! landed on the till and nothing could turn it into an order, bill it, print
//! it or close it. The sinks were counted and the screens were not, which is
//! the same defect class as a contract column nothing reads: found on
//! 2026-09-10 while trying to observe M6 C1, which had been recorded as CODE
//! COMPLETE.
//!
//! ============================================================================
//! ACCEPTING A DOCUMENT *IS* CREATING THE LOCAL ORDER
//! ============================================================================
//!
//! There is no acceptance flag, here or in storage. `aggregator_order` is a
//! READ-ONLY MIRROR of a cloud-authoritative aggregate (ADR-022), so an
//! edge-written column on it would be exactly the split authority the contract
//! rubric says to fix by splitting the aggregate. The local `order` this
//! creates is edge-authoritative, carries `external_order_id` as the only link
//! between the two, and replays up the ordinary outbox like every other order.
//!
//! **Nothing after this point is new.** Billing, printing and closing are the
//! existing paths, untouched: this module creates a DRAFT order and stops.
//!
//! ============================================================================
//! WHAT IS DELIBERATELY NOT HERE
//! ============================================================================
//!
//! No reject, and no platform-side cancel visibility -- both are filed in
//! `docs/backlog.md` with the trigger "before any platform sandbox access",
//! because their shape is argued from what a real platform does to an order we
//! refused, and C1 does not need either.
//!
//! An UNMAPPED LINE IS NOT AN ERROR AND IS NOT SILENTLY DROPPED. A line whose
//! `menu_item_id` is NULL (no local item matched, which is NORMAL per ADR-022
//! rule 4) cannot become an `order_item` -- that column has a real FK and is
//! NOT NULL. Such lines are skipped and COUNTED, and the count comes back to
//! the caller so a screen can say so. The document keeps its raw payload and
//! all of its lines, so nothing is lost; what is refused is inventing a local
//! item for a platform id we do not recognise.

use holler_edge_database::model::{NewOutboxEntry, OrderItemModifier as DbOrderItemModifier};
use holler_edge_database::Db;
use tauri::State;

use crate::domain::order::{build_new_draft_order, DraftOrderInput, DraftOrderItemInput};
use crate::dto::CanonicalOrder;
use crate::error::{AppError, AppResult};
use crate::ids::{new_id, now_iso};
use crate::state::AppState;

fn lock_db(state: &AppState) -> AppResult<std::sync::MutexGuard<'_, Db>> {
    state.db.lock().map_err(|_| AppError {
        code: "LOCK_POISONED",
        message: "database lock poisoned".into(),
    })
}

/// One line of a document, as the accept screen needs it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AggregatorOrderLineView {
    pub id: String,
    pub line_number: i64,
    pub external_item_id: String,
    pub external_item_name: String,
    /// NULL is normal and the screen must show it as "not matched" rather than
    /// hiding the line.
    pub menu_item_id: Option<String>,
    pub quantity: i64,
    pub stated_unit_price_paise: Option<i64>,
}

/// One document awaiting acceptance, with its lines.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UnacceptedAggregatorOrder {
    pub id: String,
    pub platform: String,
    pub external_order_id: String,
    pub platform_status: String,
    pub document_version: i64,
    pub stated_total_paise: Option<i64>,
    pub received_at: String,
    pub business_date: String,
    pub lines: Vec<AggregatorOrderLineView>,
}

/// What accepting produced: the local order, and what could not be carried.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AcceptedAggregatorOrder {
    pub order: CanonicalOrder,
    pub external_order_id: String,
    /// Lines carried onto the order.
    pub lines_created: usize,
    /// Lines with no local menu item, skipped and reported -- never dropped in
    /// silence.
    pub lines_unmapped: usize,
}

pub fn list_unaccepted_aggregator_orders_impl(
    state: &AppState,
) -> AppResult<Vec<UnacceptedAggregatorOrder>> {
    let db = lock_db(state)?;
    let docs = db.list_unaccepted_aggregator_orders(&state.outlet_id)?;
    let mut out = Vec::with_capacity(docs.len());
    for doc in docs {
        let lines = db.list_aggregator_order_lines(&doc.id)?;
        out.push(UnacceptedAggregatorOrder {
            id: doc.id,
            platform: doc.platform,
            external_order_id: doc.external_order_id,
            platform_status: doc.platform_status,
            document_version: doc.document_version,
            stated_total_paise: doc.stated_total_paise,
            received_at: doc.received_at,
            business_date: doc.business_date,
            lines: lines
                .into_iter()
                .map(|l| AggregatorOrderLineView {
                    id: l.id,
                    line_number: l.line_number,
                    external_item_id: l.external_item_id,
                    external_item_name: l.external_item_name,
                    menu_item_id: l.menu_item_id,
                    quantity: l.quantity,
                    stated_unit_price_paise: l.stated_unit_price_paise,
                })
                .collect(),
        });
    }
    Ok(out)
}

/// `order.source` for an accepted document.
///
/// **`DIRECT`, and the reason is a contract limit, not a preference.** The
/// column's CHECK is a closed set written in contracts 0004 --
/// `POS`/`QR`/`AGGREGATOR_ZOMATO`/`AGGREGATOR_SWIGGY`/`DIRECT` -- and it cannot
/// name ONDC, a local sync-REST platform, or anything else that was not
/// enumerated then. Writing `AGGREGATOR_SWIGGY` for an ONDC order would be a
/// stored value that lies; `POS` would claim a cashier typed it. `DIRECT` is the
/// only member that is not a false claim, and the platform's identity is
/// preserved exactly in `source_payload_json` beside it.
///
/// Widening that CHECK is a frozen-contract change and is escalated, not done
/// here.
const ACCEPTED_ORDER_SOURCE: &str = "DIRECT";

/// `order.order_type` for an accepted document -- this member exists in the
/// 0001 CHECK and means what it says.
const ACCEPTED_ORDER_TYPE: &str = "AGGREGATOR";

pub fn accept_aggregator_order_impl(
    state: &AppState,
    aggregator_order_id: String,
) -> AppResult<AcceptedAggregatorOrder> {
    let mut db = lock_db(state)?;

    // Re-read rather than trusting what a screen was holding: a document the
    // pull has since replaced carries different lines, and the cheapest way to
    // be wrong here is to bill last minute's version of an order.
    let doc = db
        .list_unaccepted_aggregator_orders(&state.outlet_id)?
        .into_iter()
        .find(|d| d.id == aggregator_order_id)
        .ok_or_else(|| AppError {
            code: "AGGREGATOR_ORDER_NOT_PENDING",
            message: format!(
                "no unaccepted aggregator order {aggregator_order_id:?} at this outlet -- it may \
                 already have been accepted"
            ),
        })?;

    let lines = db.list_aggregator_order_lines(&doc.id)?;
    let total_lines = lines.len();

    let mut items = Vec::new();
    for line in &lines {
        let Some(menu_item_id) = line.menu_item_id.clone() else {
            continue;
        };
        items.push(DraftOrderItemInput {
            menu_item_id,
            variant_id: None,
            quantity: line.quantity,
            // The platform's stated price, not the local menu's. An order from a
            // platform is billed at what the platform told the customer;
            // substituting our own price silently changes what was agreed. A
            // line with no stated price falls back to 0 rather than being
            // refused -- the document is the record either way, and a visible
            // zero is better than a rejected order standing in the kitchen.
            unit_price_paise: line.stated_unit_price_paise.unwrap_or(0),
            notes: Some(format!(
                "{} x{} ({})",
                line.external_item_name, line.quantity, line.external_item_id
            )),
            modifiers: vec![],
        });
    }
    let lines_created = items.len();
    let lines_unmapped = total_lines - lines_created;

    if lines_created == 0 {
        return Err(AppError {
            code: "AGGREGATOR_ORDER_NO_MAPPED_LINES",
            message: format!(
                "aggregator order {} has {} line(s) and none matched a local menu item, so there \
                 is nothing to bill. The document is kept; map the items and accept again.",
                doc.external_order_id, total_lines
            ),
        });
    }

    let provenance = serde_json::json!({
        "platform": doc.platform,
        "aggregator_order_id": doc.id,
        "document_version": doc.document_version,
        "platform_status": doc.platform_status,
        "stated_total_paise": doc.stated_total_paise,
        "lines_unmapped": lines_unmapped,
    });

    let input = DraftOrderInput {
        outlet_id: state.outlet_id.clone(),
        device_id: state.device_id.clone(),
        order_type: ACCEPTED_ORDER_TYPE.to_string(),
        table_id: None,
        items,
        source: ACCEPTED_ORDER_SOURCE.to_string(),
        external_order_id: Some(doc.external_order_id.clone()),
        source_payload_json: Some(provenance.to_string()),
    };

    let order_id = new_id();
    let item_ids: Vec<String> = input.items.iter().map(|_| new_id()).collect();
    let now = now_iso();

    let (new_order, new_items) =
        build_new_draft_order(order_id.clone(), item_ids, &input, &now).map_err(AppError::from)?;

    let item_modifiers: Vec<Vec<DbOrderItemModifier>> =
        new_items.iter().map(|_| Vec::new()).collect();

    let draft_dto =
        CanonicalOrder::from_new_order_and_items(&new_order, &new_items, &item_modifiers);

    let event = serde_json::json!({
        "event_id": new_id(),
        "event_type": "OrderCreated",
        "occurred_at": now,
        "outlet_id": state.outlet_id,
        "schema_version": 1,
        "data": { "order": draft_dto },
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

    db.create_order_with_outbox_and_modifiers(&new_order, &new_items, &item_modifiers, &outbox)?;

    // Re-read for the transactionally minted display_number, exactly as
    // `commands::orders::create_order_impl` does and for the same reason.
    let persisted_order = db.get_order(&order_id)?.ok_or_else(|| AppError {
        code: "NOT_FOUND",
        message: format!("order {order_id} not found immediately after create"),
    })?;
    let persisted_items = holler_edge_database::repo::list_order_items(db.connection(), &order_id)?;
    let modifiers_map =
        holler_edge_database::repo::list_order_item_modifiers_for_order(db.connection(), &order_id)?;

    Ok(AcceptedAggregatorOrder {
        order: CanonicalOrder::from_order_and_items(
            persisted_order,
            persisted_items,
            &modifiers_map,
        ),
        external_order_id: doc.external_order_id,
        lines_created,
        lines_unmapped,
    })
}

#[tauri::command]
pub fn list_unaccepted_aggregator_orders(
    state: State<'_, AppState>,
) -> AppResult<Vec<UnacceptedAggregatorOrder>> {
    list_unaccepted_aggregator_orders_impl(&state)
}

#[tauri::command]
pub fn accept_aggregator_order(
    state: State<'_, AppState>,
    aggregator_order_id: String,
) -> AppResult<AcceptedAggregatorOrder> {
    accept_aggregator_order_impl(&state, aggregator_order_id)
}
