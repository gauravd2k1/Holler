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
	refresh := NewRefreshStore()
	svc := NewService(repo, signer, refresh, auditor, time.Minute, time.Hour)
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

	result, err := svc.Login(ctx, tenantID, "integration@example.com", "correct-horse-battery", outletID)
	if err != nil {
		t.Fatalf("login: %v", err)
	}
	if result.Principal.UserID != userID {
		t.Fatalf("unexpected principal: %+v", result.Principal)
	}
}
