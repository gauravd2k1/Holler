//! The cloud→edge pull for `aggregator_order` (M6 Phase C, C1).
//!
//! WHAT THIS EXISTS TO MAKE TRUE. ADR-022's published guarantee is that **an
//! aggregator order that has already arrived is fully operable offline** — the
//! till bills, prints and closes it with no uplink. That guarantee is worth
//! nothing unless the document reaches the till in the first place, and
//! `aggregator_order` is the first non-config aggregate to travel cloud→edge.
//!
//! DELIBERATELY NOT A GENERAL DOWN-SYNC FRAMEWORK. One aggregate, one route,
//! one cursor. A framework built for one caller is a framework shaped by one
//! caller, and the second aggregate to need this will have requirements this
//! one cannot see.
//!
//! THE EDGE NEVER WRITES `aggregator_order` BACK. It is a read-only mirror
//! here: the edge creates its own `order` from a document, linked by
//! `external_order_id`, and that order is edge-authoritative and syncs up like
//! every other one. An edge write path to this table would be split authority
//! and is forbidden (ADR-022).

use holler_edge_database::{model, repo, Db};

use crate::client::HttpClient;
use crate::error::SyncResult;

/// One page of documents, as the cloud serves them.
#[derive(serde::Deserialize)]
struct AggregatorOrderPage {
    orders: Vec<WireDocument>,
    /// Absent on the last page. The cursor advances ONLY over what was applied.
    next_cursor: Option<String>,
}

#[derive(serde::Deserialize)]
struct WireDocument {
    id: String,
    tenant_id: String,
    outlet_id: String,
    platform: String,
    external_order_id: String,
    platform_status: String,
    document_version: i64,
    /// Kept whole, stored as text. The edge does not interpret a platform's
    /// payload — that is the adapter's job, and it happens at the cloud.
    raw_payload: serde_json::Value,
    stated_total_paise: Option<i64>,
    received_at: String,
    business_date: String,
    lines: Vec<WireLine>,
}

#[derive(serde::Deserialize)]
struct WireLine {
    id: String,
    line_number: i64,
    external_item_id: String,
    external_item_name: String,
    /// NULL when nothing local matched. Load-bearing (ADR-022 rule 4).
    menu_item_id: Option<String>,
    quantity: i64,
    stated_unit_price_paise: Option<i64>,
}

/// How many documents to ask for at once. Bounded so one pull cannot walk an
/// unbounded backlog inside a drain that has a deadline.
const PAGE_LIMIT: usize = 100;

/// Pulls documents newer than this outlet's cursor and applies them.
///
/// Returns how many were actually applied. Zero is the ordinary answer and is
/// not a failure: most drains find nothing new.
///
/// IDEMPOTENT BY ID AND REPLACE-NOT-MERGE ON VERSION. Re-applying a page is
/// harmless, which is what makes it safe for a decode failure to reset the
/// cursor and start again.
pub fn pull_and_apply_aggregator_orders(
    db: &mut Db,
    client: &HttpClient,
    outlet_id: &str,
) -> SyncResult<usize> {
    let cursor = repo::get_aggregator_pull_cursor(db.connection(), outlet_id)?;

    let path = match cursor.as_deref() {
        Some(c) => format!("/sync/aggregator-orders?limit={PAGE_LIMIT}&cursor={c}"),
        None => format!("/sync/aggregator-orders?limit={PAGE_LIMIT}"),
    };
    let page: AggregatorOrderPage = client.get_json(&path)?;

    let mut applied = 0usize;
    for doc in &page.orders {
        let model_doc = model::AggregatorOrder {
            id: doc.id.clone(),
            tenant_id: doc.tenant_id.clone(),
            outlet_id: doc.outlet_id.clone(),
            platform: doc.platform.clone(),
            external_order_id: doc.external_order_id.clone(),
            platform_status: doc.platform_status.clone(),
            document_version: doc.document_version,
            raw_payload: doc.raw_payload.to_string(),
            stated_total_paise: doc.stated_total_paise,
            received_at: doc.received_at.clone(),
            business_date: doc.business_date.clone(),
            created_at: doc.received_at.clone(),
            updated_at: doc.received_at.clone(),
        };
        let lines: Vec<model::AggregatorOrderLine> = doc
            .lines
            .iter()
            .map(|l| model::AggregatorOrderLine {
                id: l.id.clone(),
                aggregator_order_id: doc.id.clone(),
                line_number: l.line_number,
                external_item_id: l.external_item_id.clone(),
                external_item_name: l.external_item_name.clone(),
                menu_item_id: l.menu_item_id.clone(),
                quantity: l.quantity,
                stated_unit_price_paise: l.stated_unit_price_paise,
            })
            .collect();

        if repo::apply_aggregator_order(db.connection(), &model_doc, &lines)? {
            applied += 1;
        }
    }

    // THE CURSOR MOVES ONLY AFTER THE PAGE IS WRITTEN, AND ONLY IF THE CLOUD
    // GAVE US ONE. Advancing ahead of what was applied is how a document is
    // skipped permanently and silently -- the failure the ranged streams'
    // machinery exists to prevent, on the other side of the same wire.
    //
    // Note it advances even when `applied` is zero for the page: a page of
    // documents the edge already had at an equal-or-newer version is still
    // progress, and refusing to move past it would wedge the pull forever on a
    // page it can never "apply".
    if let Some(next) = page.next_cursor.as_deref() {
        repo::set_aggregator_pull_cursor(db.connection(), outlet_id, next)?;
    }

    Ok(applied)
}
