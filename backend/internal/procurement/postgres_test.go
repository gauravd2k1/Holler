package procurement

// Postgres-backed tests for the procurement repository.
//
// WHY THIS FILE EXISTS AT ALL. Until it was written, procurement was the ONLY
// bounded context in this backend with no Postgres-backed test: 984 lines of
// repository.go had never executed one SQL statement against a real server. It
// was covered instead by a static cross-check that every INSERT's column names
// and placeholder arity matched the shipped DDL — which proves the STRINGS are
// consistent with each other and proves nothing about whether Postgres accepts
// them. A type mismatch, a NOT NULL, a CHECK, a trigger or an FK is invisible
// to that check and fatal at runtime. This project has been bitten by exactly
// that substitution before.
//
// Every test here runs the real migrations (packages/contracts/postgres/*.sql)
// and talks to a live server through testdb.RequireDatabaseURL.

import (
	"context"
	"errors"
	"path/filepath"
	"testing"
	"time"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/inventory"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/platform/testdb"
	"github.com/holler/backend/internal/tenant"
	contracts "github.com/holler/contracts"
)

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

// pgFixture is one tenant/brand/outlet with a user, a second outlet in the
// same tenant (for the transfer destination) and two inventory items of
// DIFFERENT dimensions — the second one exists so the quantity_dimension
// mismatch can be provoked against a REAL inventory_item row rather than a
// stub, which is the only way that guard is worth anything.
type pgFixture struct {
	tenantID      string
	outletID      string
	otherOutletID string
	userID        string
	massItemID    string
	countItemID   string
}

func newPgFixture(t *testing.T, pool postgres.Pool, label string) pgFixture {
	t.Helper()
	ctx := context.Background()

	tenantSvc := tenant.NewService(tenant.NewPostgresRepository(pool))
	outletSvc := outlet.NewService(outlet.NewPostgresRepository(pool))

	org, err := tenantSvc.CreateOrganisation(ctx, label+" Org")
	if err != nil {
		t.Fatalf("CreateOrganisation: %v", err)
	}
	brand, err := tenantSvc.CreateBrand(ctx, org.ID, label+" Brand")
	if err != nil {
		t.Fatalf("CreateBrand: %v", err)
	}
	out, err := outletSvc.CreateOutlet(ctx, outlet.Principal{TenantID: org.ID}, brand.ID, label+" Outlet", "")
	if err != nil {
		t.Fatalf("CreateOutlet: %v", err)
	}
	other, err := outletSvc.CreateOutlet(ctx, outlet.Principal{TenantID: org.ID}, brand.ID, label+" Outlet 2", "")
	if err != nil {
		t.Fatalf("CreateOutlet (destination): %v", err)
	}

	userID := id.New()
	if _, err := pool.Exec(ctx, `
		INSERT INTO app_user (id, tenant_id, email, full_name, password_hash, is_active, config_version, created_at, updated_at)
		VALUES ($1, $2, $3, $4, 'unused', TRUE, 0, now(), now())`,
		userID, org.ID, userID+"@example.com", label+" Buyer"); err != nil {
		t.Fatalf("inserting app_user fixture: %v", err)
	}

	invSvc := inventory.NewService(inventory.NewRepository(pool))
	massItem, _, err := invSvc.CreateInventoryItem(ctx, org.ID, inventory.NewInventoryItemInput{
		ID: id.New(), OutletID: out.ID, SKU: label + "-RICE", Name: "Rice",
		Dimension: contracts.DimensionMass, IsActive: true,
	})
	if err != nil {
		t.Fatalf("CreateInventoryItem (MASS): %v", err)
	}
	countItem, _, err := invSvc.CreateInventoryItem(ctx, org.ID, inventory.NewInventoryItemInput{
		ID: id.New(), OutletID: out.ID, SKU: label + "-EGGS", Name: "Eggs",
		Dimension: contracts.DimensionCount, IsActive: true,
	})
	if err != nil {
		t.Fatalf("CreateInventoryItem (COUNT): %v", err)
	}

	return pgFixture{
		tenantID: org.ID, outletID: out.ID, otherOutletID: other.ID, userID: userID,
		massItemID: massItem.ID, countItemID: countItem.ID,
	}
}

// grantApprovalRole creates a real role carrying procurement.approve with the
// given ceiling and assigns it to the fixture's user, so PoApprovalLimitForUser
// and RolesAbleToApprove run their real joins over role / role_permission /
// user_role rather than a fake's map lookup.
//
// limitPaise nil leaves po_approval_limit_paise NULL — the case the whole
// approval design turns on: NULL means "may not approve any amount", never
// "unlimited".
func grantApprovalRole(t *testing.T, pool postgres.Pool, fx pgFixture, code, name string, limitPaise *int64) string {
	t.Helper()
	ctx := context.Background()
	roleID := id.New()
	if _, err := pool.Exec(ctx,
		`INSERT INTO role (id, tenant_id, code, name, po_approval_limit_paise, created_at, updated_at)
		 VALUES ($1, $2, $3, $4, $5, now(), now())`,
		roleID, fx.tenantID, code, name, limitPaise); err != nil {
		t.Fatalf("inserting role fixture: %v", err)
	}
	if _, err := pool.Exec(ctx,
		`INSERT INTO role_permission (role_id, permission) VALUES ($1, $2)`,
		roleID, string(PermissionApprove)); err != nil {
		t.Fatalf("inserting role_permission fixture: %v", err)
	}
	if _, err := pool.Exec(ctx,
		`INSERT INTO user_role (id, user_id, role_id, outlet_id, created_at) VALUES ($1, $2, $3, NULL, now())`,
		id.New(), fx.userID, roleID); err != nil {
		t.Fatalf("inserting user_role fixture: %v", err)
	}
	return roleID
}

func newPgService(pool postgres.Pool) *Service {
	return NewService(NewRepository(pool))
}

func pgPrincipal(fx pgFixture, perms ...contracts.Permission) auth.AuthenticatedPrincipal {
	return auth.AuthenticatedPrincipal{
		UserID: fx.userID, TenantID: fx.tenantID, OutletID: fx.outletID, Permissions: perms,
	}
}

func strPtr(s string) *string { return &s }
