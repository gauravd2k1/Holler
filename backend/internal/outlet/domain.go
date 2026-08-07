// Package outlet owns the outlet level of docs/spec/multi-outlet.md's
// hierarchy (Organisation -> Brand -> Outlet) and enforces tenant isolation
// for every outlet read: every query is scoped by the tenant id taken from
// the authenticated principal's context, never from a request parameter
// (docs/spec/security-rbac.md §Tenant isolation).
package outlet

import "time"

// defaultTimezone matches the packages/contracts/postgres/0001_init.sql
// column default; the service applies it explicitly rather than relying on
// an implicit DB default so behaviour is identical however the row is
// created.
const defaultTimezone = "Asia/Kolkata"

// Outlet is a single physical/operational location under a brand. Revenue
// centers, floors, tables, kitchens, stations, registers and devices all
// hang off an outlet (docs/spec/multi-outlet.md), but those are owned by
// other bounded contexts.
type Outlet struct {
	ID            string
	BrandID       string
	Name          string
	Timezone      string
	ConfigVersion int
	CreatedAt     time.Time
	UpdatedAt     time.Time
}
