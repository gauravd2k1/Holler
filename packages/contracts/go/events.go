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
