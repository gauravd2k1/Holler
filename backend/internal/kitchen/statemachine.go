package kitchen

import (
	"fmt"

	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

// kotTransitions encodes docs/spec/kitchen.md's KOT lifecycle:
//
//	NEW -> ACKNOWLEDGED -> PREPARING -> READY -> SERVED
//	plus CANCELLED, reachable from any non-terminal state.
//
// Illegal transitions are rejected here, at the command layer — never merely
// by a KDS screen.
var kotTransitions = map[KotStatus]map[KotStatus]bool{
	KotStatusNew: {
		KotStatusAcknowledged: true,
		KotStatusCancelled:    true,
	},
	KotStatusAcknowledged: {
		KotStatusPreparing: true,
		KotStatusCancelled: true,
	},
	KotStatusPreparing: {
		KotStatusReady:     true,
		KotStatusCancelled: true,
	},
	KotStatusReady: {
		KotStatusServed:    true,
		KotStatusCancelled: true,
	},
	KotStatusServed:    {},
	KotStatusCancelled: {},
}

// ErrIllegalTransition is a sentinel wrapped by httpx.ErrConflict so the HTTP
// layer maps an illegal KOT status transition to 409 without kitchen needing
// its own status table.
var ErrIllegalTransition = fmt.Errorf("%w: illegal KOT status transition", httpx.ErrConflict)

// ErrAuthorityViolation is returned when a sync envelope's aggregate_type or
// direction contradicts contracts.AggregateAuthority — the kot aggregate is
// EDGE_TO_CLOUD only. A mismatch is a protocol violation, never a coercion
// (docs/spec/sync.md §50.1, ADR-014).
var ErrAuthorityViolation = fmt.Errorf("%w: sync envelope aggregate_type/direction violates aggregate authority", httpx.ErrInvalidInput)

// validKotTransition reports whether a ticket may move from -> to.
func validKotTransition(from, to KotStatus) bool {
	next, ok := kotTransitions[from]
	if !ok {
		return false
	}
	return next[to]
}

// validateAuthority enforces the §50.1 authority rule: an envelope for
// aggregateType must carry exactly the direction
// contracts.AggregateAuthority assigns that aggregate type.
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

// requireKotEnvelope validates a SyncEnvelope carries aggregate_type "kot"
// with the EDGE_TO_CLOUD direction §50.1 requires, plus the envelope's
// mandatory identity fields. Both the aggregate_type and direction checks
// wrap ErrAuthorityViolation so the HTTP layer maps them to 422
// EnvelopeRouteMismatch, never a coercion.
func requireKotEnvelope(env contracts.SyncEnvelope) error {
	if env.AggregateType != contracts.AggregateTypeKot {
		return fmt.Errorf("%w: expected aggregate_type %q, got %q", ErrAuthorityViolation, contracts.AggregateTypeKot, env.AggregateType)
	}
	if err := validateAuthority(env.AggregateType, env.Direction); err != nil {
		return err
	}
	if env.RecordID == "" {
		return fmt.Errorf("%w: record_id is required", httpx.ErrInvalidInput)
	}
	if env.TenantID == "" {
		return fmt.Errorf("%w: tenant_id is required", httpx.ErrInvalidInput)
	}
	if env.OutletID == "" {
		return fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	if env.DeviceID == "" {
		return fmt.Errorf("%w: device_id is required", httpx.ErrInvalidInput)
	}
	return nil
}
