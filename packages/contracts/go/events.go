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

// OrderConfirmedEvent added at 0.2.5 — the cashier confirming a draft
// (DRAFT->CONFIRMED). Deliberately not named OrderAccepted: M6 aggregator
// acceptance is a different business event and gets its own type.
type OrderConfirmedEvent struct {
	EventEnvelope
	Data struct {
		OrderID string `json:"order_id"`
		// The moment the EDGE recorded, not when the cloud received it (§50.1).
		ConfirmedAt time.Time `json:"confirmed_at"`
	} `json:"data"`
}

type KotCreatedEvent struct {
	EventEnvelope
	Data struct {
		Kot Kot `json:"kot"`
	} `json:"data"`
}

// KotStatusChangedEvent added at 0.3.0 (ADR-014). Milestone 2 gives the KOT a
// lifecycle driven from KDS screens; before this, KOTCreated was the only KOT
// event frozen, so every transition after creation was invisible to the cloud.
// ChangedAt is the moment the EDGE recorded (§50.1) — an outlet syncing once an
// hour would otherwise report every ticket as prepared in the same instant.
type KotStatusChangedEvent struct {
	EventEnvelope
	Data struct {
		KotID             string    `json:"kot_id"`
		OrderID           string    `json:"order_id"`
		Status            KotStatus `json:"status"`
		ChangedAt         time.Time `json:"changed_at"`
		ChangedByDeviceID string    `json:"changed_by_device_id"`
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
	EventTypeOrderCreated = "OrderCreated"
	EventTypeItemAdded    = "ItemAdded"
	EventTypeItemRemoved  = "ItemRemoved"
	// Added at 0.4.1 (ADR-016 addendum). Carries the FULL corrected line, not a
	// delta: a delta-only payload was rejected on §50.1 grounds because it would
	// make the cloud recompute money the edge is authoritative for.
	EventTypeItemQuantityChanged = "ItemQuantityChanged"
	EventTypeOrderConfirmed      = "OrderConfirmed"
	EventTypeKotCreated          = "KOTCreated"
	EventTypeKotStatusChanged    = "KOTStatusChanged"
	EventTypeOrderReady          = "OrderReady"
	EventTypeSentToKitchen       = "SentToKitchen"
	EventTypeOrderCancelled      = "OrderCancelled"
	EventTypeTableSessionOpened  = "TableSessionOpened"
	EventTypeTableSessionUpdated = "TableSessionUpdated"

	// Milestone 3 billing events (ADR-016). §53 names InvoiceCreated,
	// PaymentReceived and PaymentRefunded as immutable business events; the two
	// shift events complete the cash-drawer trail §39 requires.
	EventTypeInvoiceCreated  = "InvoiceCreated"
	EventTypePaymentReceived = "PaymentReceived"
	EventTypePaymentRefunded = "PaymentRefunded"
	EventTypeCashShiftOpened = "CashShiftOpened"
	EventTypeCashShiftClosed = "CashShiftClosed"

	// Milestone 4 (0.5.5). stock_count is EDGE_TO_CLOUD and had no push
	// mechanism: the ledger replays by entry_seq-ranged cursor and a count
	// carries no entry_seq, so it fell between the two. Events rather than a
	// cursor because a completed stocktake is an individually meaningful,
	// low-volume fact -- the same cut the ranged-sync decision drew.
	EventTypeStockCountOpened    = "StockCountOpened"
	EventTypeStockCountCompleted = "StockCountCompleted"

	// Milestone 5 (0.6.1, ADR-019 addendum). Frozen because nothing replays
	// without them: edge/database emits these today and edge/sync cannot carry
	// a type the contract does not name. Discrete, individually meaningful,
	// low-volume -- the StockCountCompleted cut, not the ranged-cursor one.
	EventTypeGoodsReceived    = "GoodsReceived"
	EventTypeGrnGapRecorded   = "GrnGapRecorded"
	EventTypePurchaseReturned = "PurchaseReturned"
	EventTypeStockDispatched  = "StockDispatched"
)

// OutboxEventTypes mirrors OUTBOX_EVENT_TYPES in src/types/events.ts, in the
// same order. A drift test asserts they are identical.
var OutboxEventTypes = []string{
	EventTypeOrderCreated,
	EventTypeItemAdded,
	EventTypeItemRemoved,
	EventTypeItemQuantityChanged,
	EventTypeOrderConfirmed,
	EventTypeKotCreated,
	EventTypeKotStatusChanged,
	EventTypeOrderReady,
	EventTypeSentToKitchen,
	EventTypeOrderCancelled,
	EventTypeTableSessionOpened,
	EventTypeTableSessionUpdated,
	EventTypeInvoiceCreated,
	EventTypePaymentReceived,
	EventTypePaymentRefunded,
	EventTypeCashShiftOpened,
	EventTypeCashShiftClosed,
	EventTypeStockCountOpened,
	EventTypeStockCountCompleted,
	EventTypeGoodsReceived,
	EventTypeGrnGapRecorded,
	EventTypePurchaseReturned,
	EventTypeStockDispatched,
}
