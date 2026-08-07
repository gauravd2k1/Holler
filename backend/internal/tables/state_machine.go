package tables

import (
	"errors"
	"fmt"

	contracts "github.com/holler/contracts"

	"github.com/holler/backend/internal/platform/httpx"
)

// allowedTransitions is the table session state machine (docs/spec/tables.md
// §Table management). CLOSED is terminal. Merge/split/transfer are out of
// scope for Milestone 1, so this graph only models opening, the operational
// progression, and closing.
var allowedTransitions = map[TableSessionState][]TableSessionState{
	contracts.TableSessionStateOccupied: {
		contracts.TableSessionStateOrdered,
		contracts.TableSessionStateDirty,
		contracts.TableSessionStateClosed,
	},
	contracts.TableSessionStateOrdered: {
		contracts.TableSessionStateKotSent,
		contracts.TableSessionStateDirty,
		contracts.TableSessionStateClosed,
	},
	contracts.TableSessionStateKotSent: {
		contracts.TableSessionStateFoodReady,
		contracts.TableSessionStateDirty,
		contracts.TableSessionStateClosed,
	},
	contracts.TableSessionStateFoodReady: {
		contracts.TableSessionStateBillRequested,
		contracts.TableSessionStateDirty,
		contracts.TableSessionStateClosed,
	},
	contracts.TableSessionStateBillRequested: {
		contracts.TableSessionStatePaymentPending,
		contracts.TableSessionStateDirty,
		contracts.TableSessionStateClosed,
	},
	contracts.TableSessionStatePaymentPending: {
		contracts.TableSessionStatePaid,
		contracts.TableSessionStateDirty,
		contracts.TableSessionStateClosed,
	},
	contracts.TableSessionStatePaid: {
		contracts.TableSessionStateDirty,
		contracts.TableSessionStateClosed,
	},
	contracts.TableSessionStateDirty: {
		contracts.TableSessionStateClosed,
	},
	contracts.TableSessionStateClosed: {},
}

// ErrIllegalTransition marks a state-machine violation specifically (as
// opposed to other httpx.ErrInvalidInput causes such as a missing field), so
// HTTP layers that must map it to a different status code — the envelope
// ingest routes return 409, not 400 — can distinguish it with errors.Is.
var ErrIllegalTransition = errors.New("illegal table session state transition")

// validateTransition returns an error wrapping both ErrIllegalTransition and
// httpx.ErrInvalidInput unless moving a session from `from` to `to` is a
// legal edge in the state machine.
func validateTransition(from, to TableSessionState) error {
	for _, next := range allowedTransitions[from] {
		if next == to {
			return nil
		}
	}
	return fmt.Errorf("%w: table session cannot transition from %s to %s: %w", ErrIllegalTransition, from, to, httpx.ErrInvalidInput)
}

// DeriveDisplayState returns the floor-plan state for a table. A table with
// no open session is AVAILABLE, never a stored value (ADR-011). Otherwise the
// display state is the open session's stored state.
func DeriveDisplayState(openSession *TableSession) string {
	if openSession == nil {
		return string(contracts.TableDisplayStateAvailable)
	}
	return string(openSession.State)
}
