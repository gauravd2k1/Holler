package tenant_test

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/tenant"
)

// setupPool opens a pool against HOLLER_TEST_DATABASE_URL and applies the
// frozen contracts migrations. Every test in this file skips, not fails,
// when the env var is unset — CI without a live Postgres still passes.
func setupPool(t *testing.T) postgres.Pool {
	t.Helper()

	dbURL := os.Getenv("HOLLER_TEST_DATABASE_URL")
	if dbURL == "" {
		t.Skip("HOLLER_TEST_DATABASE_URL not set; skipping tenant Postgres integration test")
	}

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

func TestPostgresRepository_CreateAndScopeBrand(t *testing.T) {
	pool := setupPool(t)
	repo := tenant.NewPostgresRepository(pool)
	svc := tenant.NewService(repo)
	ctx := context.Background()

	orgA, err := svc.CreateOrganisation(ctx, "Integration Org A")
	if err != nil {
		t.Fatalf("CreateOrganisation A: %v", err)
	}
	orgB, err := svc.CreateOrganisation(ctx, "Integration Org B")
	if err != nil {
		t.Fatalf("CreateOrganisation B: %v", err)
	}

	brand, err := svc.CreateBrand(ctx, orgA.ID, "Integration Brand A")
	if err != nil {
		t.Fatalf("CreateBrand: %v", err)
	}

	if _, err := svc.BrandForTenant(ctx, orgA.ID, brand.ID); err != nil {
		t.Fatalf("owning tenant should see its brand: %v", err)
	}

	if _, err := svc.BrandForTenant(ctx, orgB.ID, brand.ID); !errors.Is(err, httpx.ErrNotFound) {
		t.Fatalf("expected ErrNotFound for cross-tenant brand lookup, got %v", err)
	}
}
