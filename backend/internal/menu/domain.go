// Package menu implements the Milestone 1 catalog: one menu per outlet with
// categories, items, variants and modifiers, priced in integer paise. Cloud
// is the source of truth for the catalog (docs/spec/sync.md §50.1): every
// write that changes catalog content bumps the owning outlet's
// config_version exactly once and stamps the affected rows with it, so the
// edge can pull "everything newer than N" and replace it wholesale.
//
// Channel price books, aggregator pricing, combos and brand→outlet→channel
// inheritance are out of scope for Milestone 1 (docs/spec/menu.md describes
// them for Milestones 6/8) and are intentionally not modelled here.
package menu

import "context"

// Category is a menu_category row.
type Category struct {
	ID            string
	OutletID      string
	Name          string
	SortOrder     int
	ConfigVersion int
}

// Item is a menu_item row. BasePricePaise is always an integer; no float
// arithmetic touches money anywhere in this package.
type Item struct {
	ID             string
	OutletID       string
	CategoryID     string
	Name           string
	BasePricePaise int64
	IsAvailable    bool
	// TaxProfileID added at contracts 0.4.2 (ADR-016 addendum); nil means
	// "use the outlet's default profile". No write path in this package
	// sets it yet (filed gap, M4 T4 delivery-fix follow-up) — added here so
	// the column round-trips once one exists, and so GET /sync/config can
	// carry whatever a future write path, or a direct migration, puts here.
	TaxProfileID *string
	// HSNSAC added at contracts 0.4.5. An invoice cannot legally issue with
	// a NULL/blank HSN/SAC on any line (CLAUDE.md) — same "unwritten, but
	// must still be delivered" reasoning as TaxProfileID above.
	HSNSAC        *string
	ConfigVersion int
}

// Variant is a menu_item_variant row. PriceDeltaPaise is added to the item's
// base price when the variant is selected.
type Variant struct {
	ID              string
	MenuItemID      string
	Name            string
	PriceDeltaPaise int64
	// IsDefault added at contracts 0.5.0 (ADR-018 §2.1): at most one default
	// variant per item, enforced by a partial unique index
	// (postgres/0014_menu_default_variant.sql). A NOT NULL recipe binding
	// depends on every sellable item resolving to a variant; this is the
	// column that makes "resolve to a variant" meaningful when none was
	// explicitly chosen.
	IsDefault     bool
	ConfigVersion int
}

// Modifier is a single option within a modifier group (e.g. group "Toppings",
// option "Paneer"). MinSelection/MaxSelection are carried per group: every
// modifier row sharing a GroupName on the same item is expected to carry the
// same min/max, enforced by the service layer at write time.
type Modifier struct {
	ID              string
	MenuItemID      string
	GroupName       string
	OptionName      string
	PriceDeltaPaise int64
	MinSelection    int
	MaxSelection    int
	ConfigVersion   int
}

// Principal is the minimal view this package needs of an authenticated
// caller. Authentication itself belongs to backend/internal/auth (a
// concurrent, separate build); this package only depends on this small
// interface so it can be developed and tested independently.
type Principal interface {
	// HasPermission reports whether the caller holds the named permission,
	// e.g. "menu.manage".
	HasPermission(permission string) bool
}

type principalContextKey struct{}

// WithPrincipal returns a context carrying p. Middleware wiring the real
// auth.Principal implementation into request context should use this so
// handlers in this package can retrieve it via PrincipalFromContext.
func WithPrincipal(ctx context.Context, p Principal) context.Context {
	return context.WithValue(ctx, principalContextKey{}, p)
}

// PrincipalFromContext retrieves the Principal stashed by WithPrincipal.
func PrincipalFromContext(ctx context.Context) (Principal, bool) {
	p, ok := ctx.Value(principalContextKey{}).(Principal)
	return p, ok
}

const permMenuManage = "menu.manage"
