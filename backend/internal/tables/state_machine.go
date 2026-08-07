package tables

import (
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

// validateTransition returns an httpx.ErrInvalidInput-wrapped error unless
// moving a session from `from` to `to` is a legal edge in the state machine.
func validateTransition(from, to TableSessionState) error {
	for _, next := range allowedTransitions[from] {
		if next == to {
			return nil
		}
	}
	return fmt.Errorf("%w: table session cannot transition from %s to %s", httpx.ErrInvalidInput, from, to)
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
