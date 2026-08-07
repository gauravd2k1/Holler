package tenant

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/holler/backend/internal/platform/httpx"
)

type fakeRepo struct {
	tenants map[string]Tenant
	brands  map[string]Brand
}

func newFakeRepo() *fakeRepo {
	return &fakeRepo{tenants: map[string]Tenant{}, brands: map[string]Brand{}}
}

func (f *fakeRepo) InsertTenant(_ context.Context, t Tenant) error {
	f.tenants[t.ID] = t
	return nil
}

func (f *fakeRepo) InsertBrand(_ context.Context, b Brand) error {
	f.brands[b.ID] = b
	return nil
}

func (f *fakeRepo) GetBrand(_ context.Context, tenantID, brandID string) (Brand, error) {
	b, ok := f.brands[brandID]
	if !ok || b.TenantID != tenantID {
		return Brand{}, httpx.ErrNotFound
	}
	return b, nil
}

func TestCreateOrganisation(t *testing.T) {
	svc := NewService(newFakeRepo())

	got, err := svc.CreateOrganisation(context.Background(), "  Spice Route  ")
	if err != nil {
		t.Fatalf("CreateOrganisation: %v", err)
	}
	if got.ID == "" {
		t.Fatal("expected a generated id")
	}
	if got.Name != "Spice Route" {
		t.Fatalf("expected trimmed name, got %q", got.Name)
	}
	if got.CreatedAt.Location() != time.UTC {
		t.Fatal("expected UTC timestamp")
	}
}

func TestCreateOrganisation_RejectsBlankName(t *testing.T) {
	svc := NewService(newFakeRepo())

	_, err := svc.CreateOrganisation(context.Background(), "   ")
	if !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput, got %v", err)
	}
}

func TestCreateBrand_RequiresTenantID(t *testing.T) {
	svc := NewService(newFakeRepo())

	_, err := svc.CreateBrand(context.Background(), "", "Spice Route Pune")
	if !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput, got %v", err)
	}
}

func TestCreateBrand_ThenLookupIsTenantScoped(t *testing.T) {
	repo := newFakeRepo()
	svc := NewService(repo)
	ctx := context.Background()

	orgA, err := svc.CreateOrganisation(ctx, "Org A")
	if err != nil {
		t.Fatalf("CreateOrganisation: %v", err)
	}
	orgB, err := svc.CreateOrganisation(ctx, "Org B")
	if err != nil {
		t.Fatalf("CreateOrganisation: %v", err)
	}

	brand, err := svc.CreateBrand(ctx, orgA.ID, "Brand A")
	if err != nil {
		t.Fatalf("CreateBrand: %v", err)
	}

	if _, err := svc.BrandForTenant(ctx, orgA.ID, brand.ID); err != nil {
		t.Fatalf("expected brand visible to owning tenant, got %v", err)
	}

	// Cross-tenant lookup: tenant B must not be able to resolve tenant A's
	// brand by id, even though the id itself is valid and exists.
	if _, err := svc.BrandForTenant(ctx, orgB.ID, brand.ID); !errors.Is(err, httpx.ErrNotFound) {
		t.Fatalf("expected ErrNotFound for cross-tenant brand lookup, got %v", err)
	}
}
