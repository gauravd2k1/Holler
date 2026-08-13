package outlet_test

import (
	"context"
	"errors"
	"testing"

	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/tenant"
)

// TestPostgresDeviceService_EnrollRotateRevoke_EndToEnd runs the full device
// lifecycle against a real Postgres, driven through
// packages/contracts/postgres/0008_device_enrollment.sql. Skips (does not
// fail) if HOLLER_TEST_DATABASE_URL is unset — see setupPool.
func TestPostgresDeviceService_EnrollRotateRevoke_EndToEnd(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()

	tenantSvc := tenant.NewService(tenant.NewPostgresRepository(pool))
	outletsRepo := outlet.NewPostgresRepository(pool)
	outletSvc := outlet.NewService(outletsRepo)
	deviceSvc := outlet.NewDeviceService(outletsRepo, outletsRepo, nil)

	org, err := tenantSvc.CreateOrganisation(ctx, "Integration Device Org")
	if err != nil {
		t.Fatalf("CreateOrganisation: %v", err)
	}
	brand, err := tenantSvc.CreateBrand(ctx, org.ID, "Integration Device Brand")
	if err != nil {
		t.Fatalf("CreateBrand: %v", err)
	}
	principal := outlet.Principal{TenantID: org.ID}
	o, err := outletSvc.CreateOutlet(ctx, principal, brand.ID, "Integration Device Outlet", "")
	if err != nil {
		t.Fatalf("CreateOutlet: %v", err)
	}

	enrolled, err := deviceSvc.EnrollDevice(ctx, principal, o.ID, outlet.DeviceKindPOS, "POS-INTEGRATION-1", "install visit", nil)
	if err != nil {
		t.Fatalf("EnrollDevice: %v", err)
	}
	if enrolled.Token == "" {
		t.Fatal("expected a non-empty plaintext token")
	}

	p, err := deviceSvc.VerifyToken(ctx, enrolled.Token)
	if err != nil {
		t.Fatalf("VerifyToken on freshly enrolled device: %v", err)
	}
	if p.DeviceID != enrolled.Device.ID || p.TenantID != org.ID || p.OutletID != o.ID {
		t.Fatalf("unexpected device principal: %+v", p)
	}

	// Re-enrolling the same (outlet, name) while a credential is active is a
	// conflict, not a silent replacement.
	if _, err := deviceSvc.EnrollDevice(ctx, principal, o.ID, outlet.DeviceKindPOS, "POS-INTEGRATION-1", "", nil); !errors.Is(err, httpx.ErrConflict) {
		t.Fatalf("expected ErrConflict re-enrolling a live device, got %v", err)
	}

	rotated, err := deviceSvc.RotateCredential(ctx, principal, enrolled.Device.ID, "rotation 1", nil)
	if err != nil {
		t.Fatalf("RotateCredential: %v", err)
	}
	if rotated.Token == enrolled.Token {
		t.Fatal("expected rotation to mint a distinct token")
	}
	if _, err := deviceSvc.VerifyToken(ctx, enrolled.Token); !errors.Is(err, httpx.ErrUnauthorized) {
		t.Fatalf("expected the pre-rotation token to be rejected, got %v", err)
	}
	if _, err := deviceSvc.VerifyToken(ctx, rotated.Token); err != nil {
		t.Fatalf("expected the post-rotation token to verify: %v", err)
	}

	if err := deviceSvc.RevokeCredential(ctx, principal, enrolled.Device.ID, nil); err != nil {
		t.Fatalf("RevokeCredential: %v", err)
	}
	if _, err := deviceSvc.VerifyToken(ctx, rotated.Token); !errors.Is(err, httpx.ErrUnauthorized) {
		t.Fatalf("expected the revoked device's token to be rejected, got %v", err)
	}
}

// TestPostgresDeviceService_EnrollCrossTenantOutlet_IsNotFound is the
// dedicated cross-tenant isolation test for device enrollment, mirroring
// TestPostgresRepository_CrossTenantOutletLookupIsNotFound.
func TestPostgresDeviceService_EnrollCrossTenantOutlet_IsNotFound(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()

	tenantSvc := tenant.NewService(tenant.NewPostgresRepository(pool))
	outletsRepo := outlet.NewPostgresRepository(pool)
	outletSvc := outlet.NewService(outletsRepo)
	deviceSvc := outlet.NewDeviceService(outletsRepo, outletsRepo, nil)

	orgA, err := tenantSvc.CreateOrganisation(ctx, "Device Isolation Org A")
	if err != nil {
		t.Fatalf("CreateOrganisation A: %v", err)
	}
	orgB, err := tenantSvc.CreateOrganisation(ctx, "Device Isolation Org B")
	if err != nil {
		t.Fatalf("CreateOrganisation B: %v", err)
	}
	brandB, err := tenantSvc.CreateBrand(ctx, orgB.ID, "Device Isolation Brand B")
	if err != nil {
		t.Fatalf("CreateBrand B: %v", err)
	}
	outletB, err := outletSvc.CreateOutlet(ctx, outlet.Principal{TenantID: orgB.ID}, brandB.ID, "Device Isolation Outlet B", "")
	if err != nil {
		t.Fatalf("CreateOutlet B: %v", err)
	}

	_, err = deviceSvc.EnrollDevice(ctx, outlet.Principal{TenantID: orgA.ID}, outletB.ID, outlet.DeviceKindPOS, "POS-X", "", nil)
	if !errors.Is(err, httpx.ErrNotFound) {
		t.Fatalf("expected ErrNotFound enrolling a device at another tenant's outlet, got %v", err)
	}
}
