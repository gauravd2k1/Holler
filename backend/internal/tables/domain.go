// Package tables implements the Milestone 1 table bounded context
// (docs/spec/tables.md, ADR-011).
//
// Two separate aggregates live here, deliberately never merged into one row:
//
//   - RestaurantTable — the physical definition (section, label, seat count).
//     Pure configuration: cloud is the source of truth, writes bump the
//     owning outlet's config_version exactly like menu, and the row syncs
//     cloud→edge, replaced wholesale.
//
//   - TableSession — one seating of one table: state, current_order_id,
//     guest_count, opened_at/closed_at. This is edge-authoritative,
//     replayed edge→cloud append-only. The cloud accepts replayed sessions;
//     it never originates or mutates one on its own initiative.
//
// AVAILABLE is not a stored TableSessionState — a table with no open session
// is available, derived at read time. RESERVED exists in the display-state
// vocabulary because docs/spec/tables.md defines it, but nothing in
// Milestone 1 produces it (reservations are Milestone 9 and are explicitly
// excluded here — no reservation model, field or endpoint of any kind).
//
// Milestone 1 excludes merge/split tables and item transfer; only open,
// state transition and close are implemented.
package tables

import (
	"context"

	contracts "github.com/holler/contracts"
)

// RestaurantTable is the physical table definition. Re-exported from
// packages/contracts/go so this package never hand-rolls a mirror.
type RestaurantTable = contracts.RestaurantTable

// TableSession is the operational aggregate: one seating of one table.
type TableSession = contracts.TableSession

// TableSessionState is the set of stored session states.
type TableSessionState = contracts.TableSessionState

// TableDisplayState is the floor-plan vocabulary docs/spec/tables.md
// renders. AVAILABLE is derived, never stored (see package doc).
type TableDisplayState = contracts.TableDisplayState

const permTableManage = "table.manage"

// Principal is the minimal view this package needs of an authenticated
// caller, matching the pattern in backend/internal/menu so this package can
// be built and tested without a hard dependency on backend/internal/auth.
// backend/internal/auth.AuthenticatedPrincipal satisfies this interface.
type Principal interface {
	HasPermission(permission string) bool
}

type principalContextKey struct{}

// WithPrincipal attaches an authenticated principal to ctx.
func WithPrincipal(ctx context.Context, p Principal) context.Context {
	return context.WithValue(ctx, principalContextKey{}, p)
}

// PrincipalFromContext retrieves the principal stashed by WithPrincipal.
func PrincipalFromContext(ctx context.Context) (Principal, bool) {
	p, ok := ctx.Value(principalContextKey{}).(Principal)
	return p, ok
}
