//! Maps a `local_outbox` row to the contracted ingest route + payload
//! (`packages/contracts/openapi/openapi.yaml`).
//!
//! `local_outbox.payload_json` "matches src/types/events.ts / go/events.go"
//! per the schema comment (`0001_init.sql`) — i.e. it is an `OutboxEvent`
//! envelope: `{ event_id, event_type, occurred_at, outlet_id, schema_version,
//! data }`. Every `event_type` string this module matches on —
//! `OrderCreated`, `ItemAdded`, `OrderConfirmed`, `SentToKitchen`,
//! `OrderCancelled`, `TableSessionOpened`, `TableSessionUpdated` — is frozen in
//! `packages/contracts` `OUTBOX_EVENT_TYPES` (`src/types/events.ts` /
//! `go/events.go`, contracts 0.2.5). `scripts/check-event-type-drift.mjs`
//! enforces both directions: every literal here must be frozen, and every
//! frozen type must appear here or in that script's `NOT_YET_EMITTED` list.
//!
//! Every arm below matches an explicit event_type literal — deliberately no
//! wildcard for any known aggregate_type. An unrecognized event_type
//! (typo'd or genuinely new) must fail loudly as
//! [`SyncError::UnroutedEvent`] rather than being silently replayed against
//! a route that happens to also fit a different event.

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
        ("order", "OrderConfirmed") => {
            let confirmed_at = data_field(outbox_id, event_json, "confirmed_at")?.clone();
            Ok(RouteCall {
                path: format!("/orders/{aggregate_id}/confirm"),
                payload: serde_json::json!({ "confirmed_at": confirmed_at }),
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
        ("table_session", "TableSessionUpdated") => {
            // Covers every state transition and close: the single-session
            // route re-validates the transition cloud-side (openapi.yaml).
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
    fn order_confirmed_routes_to_confirm_endpoint() {
        let event = serde_json::json!({
            "event_type": "OrderConfirmed",
            "data": { "order_id": "order-1", "confirmed_at": "2026-08-08T10:00:00Z" }
        });
        let call = resolve(
            "ob1",
            "order",
            "OrderConfirmed",
            "order-1",
            "outlet-1",
            &event,
        )
        .unwrap();
        assert_eq!(call.path, "/orders/order-1/confirm");
        assert_eq!(call.payload["confirmed_at"], "2026-08-08T10:00:00Z");
    }

    #[test]
    fn kot_has_no_route_yet() {
        let event = serde_json::json!({ "event_type": "KOTCreated", "data": {} });
        let err = resolve("ob1", "kot", "KOTCreated", "kot-1", "outlet-1", &event).unwrap_err();
        assert!(matches!(err, SyncError::UnroutedEvent { .. }));
    }

    #[test]
    fn table_session_updated_routes_to_single_session_route() {
        let event = serde_json::json!({
            "event_type": "TableSessionUpdated",
            "data": { "session": { "id": "session-1" } }
        });
        let call = resolve(
            "ob1",
            "table_session",
            "TableSessionUpdated",
            "session-1",
            "outlet-1",
            &event,
        )
        .unwrap();
        assert_eq!(call.path, "/outlets/outlet-1/table-sessions/session-1");
        assert_eq!(call.payload["id"], "session-1");
    }

    /// An unrecognized event_type for a *known* aggregate_type must be a
    /// hard error, never silently absorbed by a wildcard arm — a typo in
    /// the POS's outbox event_type (e.g. "NotAFrozenEvent") must not be
    /// replayed as if it were a legitimate transition.
    #[test]
    fn unknown_event_type_for_known_aggregate_is_unrouted_not_swallowed() {
        let event = serde_json::json!({ "event_type": "NotAFrozenEvent", "data": {} });
        let err = resolve(
            "ob1",
            "table_session",
            "NotAFrozenEvent",
            "session-1",
            "outlet-1",
            &event,
        )
        .unwrap_err();
        assert!(matches!(err, SyncError::UnroutedEvent { .. }));

        let event2 = serde_json::json!({ "event_type": "NotAFrozenOrderEvent", "data": {} });
        let err2 = resolve(
            "ob1",
            "order",
            "NotAFrozenOrderEvent",
            "order-1",
            "outlet-1",
            &event2,
        )
        .unwrap_err();
        assert!(matches!(err2, SyncError::UnroutedEvent { .. }));
    }
}
