package outlet_test

import (
	"context"
	"errors"
	"path/filepath"
	"testing"

	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/platform/testdb"
	"github.com/holler/backend/internal/tenant"
)

// setupPool uses the shared testdb gate: an unset HOLLER_TEST_DATABASE_URL
// fails this test loudly by default rather than skipping silently.
func setupPool(t *testing.T) postgres.Pool {
	t.Helper()

	dbURL := testdb.RequireDatabaseURL(t)

	ctx := context.Background()
	pool, err := postgres.Open(ctx, dbURL)
	if err != nil {
		t.Fatalf("postgres.Open: %v", err)
	}
	t.Cleanup(pool.Close)

	contractsDir, err := filepath.Abs(filepath.Join("..", "..", "..", "packages", "contracts", "postgres"))
	if err != nil {
		t.Fatalf("resolving contracts dir: %v", err)
	}
	if err := postgres.Migrate(ctx, pool, contractsDir); err != nil {
		t.Fatalf("postgres.Migrate: %v", err)
	}
	return pool
}

// TestPostgresRepository_CrossTenantOutletLookupIsNotFound is the
// dedicated automated test docs/spec/security-rbac.md §Tenant isolation
// requires, run against a real Postgres: a request carrying tenant A's
// principal and tenant B's outlet id must 404, not 200 and not a
// leak-confirming 403.
func TestPostgresRepository_CrossTenantOutletLookupIsNotFound(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()

	tenantSvc := tenant.NewService(tenant.NewPostgresRepository(pool))
	outletSvc := outlet.NewService(outlet.NewPostgresRepository(pool))

	orgA, err := tenantSvc.CreateOrganisation(ctx, "Integration Outlet Org A")
	if err != nil {
		t.Fatalf("CreateOrganisation A: %v", err)
	}
	orgB, err := tenantSvc.CreateOrganisation(ctx, "Integration Outlet Org B")
	if err != nil {
		t.Fatalf("CreateOrganisation B: %v", err)
	}

	brandA, err := tenantSvc.CreateBrand(ctx, orgA.ID, "Integration Brand A")
	if err != nil {
		t.Fatalf("CreateBrand A: %v", err)
	}
	brandB, err := tenantSvc.CreateBrand(ctx, orgB.ID, "Integration Brand B")
	if err != nil {
		t.Fatalf("CreateBrand B: %v", err)
	}

	principalA := outlet.Principal{TenantID: orgA.ID}
	principalB := outlet.Principal{TenantID: orgB.ID}

	outletA, err := outletSvc.CreateOutlet(ctx, principalA, brandA.ID, "Outlet A", "")
	if err != nil {
		t.Fatalf("CreateOutlet A: %v", err)
	}
	outletB, err := outletSvc.CreateOutlet(ctx, principalB, brandB.ID, "Outlet B", "")
	if err != nil {
		t.Fatalf("CreateOutlet B: %v", err)
	}

	// A cannot create an outlet under B's brand.
	if _, err := outletSvc.CreateOutlet(ctx, principalA, brandB.ID, "Hijacked Outlet", ""); !errors.Is(err, httpx.ErrNotFound) {
		t.Fatalf("expected ErrNotFound creating outlet under another tenant's brand, got %v", err)
	}

	// A's list never contains B's outlet.
	listA, err := outletSvc.ListOutlets(ctx, principalA)
	if err != nil {
		t.Fatalf("ListOutlets A: %v", err)
	}
	for _, o := range listA {
		if o.ID == outletB.ID {
			t.Fatal("tenant A's outlet list leaked tenant B's outlet")
		}
	}

	// The core assertion: A's principal, B's outlet id -> not found.
	if _, err := outletSvc.GetOutlet(ctx, principalA, outletB.ID); !errors.Is(err, httpx.ErrNotFound) {
		t.Fatalf("expected ErrNotFound for cross-tenant outlet lookup, got %v", err)
	}

	// Sanity: B can fetch its own outlet, A can fetch its own outlet.
	if _, err := outletSvc.GetOutlet(ctx, principalB, outletB.ID); err != nil {
		t.Fatalf("owning tenant B should fetch its own outlet: %v", err)
	}
	if _, err := outletSvc.GetOutlet(ctx, principalA, outletA.ID); err != nil {
		t.Fatalf("owning tenant A should fetch its own outlet: %v", err)
	}
}

// TestPostgresRepository_DayStartTime_NonMidnightSurvivesRoundTrip is the M4
// T4 task's non-negotiable acceptance condition: a non-midnight value must be
// written and read back, not merely the default '00:00' the column already
// carries from creation — every existing test in this suite exercises only
// that default, which is exactly how the write path went unshipped.
func TestPostgresRepository_DayStartTime_NonMidnightSurvivesRoundTrip(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()

	tenantSvc := tenant.NewService(tenant.NewPostgresRepository(pool))
	outletSvc := outlet.NewService(outlet.NewPostgresRepository(pool))

	org, err := tenantSvc.CreateOrganisation(ctx, "Day Start Org")
	if err != nil {
		t.Fatalf("CreateOrganisation: %v", err)
	}
	brand, err := tenantSvc.CreateBrand(ctx, org.ID, "Day Start Brand")
	if err != nil {
		t.Fatalf("CreateBrand: %v", err)
	}
	principal := outlet.Principal{TenantID: org.ID}

	o, err := outletSvc.CreateOutlet(ctx, principal, brand.ID, "Day Start Outlet", "")
	if err != nil {
		t.Fatalf("CreateOutlet: %v", err)
	}
	if o.DayStartTime != "00:00" {
		t.Fatalf("expected default day_start_time 00:00, got %q", o.DayStartTime)
	}

	updated, err := outletSvc.SetDayStartTime(ctx, principal, o.ID, "04:00")
	if err != nil {
		t.Fatalf("SetDayStartTime: %v", err)
	}
	if updated.DayStartTime != "04:00" {
		t.Fatalf("expected day_start_time 04:00 immediately after the write, got %q", updated.DayStartTime)
	}
	if updated.ConfigVersion <= o.ConfigVersion {
		t.Fatalf("expected config_version to bump: before=%d after=%d", o.ConfigVersion, updated.ConfigVersion)
	}

	// The non-negotiable assertion: read it back through a fresh GetOutlet,
	// independent of the value SetDayStartTime itself returned.
	reread, err := outletSvc.GetOutlet(ctx, principal, o.ID)
	if err != nil {
		t.Fatalf("GetOutlet after SetDayStartTime: %v", err)
	}
	if reread.DayStartTime != "04:00" {
		t.Fatalf("day_start_time did not survive the round trip: got %q, want 04:00", reread.DayStartTime)
	}

	// Invalid input is a hard rejection, never coerced (task instruction).
	if _, err := outletSvc.SetDayStartTime(ctx, principal, o.ID, "25:99"); !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput for a malformed day_start_time, got %v", err)
	}
	if _, err := outletSvc.SetDayStartTime(ctx, principal, o.ID, "4:00"); !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput for a non-zero-padded hour, got %v", err)
	}
}
