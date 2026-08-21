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

// defaultDayStartTime matches the packages/contracts/postgres/0013_outlet_day_start.sql
// column default: the plain outlet-local calendar date, correct for any
// outlet that closes before midnight (ADR-018 §9.2).
const defaultDayStartTime = "00:00"

// Outlet is a single physical/operational location under a brand. Revenue
// centers, floors, tables, kitchens, stations, registers and devices all
// hang off an outlet (docs/spec/multi-outlet.md), but those are owned by
// other bounded contexts.
type Outlet struct {
	ID       string
	BrandID  string
	Name     string
	Timezone string
	// DayStartTime is local HH:MM, the business-day boundary an outlet
	// trading past midnight configures (ADR-018 §9.2). CONFIG, cloud->edge,
	// travelling with the rest of this row — not a new aggregate. Computing
	// business_date is entirely an edge concern; the cloud only carries the
	// input.
	DayStartTime  string
	ConfigVersion int
	CreatedAt     time.Time
	UpdatedAt     time.Time
}
