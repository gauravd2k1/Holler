// Package ordering owns the cloud-side ordering bounded context. Per
// docs/spec/sync.md §50.1 (ADR-009) the EDGE is the source of truth for
// order transactions: the cloud replays what the edge sends, never merges,
// never originates an order and never generates order ids.
package ordering

import (
	"fmt"

	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

// transitions encodes docs/spec/ordering.md's order state machine exactly:
//
//	DRAFT -> CONFIRMED -> SENT_TO_KITCHEN -> PREPARING -> READY -> SERVED -> BILLED -> PAID -> CLOSED
//	plus CANCELLED, reachable from any non-terminal state.
//
// Illegal transitions (e.g. CLOSED -> DRAFT) are rejected here, at the
// command layer, never merely by a UI.
var transitions = map[contracts.OrderStatus]map[contracts.OrderStatus]bool{
	contracts.OrderStatusDraft: {
		contracts.OrderStatusConfirmed: true,
		contracts.OrderStatusCancelled: true,
	},
	contracts.OrderStatusConfirmed: {
		contracts.OrderStatusSentToKitchen: true,
		contracts.OrderStatusCancelled:     true,
	},
	contracts.OrderStatusSentToKitchen: {
		contracts.OrderStatusPreparing: true,
		contracts.OrderStatusCancelled: true,
	},
	contracts.OrderStatusPreparing: {
		contracts.OrderStatusReady:     true,
		contracts.OrderStatusCancelled: true,
	},
	contracts.OrderStatusReady: {
		contracts.OrderStatusServed:    true,
		contracts.OrderStatusCancelled: true,
	},
	contracts.OrderStatusServed: {
		contracts.OrderStatusBilled: true,
		// Once served, an order is no longer cancellable — it has already
		// reached the guest.
	},
	contracts.OrderStatusBilled: {
		contracts.OrderStatusPaid: true,
	},
	contracts.OrderStatusPaid: {
		contracts.OrderStatusClosed: true,
	},
	contracts.OrderStatusClosed:    {},
	contracts.OrderStatusCancelled: {},
}

// creatableStatuses are the statuses a newly-ingested order may arrive in.
// The edge may create an order already CONFIRMED (e.g. a walk-in cashier
// flow that confirms immediately), but never in a downstream or terminal
// state — that would bypass the state machine entirely.
var creatableStatuses = map[contracts.OrderStatus]bool{
	contracts.OrderStatusDraft:     true,
	contracts.OrderStatusConfirmed: true,
}

// ErrIllegalTransition is a sentinel wrapped by httpx.ErrInvalidInput so the
// HTTP layer maps it to 400 without ordering needing its own status table.
var ErrIllegalTransition = fmt.Errorf("%w: illegal order state transition", httpx.ErrInvalidInput)

// ErrAuthorityViolation is returned when a sync envelope's direction
// contradicts contracts.AggregateAuthority for its aggregate type — the
// order aggregate is EDGE_TO_CLOUD only; a CLOUD_TO_EDGE order envelope is a
// protocol violation, not a value to coerce (docs/spec/sync.md §50.1).
var ErrAuthorityViolation = fmt.Errorf("%w: sync envelope direction violates aggregate authority", httpx.ErrInvalidInput)

// validTransition reports whether an order may move from -> to.
func validTransition(from, to contracts.OrderStatus) bool {
	next, ok := transitions[from]
	if !ok {
		return false
	}
	return next[to]
}

// validCreationStatus reports whether status is a legal status for a
// newly-ingested order.
func validCreationStatus(status contracts.OrderStatus) bool {
	return creatableStatuses[status]
}

// validateAuthority enforces the §50.1 authority rule: an envelope for
// aggregateType must carry exactly the direction contracts.AggregateAuthority
// assigns that aggregate type. Anything else is rejected outright.
func validateAuthority(aggregateType contracts.AggregateType, direction contracts.SyncDirection) error {
	want, known := contracts.AggregateAuthority[aggregateType]
	if !known {
		return fmt.Errorf("%w: unknown aggregate type %q", httpx.ErrInvalidInput, aggregateType)
	}
	if direction != want {
		return fmt.Errorf("%w: aggregate %q requires direction %q, got %q", ErrAuthorityViolation, aggregateType, want, direction)
	}
	return nil
}
