// Package tenant owns the organisation (tenant) and brand levels of the
// hierarchy described in docs/spec/multi-outlet.md:
//
//	Organisation (tenant) -> Brand -> Outlet
//
// Outlet itself lives in internal/outlet; this package provides the
// business commands needed to create the tenant and brand an outlet is
// attached to. It deliberately exposes commands (CreateOrganisation,
// CreateBrand), not raw CRUD, per CLAUDE.md.
package tenant

import "time"

// Tenant is an organisation: the top of the ownership hierarchy. Every
// tenant-owned row across every bounded context is scoped by Tenant.ID.
type Tenant struct {
	ID        string
	Name      string
	CreatedAt time.Time
	UpdatedAt time.Time
}

// Brand belongs to exactly one tenant. Outlets belong to a brand, so brand
// is the join point tenant isolation checks pivot through when an operation
// only carries a brand id or outlet id.
type Brand struct {
	ID        string
	TenantID  string
	Name      string
	CreatedAt time.Time
	UpdatedAt time.Time
}
