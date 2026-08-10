package auth

import (
	"context"
	"os"
	"testing"
	"time"

	"github.com/holler/backend/internal/platform/id"
	"github.com/holler/backend/internal/platform/postgres"
)

// testPool opens a Postgres pool for integration tests, or skips the test
// when HOLLER_TEST_DATABASE_URL is unset.
func testPool(t *testing.T) postgres.Pool {
	t.Helper()
	url := os.Getenv("HOLLER_TEST_DATABASE_URL")
	if url == "" {
		t.Skip("HOLLER_TEST_DATABASE_URL not set; skipping Postgres integration test")
	}
	pool, err := postgres.Open(context.Background(), url)
	if err != nil {
		t.Fatalf("opening test pool: %v", err)
	}
	t.Cleanup(pool.Close)
	return pool
}

func TestIntegration_SeedTenantRoles_CreatesFifteenRoles(t *testing.T) {
	pool := testPool(t)
	repo := NewRepository(pool)
	ctx := context.Background()

	tenantID := id.New()
	if _, err := pool.Exec(ctx, `INSERT INTO tenant (id, name, created_at, updated_at) VALUES ($1, $2, now(), now())`, tenantID, "Integration Test Tenant"); err != nil {
		t.Fatalf("inserting tenant fixture: %v", err)
	}
	t.Cleanup(func() {
		pool.Exec(ctx, `DELETE FROM role_permission WHERE role_id IN (SELECT id FROM role WHERE tenant_id = $1)`, tenantID)
		pool.Exec(ctx, `DELETE FROM role WHERE tenant_id = $1`, tenantID)
		pool.Exec(ctx, `DELETE FROM tenant WHERE id = $1`, tenantID)
	})

	if err := SeedTenantRoles(ctx, repo, tenantID); err != nil {
		t.Fatalf("seeding roles: %v", err)
	}

	roles, err := repo.ListRoles(ctx, tenantID)
	if err != nil {
		t.Fatalf("listing roles: %v", err)
	}
	if len(roles) != 15 {
		t.Fatalf("expected 15 roles, got %d", len(roles))
	}

	// Idempotent: seeding again must not duplicate rows.
	if err := SeedTenantRoles(ctx, repo, tenantID); err != nil {
		t.Fatalf("re-seeding roles: %v", err)
	}
	roles, err = repo.ListRoles(ctx, tenantID)
	if err != nil {
		t.Fatalf("listing roles after re-seed: %v", err)
	}
	if len(roles) != 15 {
		t.Fatalf("expected 15 roles after re-seed, got %d", len(roles))
	}
}

func TestIntegration_CreateUserAndLogin(t *testing.T) {
	pool := testPool(t)
	repo := NewRepository(pool)
	auditor := NewAuditor(repo)
	signer := NewTokenSigner([]byte("integration-test-key"))
	refresh := NewInMemoryRefreshStore()
	limiter := NewInMemoryRateLimiter()
	svc := NewService(repo, signer, refresh, limiter, auditor, time.Minute, time.Hour)
	ctx := context.Background()

	tenantID := id.New()
	outletID := id.New()
	if _, err := pool.Exec(ctx, `INSERT INTO tenant (id, name, created_at, updated_at) VALUES ($1, $2, now(), now())`, tenantID, "Integration Test Tenant 2"); err != nil {
		t.Fatalf("inserting tenant fixture: %v", err)
	}
	if _, err := pool.Exec(ctx, `INSERT INTO outlet (id, tenant_id, name, timezone, created_at, updated_at) VALUES ($1, $2, 'Test Outlet', 'Asia/Kolkata', now(), now())`, outletID, tenantID); err != nil {
		t.Fatalf("inserting outlet fixture: %v", err)
	}
	t.Cleanup(func() {
		pool.Exec(ctx, `DELETE FROM audit_event WHERE tenant_id = $1`, tenantID)
		pool.Exec(ctx, `DELETE FROM user_role WHERE user_id IN (SELECT id FROM app_user WHERE tenant_id = $1)`, tenantID)
		pool.Exec(ctx, `DELETE FROM app_user WHERE tenant_id = $1`, tenantID)
		pool.Exec(ctx, `DELETE FROM outlet WHERE id = $1`, outletID)
		pool.Exec(ctx, `DELETE FROM tenant WHERE id = $1`, tenantID)
	})

	userID := id.New()
	actor := userID
	user, err := svc.CreateUser(ctx, tenantID, userID, "integration@example.com", "Integration User", "correct-horse-battery", &actor, nil)
	if err != nil {
		t.Fatalf("creating user: %v", err)
	}
	if user.Email != "integration@example.com" {
		t.Fatalf("unexpected created user: %+v", user)
	}

	result, err := svc.Login(ctx, testClientIP, tenantID, "integration@example.com", "correct-horse-battery", outletID)
	if err != nil {
		t.Fatalf("login: %v", err)
	}
	if result.Principal.UserID != userID {
		t.Fatalf("unexpected principal: %+v", result.Principal)
	}
}

// TestIntegration_ListEdgeUserCache covers the /sync/config users export end
// to end against real Postgres: outlet eligibility (tenant-wide role vs.
// outlet-scoped role vs. a different outlet), since_version filtering, both
// pin_hash states round-tripping, and permissions arriving pre-flattened.
func TestIntegration_ListEdgeUserCache(t *testing.T) {
	pool := testPool(t)
	repo := NewRepository(pool)
	ctx := context.Background()

	tenantID := id.New()
	brandID := id.New()
	outletA := id.New()
	outletB := id.New()
	if _, err := pool.Exec(ctx, `INSERT INTO tenant (id, name, created_at, updated_at) VALUES ($1, $2, now(), now())`, tenantID, "Edge Cache Test Tenant"); err != nil {
		t.Fatalf("inserting tenant fixture: %v", err)
	}
	// outlet.brand_id is NOT NULL and has no tenant_id column of its own
	// (packages/contracts/postgres/0001_init.sql: tenant -> brand -> outlet),
	// unlike the tenant_id-on-outlet shape TestIntegration_CreateUserAndLogin
	// assumes elsewhere in this file — that fixture has never executed
	// against real Postgres either, so the mismatch was latent.
	if _, err := pool.Exec(ctx, `INSERT INTO brand (id, tenant_id, name, created_at, updated_at) VALUES ($1, $2, $3, now(), now())`, brandID, tenantID, "Edge Cache Test Brand"); err != nil {
		t.Fatalf("inserting brand fixture: %v", err)
	}
	for _, outletID := range []string{outletA, outletB} {
		if _, err := pool.Exec(ctx, `INSERT INTO outlet (id, brand_id, name, timezone, created_at, updated_at) VALUES ($1, $2, 'Test Outlet', 'Asia/Kolkata', now(), now())`, outletID, brandID); err != nil {
			t.Fatalf("inserting outlet fixture: %v", err)
		}
	}
	t.Cleanup(func() {
		pool.Exec(ctx, `DELETE FROM user_role WHERE user_id IN (SELECT id FROM app_user WHERE tenant_id = $1)`, tenantID)
		pool.Exec(ctx, `DELETE FROM role_permission WHERE role_id IN (SELECT id FROM role WHERE tenant_id = $1)`, tenantID)
		pool.Exec(ctx, `DELETE FROM role WHERE tenant_id = $1`, tenantID)
		pool.Exec(ctx, `DELETE FROM app_user WHERE tenant_id = $1`, tenantID)
		pool.Exec(ctx, `DELETE FROM outlet WHERE brand_id = $1`, brandID)
		pool.Exec(ctx, `DELETE FROM brand WHERE id = $1`, brandID)
		pool.Exec(ctx, `DELETE FROM tenant WHERE id = $1`, tenantID)
	})

	cashierRoleID := id.New()
	if err := repo.SeedRole(ctx, cashierRoleID, tenantID, RoleCodeCashier, "Cashier", []Permission{PermissionOrderCreate}, time.Now().UTC()); err != nil {
		t.Fatalf("seeding cashier role: %v", err)
	}

	now := time.Now().UTC()

	// tenantWideUser: role assignment with outlet_id NULL, eligible at both
	// outlets, has a PIN set.
	tenantWideUser := id.New()
	if err := repo.CreateUser(ctx, tenantWideUser, tenantID, "tenant-wide@example.com", "Tenant Wide", "hash-tenant-wide", now); err != nil {
		t.Fatalf("creating tenant-wide user: %v", err)
	}
	pinHash := "hash-pin-tenant-wide"
	if _, err := pool.Exec(ctx, `UPDATE app_user SET pin_hash = $1, config_version = 5 WHERE id = $2`, pinHash, tenantWideUser); err != nil {
		t.Fatalf("setting pin_hash: %v", err)
	}
	if err := repo.ReplaceUserRoles(ctx, tenantWideUser, []RoleAssignment{{ID: id.New(), RoleID: cashierRoleID, OutletID: nil}}, now); err != nil {
		t.Fatalf("assigning tenant-wide role: %v", err)
	}

	// outletAUser: role scoped to outlet A only, no PIN set.
	outletAUser := id.New()
	if err := repo.CreateUser(ctx, outletAUser, tenantID, "outlet-a@example.com", "Outlet A", "hash-outlet-a", now); err != nil {
		t.Fatalf("creating outlet A user: %v", err)
	}
	if _, err := pool.Exec(ctx, `UPDATE app_user SET config_version = 3 WHERE id = $1`, outletAUser); err != nil {
		t.Fatalf("setting config_version: %v", err)
	}
	outletAID := outletA
	if err := repo.ReplaceUserRoles(ctx, outletAUser, []RoleAssignment{{ID: id.New(), RoleID: cashierRoleID, OutletID: &outletAID}}, now); err != nil {
		t.Fatalf("assigning outlet-scoped role: %v", err)
	}

	// outletBOnlyUser: role scoped to outlet B only — must never appear for
	// outlet A.
	outletBOnlyUser := id.New()
	if err := repo.CreateUser(ctx, outletBOnlyUser, tenantID, "outlet-b@example.com", "Outlet B", "hash-outlet-b", now); err != nil {
		t.Fatalf("creating outlet B user: %v", err)
	}
	if _, err := pool.Exec(ctx, `UPDATE app_user SET config_version = 9 WHERE id = $1`, outletBOnlyUser); err != nil {
		t.Fatalf("setting config_version: %v", err)
	}
	outletBID := outletB
	if err := repo.ReplaceUserRoles(ctx, outletBOnlyUser, []RoleAssignment{{ID: id.New(), RoleID: cashierRoleID, OutletID: &outletBID}}, now); err != nil {
		t.Fatalf("assigning outlet-B role: %v", err)
	}

	svc := NewService(repo, NewTokenSigner([]byte("integration-test-key")), NewInMemoryRefreshStore(), NewInMemoryRateLimiter(), nil, time.Minute, time.Hour)

	// since_version=0: tenant-wide + outlet-A users are eligible for outlet
	// A; outlet-B-only user must not appear.
	entries, err := svc.ListEdgeUserCache(ctx, tenantID, outletA, 0)
	if err != nil {
		t.Fatalf("listing edge user cache: %v", err)
	}
	if len(entries) != 2 {
		t.Fatalf("expected 2 entries for outlet A, got %d: %+v", len(entries), entries)
	}

	byID := map[string]EdgeUserCacheEntry{}
	for _, e := range entries {
		byID[e.ID] = e
	}

	tenantWideEntry, ok := byID[tenantWideUser]
	if !ok {
		t.Fatal("expected tenant-wide user in outlet A's cache")
	}
	if tenantWideEntry.OutletID != outletA {
		t.Errorf("expected outlet_id to be the requested outlet, got %q", tenantWideEntry.OutletID)
	}
	if tenantWideEntry.PasswordHash != "hash-tenant-wide" {
		t.Errorf("expected password_hash to round-trip, got %q", tenantWideEntry.PasswordHash)
	}
	if tenantWideEntry.PinHash == nil || *tenantWideEntry.PinHash != pinHash {
		t.Errorf("expected pin_hash to round-trip as set, got %v", tenantWideEntry.PinHash)
	}
	if len(tenantWideEntry.Permissions) != 1 || tenantWideEntry.Permissions[0] != PermissionOrderCreate {
		t.Errorf("expected flattened cashier permissions, got %v", tenantWideEntry.Permissions)
	}

	outletAEntry, ok := byID[outletAUser]
	if !ok {
		t.Fatal("expected outlet-A-scoped user in outlet A's cache")
	}
	if outletAEntry.PinHash != nil {
		t.Errorf("expected pin_hash nil for outlet-A user, got %v", *outletAEntry.PinHash)
	}

	if _, ok := byID[outletBOnlyUser]; ok {
		t.Fatal("expected outlet-B-scoped user to be absent from outlet A's cache")
	}

	// since_version filtering: tenantWideUser (config_version 5) survives a
	// floor of 4, but outletAUser (config_version 3) does not.
	filtered, err := svc.ListEdgeUserCache(ctx, tenantID, outletA, 4)
	if err != nil {
		t.Fatalf("listing with since_version=4: %v", err)
	}
	if len(filtered) != 1 || filtered[0].ID != tenantWideUser {
		t.Fatalf("expected only the tenant-wide user above since_version=4, got %+v", filtered)
	}

	// A floor above every config_version returns nothing.
	none, err := svc.ListEdgeUserCache(ctx, tenantID, outletA, 100)
	if err != nil {
		t.Fatalf("listing with since_version=100: %v", err)
	}
	if len(none) != 0 {
		t.Fatalf("expected no entries above since_version=100, got %+v", none)
	}
}

// refreshFixture creates the tenant/outlet/user rows a PostgresRefreshStore
// test needs to satisfy refresh_token's foreign keys, and registers cleanup.
func refreshFixture(t *testing.T, pool postgres.Pool) (userID, outletID string) {
	t.Helper()
	ctx := context.Background()

	tenantID := id.New()
	outletID = id.New()
	userID = id.New()

	if _, err := pool.Exec(ctx, `INSERT INTO tenant (id, name, created_at, updated_at) VALUES ($1, $2, now(), now())`, tenantID, "Refresh Store Test Tenant"); err != nil {
		t.Fatalf("inserting tenant fixture: %v", err)
	}
	if _, err := pool.Exec(ctx, `INSERT INTO outlet (id, tenant_id, name, timezone, created_at, updated_at) VALUES ($1, $2, 'Test Outlet', 'Asia/Kolkata', now(), now())`, outletID, tenantID); err != nil {
		t.Fatalf("inserting outlet fixture: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO app_user (id, tenant_id, email, full_name, password_hash, is_active, config_version, created_at, updated_at)
		VALUES ($1, $2, 'refresh-fixture@example.com', 'Refresh Fixture', 'unused', TRUE, 0, now(), now())
	`, userID, tenantID); err != nil {
		t.Fatalf("inserting user fixture: %v", err)
	}

	t.Cleanup(func() {
		pool.Exec(ctx, `DELETE FROM refresh_token WHERE user_id = $1`, userID)
		pool.Exec(ctx, `DELETE FROM app_user WHERE id = $1`, userID)
		pool.Exec(ctx, `DELETE FROM outlet WHERE id = $1`, outletID)
		pool.Exec(ctx, `DELETE FROM tenant WHERE id = $1`, tenantID)
	})

	return userID, outletID
}

func TestIntegration_PostgresRefreshStore_IssueAndRotate(t *testing.T) {
	pool := testPool(t)
	userID, outletID := refreshFixture(t, pool)
	store := NewPostgresRefreshStore(pool)

	ctx := context.Background()
	token, err := store.Issue(ctx, userID, outletID, time.Hour)
	if err != nil {
		t.Fatalf("issue: %v", err)
	}

	next, gotUserID, gotOutletID, err := store.Rotate(ctx, token, time.Hour)
	if err != nil {
		t.Fatalf("rotate: %v", err)
	}
	if gotUserID != userID || gotOutletID != outletID {
		t.Fatalf("unexpected identity from rotate: %s %s", gotUserID, gotOutletID)
	}
	if next == token {
		t.Fatal("expected a distinct successor token")
	}

	// The persisted row for the rotated-away token must carry used_at and
	// replaced_by_id set in the same transaction as the successor insert.
	var usedAt *time.Time
	var replacedByID *string
	if err := pool.QueryRow(ctx, `SELECT used_at, replaced_by_id FROM refresh_token WHERE token_hash = $1`, hashToken(token)).Scan(&usedAt, &replacedByID); err != nil {
		t.Fatalf("checking rotated row: %v", err)
	}
	if usedAt == nil || replacedByID == nil {
		t.Fatal("expected used_at and replaced_by_id to be set on the rotated-away token")
	}
}

func TestIntegration_PostgresRefreshStore_ReuseRevokesFamily(t *testing.T) {
	pool := testPool(t)
	userID, outletID := refreshFixture(t, pool)
	store := NewPostgresRefreshStore(pool)

	ctx := context.Background()
	token, err := store.Issue(ctx, userID, outletID, time.Hour)
	if err != nil {
		t.Fatalf("issue: %v", err)
	}
	next, _, _, err := store.Rotate(ctx, token, time.Hour)
	if err != nil {
		t.Fatalf("rotate: %v", err)
	}

	// Reusing the rotated-away token must revoke the whole family.
	if _, _, _, err := store.Rotate(ctx, token, time.Hour); err != ErrInvalidToken {
		t.Fatalf("expected reuse of rotated token to be rejected, got %v", err)
	}
	if _, _, _, err := store.Rotate(ctx, next, time.Hour); err != ErrInvalidToken {
		t.Fatalf("expected reuse to invalidate the whole family, got %v", err)
	}

	var revokedCount int
	if err := pool.QueryRow(ctx, `
		SELECT count(*) FROM refresh_token
		WHERE user_id = $1 AND revoked_at IS NOT NULL
	`, userID).Scan(&revokedCount); err != nil {
		t.Fatalf("counting revoked rows: %v", err)
	}
	if revokedCount == 0 {
		t.Fatal("expected every row in the family to be revoked")
	}
}

// TestIntegration_PostgresRefreshStore_SurvivesRestart proves rotation state
// is Postgres-backed, not process-local: a second store constructed over the
// same pool (simulating a backend restart) can still validate a token issued
// by the first, and reuse detected by the second store still revokes the
// family for both.
func TestIntegration_PostgresRefreshStore_SurvivesRestart(t *testing.T) {
	pool := testPool(t)
	userID, outletID := refreshFixture(t, pool)
	ctx := context.Background()

	firstProcessStore := NewPostgresRefreshStore(pool)
	token, err := firstProcessStore.Issue(ctx, userID, outletID, time.Hour)
	if err != nil {
		t.Fatalf("issue: %v", err)
	}

	// "Restart": a brand new store instance, sharing only the Postgres pool.
	secondProcessStore := NewPostgresRefreshStore(pool)

	next, gotUserID, gotOutletID, err := secondProcessStore.Rotate(ctx, token, time.Hour)
	if err != nil {
		t.Fatalf("rotate on second store instance: %v", err)
	}
	if gotUserID != userID || gotOutletID != outletID {
		t.Fatalf("unexpected identity after restart: %s %s", gotUserID, gotOutletID)
	}

	// Reuse against the token the first store issued, presented to the
	// second store instance, must still revoke the whole family.
	if _, _, _, err := secondProcessStore.Rotate(ctx, token, time.Hour); err != ErrInvalidToken {
		t.Fatalf("expected reuse to be rejected after restart, got %v", err)
	}
	if _, _, _, err := firstProcessStore.Rotate(ctx, next, time.Hour); err != ErrInvalidToken {
		t.Fatalf("expected family revoked by second store to be visible to the first, got %v", err)
	}
}
