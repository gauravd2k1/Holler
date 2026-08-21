package outlet

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/holler/backend/internal/platform/httpx"
)

type fakeRepo struct {
	outlets     map[string]Outlet
	brandTenant map[string]string // brandID -> tenantID
	// onConfigVersionRead, if set, overrides GetByID's returned
	// ConfigVersion. Used only by device_service_test.go's
	// newDeviceTestFixture, which splits DeviceRepository and Repository
	// into two separate fakes the way production's single PostgresRepository
	// never does, so a config_version bump made through one fake needs an
	// explicit way to become visible through the other.
	onConfigVersionRead func(outletID string) int
}

func newFakeRepo() *fakeRepo {
	return &fakeRepo{outlets: map[string]Outlet{}, brandTenant: map[string]string{}}
}

func (f *fakeRepo) Insert(_ context.Context, tenantID string, o Outlet) error {
	if f.brandTenant[o.BrandID] != tenantID {
		return httpx.ErrNotFound
	}
	f.outlets[o.ID] = o
	return nil
}

func (f *fakeRepo) ListByTenant(_ context.Context, tenantID string) ([]Outlet, error) {
	var out []Outlet
	for _, o := range f.outlets {
		if f.brandTenant[o.BrandID] == tenantID {
			out = append(out, o)
		}
	}
	return out, nil
}

func (f *fakeRepo) GetByID(_ context.Context, tenantID, outletID string) (Outlet, error) {
	o, ok := f.outlets[outletID]
	if !ok || f.brandTenant[o.BrandID] != tenantID {
		return Outlet{}, httpx.ErrNotFound
	}
	if f.onConfigVersionRead != nil {
		o.ConfigVersion = f.onConfigVersionRead(outletID)
	}
	return o, nil
}

func (f *fakeRepo) UpdateDayStartTime(_ context.Context, tenantID, outletID, dayStartTime string) (Outlet, error) {
	o, ok := f.outlets[outletID]
	if !ok || f.brandTenant[o.BrandID] != tenantID {
		return Outlet{}, httpx.ErrNotFound
	}
	o.DayStartTime = dayStartTime
	o.ConfigVersion++
	f.outlets[outletID] = o
	return o, nil
}

func TestCreateOutlet_RejectsBrandFromAnotherTenant(t *testing.T) {
	repo := newFakeRepo()
	repo.brandTenant["brand-a"] = "tenant-a"
	svc := NewService(repo)

	_, err := svc.CreateOutlet(context.Background(), Principal{TenantID: "tenant-b"}, "brand-a", "Outlet 1", "")
	if !errors.Is(err, httpx.ErrNotFound) {
		t.Fatalf("expected ErrNotFound creating outlet under another tenant's brand, got %v", err)
	}
}

func TestCreateOutlet_AppliesDefaultTimezone(t *testing.T) {
	repo := newFakeRepo()
	repo.brandTenant["brand-a"] = "tenant-a"
	svc := NewService(repo)

	got, err := svc.CreateOutlet(context.Background(), Principal{TenantID: "tenant-a"}, "brand-a", "Outlet 1", "")
	if err != nil {
		t.Fatalf("CreateOutlet: %v", err)
	}
	if got.Timezone != defaultTimezone {
		t.Fatalf("expected default timezone %q, got %q", defaultTimezone, got.Timezone)
	}
	if got.CreatedAt.Location() != time.UTC {
		t.Fatal("expected UTC timestamp")
	}
}

func TestCreateOutlet_RequiresName(t *testing.T) {
	repo := newFakeRepo()
	repo.brandTenant["brand-a"] = "tenant-a"
	svc := NewService(repo)

	_, err := svc.CreateOutlet(context.Background(), Principal{TenantID: "tenant-a"}, "brand-a", "  ", "")
	if !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput, got %v", err)
	}
}

func TestListOutlets_IsTenantScoped(t *testing.T) {
	repo := newFakeRepo()
	repo.brandTenant["brand-a"] = "tenant-a"
	repo.brandTenant["brand-b"] = "tenant-b"
	svc := NewService(repo)
	ctx := context.Background()

	outletA, err := svc.CreateOutlet(ctx, Principal{TenantID: "tenant-a"}, "brand-a", "Outlet A", "")
	if err != nil {
		t.Fatalf("CreateOutlet A: %v", err)
	}
	if _, err := svc.CreateOutlet(ctx, Principal{TenantID: "tenant-b"}, "brand-b", "Outlet B", ""); err != nil {
		t.Fatalf("CreateOutlet B: %v", err)
	}

	got, err := svc.ListOutlets(ctx, Principal{TenantID: "tenant-a"})
	if err != nil {
		t.Fatalf("ListOutlets: %v", err)
	}
	if len(got) != 1 || got[0].ID != outletA.ID {
		t.Fatalf("expected only tenant A's outlet, got %+v", got)
	}
}

// TestGetOutlet_CrossTenantIsNotFound is the dedicated cross-tenant
// isolation test required by docs/spec/security-rbac.md §Tenant isolation:
// a request carrying tenant A's principal and tenant B's outlet id must
// 404, not 403-with-leak and not 200.
func TestGetOutlet_CrossTenantIsNotFound(t *testing.T) {
	repo := newFakeRepo()
	repo.brandTenant["brand-a"] = "tenant-a"
	repo.brandTenant["brand-b"] = "tenant-b"
	svc := NewService(repo)
	ctx := context.Background()

	outletB, err := svc.CreateOutlet(ctx, Principal{TenantID: "tenant-b"}, "brand-b", "Outlet B", "")
	if err != nil {
		t.Fatalf("CreateOutlet B: %v", err)
	}

	// Sanity: tenant B can fetch its own outlet.
	if _, err := svc.GetOutlet(ctx, Principal{TenantID: "tenant-b"}, outletB.ID); err != nil {
		t.Fatalf("owning tenant should be able to fetch its outlet: %v", err)
	}

	// The actual test: tenant A's principal, tenant B's outlet id.
	_, err = svc.GetOutlet(ctx, Principal{TenantID: "tenant-a"}, outletB.ID)
	if !errors.Is(err, httpx.ErrNotFound) {
		t.Fatalf("expected ErrNotFound for cross-tenant outlet lookup, got %v", err)
	}
}

func TestGetOutlet_RequiresPrincipal(t *testing.T) {
	svc := NewService(newFakeRepo())
	_, err := svc.GetOutlet(context.Background(), Principal{}, "some-id")
	if !errors.Is(err, httpx.ErrUnauthorized) {
		t.Fatalf("expected ErrUnauthorized without a tenant id, got %v", err)
	}
}
