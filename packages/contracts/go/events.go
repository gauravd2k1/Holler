// Business event payloads for the transactional outbox (ADR-007). Mirrors
// src/types/events.ts. Only the M0–M2 slice events are defined here per
// §81 MILESTONE 0.5 scope.
package contracts

import "time"

type EventEnvelope struct {
	EventID       string    `json:"event_id"`
	EventType     string    `json:"event_type"`
	OccurredAt    time.Time `json:"occurred_at"`
	OutletID      string    `json:"outlet_id"`
	SchemaVersion int       `json:"schema_version"`
}

type OrderCreatedEvent struct {
	EventEnvelope
	Data struct {
		Order CanonicalOrder `json:"order"`
	} `json:"data"`
}

type ItemAddedEvent struct {
	EventEnvelope
	Data struct {
		OrderID string    `json:"order_id"`
		Item    OrderItem `json:"item"`
	} `json:"data"`
}

// ItemRemovedEvent added at 0.2.3. Carries the full item, not just an id: once
// the row is deleted the cloud cannot look up what left the order.
type ItemRemovedEvent struct {
	EventEnvelope
	Data struct {
		OrderID string    `json:"order_id"`
		Item    OrderItem `json:"item"`
	} `json:"data"`
}

type KotCreatedEvent struct {
	EventEnvelope
	Data struct {
		Kot Kot `json:"kot"`
	} `json:"data"`
}

type OrderReadyEvent struct {
	EventEnvelope
	Data struct {
		OrderID string `json:"order_id"`
	} `json:"data"`
}

// Added at 0.2.2 — see the src/types/events.ts note on why these strings were
// already in use at the edge before they were frozen here.

type SentToKitchenEvent struct {
	EventEnvelope
	Data struct {
		OrderID string `json:"order_id"`
	} `json:"data"`
}

type OrderCancelledEvent struct {
	EventEnvelope
	Data struct {
		OrderID string `json:"order_id"`
		Reason  string `json:"reason"`
	} `json:"data"`
}

type TableSessionOpenedEvent struct {
	EventEnvelope
	Data struct {
		Session TableSession `json:"session"`
	} `json:"data"`
}

type TableSessionUpdatedEvent struct {
	EventEnvelope
	Data struct {
		Session TableSession `json:"session"`
	} `json:"data"`
}

// Event type string constants. The edge crates carry these as Rust literals
// with no compile-time link to this list, so scripts/check-event-type-drift.mjs
// greps them against it in both directions.
const (
	EventTypeOrderCreated        = "OrderCreated"
	EventTypeItemAdded           = "ItemAdded"
	EventTypeItemRemoved         = "ItemRemoved"
	EventTypeKotCreated          = "KOTCreated"
	EventTypeOrderReady          = "OrderReady"
	EventTypeSentToKitchen       = "SentToKitchen"
	EventTypeOrderCancelled      = "OrderCancelled"
	EventTypeTableSessionOpened  = "TableSessionOpened"
	EventTypeTableSessionUpdated = "TableSessionUpdated"
)

// OutboxEventTypes mirrors OUTBOX_EVENT_TYPES in src/types/events.ts, in the
// same order. A drift test asserts they are identical.
var OutboxEventTypes = []string{
	EventTypeOrderCreated,
	EventTypeItemAdded,
	EventTypeItemRemoved,
	EventTypeKotCreated,
	EventTypeOrderReady,
	EventTypeSentToKitchen,
	EventTypeOrderCancelled,
	EventTypeTableSessionOpened,
	EventTypeTableSessionUpdated,
}
