//! Maps a `local_outbox` row to the contracted ingest route + payload
//! (`packages/contracts/openapi/openapi.yaml`).
//!
//! `local_outbox.payload_json` "matches src/types/events.ts / go/events.go"
//! per the schema comment (`0001_init.sql`) — i.e. it is an `OutboxEvent`
//! envelope: `{ event_id, event_type, occurred_at, outlet_id, schema_version,
//! data }`. `events.ts` is explicit that only the M0–M2 slice is defined
//! there and grows per-milestone; this module's `event_type` set (beyond the
//! two already frozen in `events.ts` — `OrderCreated`, `ItemAdded`) is this
//! crate's documented extension for the M1 command/table-session routes the
//! task requires (`SentToKitchen`, `OrderCancelled`, `TableSessionOpened`,
//! `TableSessionUpdated`). If a future contracts revision names these event
//! types differently, this is the only module that needs to change.

use serde_json::Value;

use crate::error::SyncError;

/// An outbound HTTP call this worker still needs to make.
#[derive(Debug)]
pub struct RouteCall {
    /// Path relative to the configured base URL, e.g. `/orders`.
    pub path: String,
    /// The envelope's `payload` field, extracted from the outbox event.
    pub payload: Value,
}

fn malformed(outbox_id: &str, reason: impl Into<String>) -> SyncError {
    SyncError::MalformedPayload {
        outbox_id: outbox_id.to_string(),
        reason: reason.into(),
    }
}

fn data_field<'a>(outbox_id: &str, event: &'a Value, key: &str) -> Result<&'a Value, SyncError> {
    event
        .get("data")
        .and_then(|d| d.get(key))
        .ok_or_else(|| malformed(outbox_id, format!("missing data.{key}")))
}

/// Resolves `(aggregate_type, event_type, aggregate_id, outlet_id, event_json)`
/// to the route + payload to send. `outlet_id` is required for table-session
/// routes, which are outlet-scoped in the OpenAPI path.
pub fn resolve(
    outbox_id: &str,
    aggregate_type: &str,
    event_type: &str,
    aggregate_id: &str,
    outlet_id: &str,
    event_json: &Value,
) -> Result<RouteCall, SyncError> {
    match (aggregate_type, event_type) {
        ("order", "OrderCreated") => {
            let order = data_field(outbox_id, event_json, "order")?.clone();
            Ok(RouteCall {
                path: "/orders".to_string(),
                payload: order,
            })
        }
        ("order", "ItemAdded") => {
            let item = data_field(outbox_id, event_json, "item")?.clone();
            Ok(RouteCall {
                path: format!("/orders/{aggregate_id}/items"),
                payload: item,
            })
        }
        ("order", "SentToKitchen") => {
            let payload = event_json
                .get("data")
                .cloned()
                .unwrap_or_else(|| Value::Object(Default::default()));
            Ok(RouteCall {
                path: format!("/orders/{aggregate_id}/send-to-kitchen"),
                payload,
            })
        }
        ("order", "OrderCancelled") => {
            let reason = data_field(outbox_id, event_json, "reason")?.clone();
            Ok(RouteCall {
                path: format!("/orders/{aggregate_id}/cancel"),
                payload: serde_json::json!({ "reason": reason }),
            })
        }
        ("table_session", "TableSessionOpened") => {
            let session = data_field(outbox_id, event_json, "session")?.clone();
            Ok(RouteCall {
                path: format!("/outlets/{outlet_id}/table-sessions"),
                payload: session,
            })
        }
        ("table_session", _) => {
            // Any other table_session event (state transition, close, ...)
            // replays against the single-session route.
            let session = data_field(outbox_id, event_json, "session")?.clone();
            Ok(RouteCall {
                path: format!("/outlets/{outlet_id}/table-sessions/{aggregate_id}"),
                payload: session,
            })
        }
        _ => Err(SyncError::UnroutedEvent {
            aggregate_type: aggregate_type.to_string(),
            event_type: event_type.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_created_routes_to_orders_root() {
        let event = serde_json::json!({
            "event_type": "OrderCreated",
            "data": { "order": { "holler_order_id": "abc" } }
        });
        let call = resolve("ob1", "order", "OrderCreated", "abc", "outlet-1", &event).unwrap();
        assert_eq!(call.path, "/orders");
        assert_eq!(call.payload["holler_order_id"], "abc");
    }

    #[test]
    fn item_added_routes_to_order_items() {
        let event = serde_json::json!({
            "event_type": "ItemAdded",
            "data": { "order_id": "order-1", "item": { "id": "item-1" } }
        });
        let call = resolve("ob1", "order", "ItemAdded", "order-1", "outlet-1", &event).unwrap();
        assert_eq!(call.path, "/orders/order-1/items");
        assert_eq!(call.payload["id"], "item-1");
    }

    #[test]
    fn kot_has_no_route_yet() {
        let event = serde_json::json!({ "event_type": "KOTCreated", "data": {} });
        let err = resolve("ob1", "kot", "KOTCreated", "kot-1", "outlet-1", &event).unwrap_err();
        assert!(matches!(err, SyncError::UnroutedEvent { .. }));
    }
}
