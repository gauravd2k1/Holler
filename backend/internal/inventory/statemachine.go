package inventory

import (
	"fmt"

	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

// ErrAuthorityViolation mirrors backend/internal/ordering/statemachine.go's
// sentinel exactly: returned when a sync envelope's aggregate_type/direction
// contradicts the route it arrived on, or contracts.AggregateAuthority for
// that aggregate type. The HTTP layer maps it to 422 EnvelopeRouteMismatch
// per §50.1 — a protocol violation, never a coercion.
var ErrAuthorityViolation = fmt.Errorf("%w: sync envelope direction violates aggregate authority", httpx.ErrInvalidInput)

// ErrRecipeCycle is returned when a proposed SUB_RECIPE reference would let
// the parent recipe reach itself, naming the offending path (ADR-018 §7).
var ErrRecipeCycle = fmt.Errorf("%w: sub-recipe reference would create a cycle", httpx.ErrConflict)

// ErrRecipeDepthExceeded is returned when a proposed SUB_RECIPE reference
// would nest deeper than MaxRecipeDepth (ADR-018 §7).
var ErrRecipeDepthExceeded = fmt.Errorf("%w: sub-recipe nesting exceeds MaxRecipeDepth", httpx.ErrConflict)

// ErrDimensionMismatch is returned when a recipe_ingredient's author-chosen
// quantity_dimension disagrees with its referent's own dimension (ADR-018
// 0.5.1/0.5.2 addenda). 422, never coerced — there is nothing to convert
// through: a recipe is not an inventory item.
var ErrDimensionMismatch = fmt.Errorf("%w: quantity_dimension does not match the referenced item or recipe's dimension", httpx.ErrInvalidInput)

// ErrLedgerSequenceMarkReused is returned when an incoming entry claims an
// entry_seq at or below the outlet's high-water mark under a DIFFERENT id.
// The mark would become ambiguous exactly as ADR-018 §6/§9 warns, so this is
// rejected — the one contiguity condition that still refuses a row.
//
// It is unreachable through the edge's own code path: contracts 0.5.3 made
// entry_seq a durable counter precisely so a mark is never reused, and the
// UNIQUE (outlet_id, entry_seq) key would reject it at the database anyway.
// Reaching it means something upstream is minting marks it does not own, and
// the correct response is to stop rather than to store two rows claiming one
// position. CLEARING IT IS A MANUAL OPERATION: an operator establishes which
// row is genuine and removes or renumbers the other at source.
//
// NOTE what this does NOT cover. An entry_seq BEYOND the mark — a hole — is
// accepted and recorded in ledger_replay_gap; see Service.IngestLedgerEntry.
var ErrLedgerSequenceMarkReused = fmt.Errorf("%w: entry_seq is at or below the outlet's high-water mark under a different id", httpx.ErrConflict)

// validateAuthority enforces the §50.1 authority rule: an envelope for
// aggregateType must carry exactly the direction contracts.AggregateAuthority
// assigns that aggregate type. Anything else is rejected outright — the
// unknown-type branch matters here specifically because ADR-018 §10.1 pins
// stock_deduction_gap as a real AggregateType member for exactly this check
// to be able to accept it.
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

// requireEnvelopeOneOf implements ADR-018 §10.1's "a route pins a declared
// SET" weakening of requireEnvelope's single-type pin
// (backend/internal/ordering/service.go): POST /inventory/ledger-entries
// accepts both stock_ledger_entry and stock_deduction_gap. Anything outside
// the declared set is still 422, exactly as a single-type route rejects a
// mismatch.
func requireEnvelopeOneOf(env contracts.SyncEnvelope, allowed ...contracts.AggregateType) error {
	matched := false
	for _, a := range allowed {
		if env.AggregateType == a {
			matched = true
			break
		}
	}
	if !matched {
		return fmt.Errorf("%w: expected aggregate_type in %v, got %q", ErrAuthorityViolation, allowed, env.AggregateType)
	}
	return requireEnvelopeCommon(env)
}

// requireEnvelope is the single-type form, mirroring
// backend/internal/ordering/service.go's requireEnvelope exactly.
func requireEnvelope(env contracts.SyncEnvelope, expectAggregate contracts.AggregateType) error {
	if env.AggregateType != expectAggregate {
		return fmt.Errorf("%w: expected aggregate_type %q, got %q", ErrAuthorityViolation, expectAggregate, env.AggregateType)
	}
	return requireEnvelopeCommon(env)
}

func requireEnvelopeCommon(env contracts.SyncEnvelope) error {
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
	if env.Version < 1 {
		return fmt.Errorf("%w: version must be >= 1", httpx.ErrInvalidInput)
	}
	return nil
}

// requireTenantMatch mirrors backend/internal/ordering/service.go's guard
// against a caller replaying an envelope for a tenant other than the one
// their credential authenticates as.
func requireTenantMatch(callerTenantID string, env contracts.SyncEnvelope) error {
	if callerTenantID == "" {
		return httpx.ErrUnauthorized
	}
	if env.TenantID != callerTenantID {
		return httpx.ErrForbidden
	}
	return nil
}
