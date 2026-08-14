package outlet_test

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
	"github.com/holler/backend/internal/tenant"
)

// TestPostgresDeviceService_EnrollRotateRevoke_EndToEnd runs the full device
// lifecycle against a real Postgres, driven through
// packages/contracts/postgres/0008_device_enrollment.sql. Fails loudly (does
// not skip silently) if HOLLER_TEST_DATABASE_URL is unset — see setupPool
// in postgres_test.go, which uses internal/platform/testdb.
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

// TestPostgresListEdgeDeviceCredentials_ScopesToOutletNotJustTenant is the
// falsifying test for T13 retry DEFECT 2: a previous gate removed the
// outlet_id predicate from ListEdgeCredentials' query, keeping only
// tenant_id, and every existing test still passed. Two branches under one
// tenant is the ORDINARY case, not an exotic one — GET /sync/config carries
// Argon2id hash material on the wire, so a pull scoped to outlet A must
// return only A's credential and never outlet B's, even though both outlets
// share a tenant.
func TestPostgresListEdgeDeviceCredentials_ScopesToOutletNotJustTenant(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()

	tenantSvc := tenant.NewService(tenant.NewPostgresRepository(pool))
	outletsRepo := outlet.NewPostgresRepository(pool)
	outletSvc := outlet.NewService(outletsRepo)
	deviceSvc := outlet.NewDeviceService(outletsRepo, outletsRepo, nil)

	org, err := tenantSvc.CreateOrganisation(ctx, "Two Branch Org")
	if err != nil {
		t.Fatalf("CreateOrganisation: %v", err)
	}
	brand, err := tenantSvc.CreateBrand(ctx, org.ID, "Two Branch Brand")
	if err != nil {
		t.Fatalf("CreateBrand: %v", err)
	}
	principal := outlet.Principal{TenantID: org.ID}

	outletA, err := outletSvc.CreateOutlet(ctx, principal, brand.ID, "Branch A", "")
	if err != nil {
		t.Fatalf("CreateOutlet A: %v", err)
	}
	outletB, err := outletSvc.CreateOutlet(ctx, principal, brand.ID, "Branch B", "")
	if err != nil {
		t.Fatalf("CreateOutlet B: %v", err)
	}

	enrolledA, err := deviceSvc.EnrollDevice(ctx, principal, outletA.ID, outlet.DeviceKindPOS, "POS-A-1", "", nil)
	if err != nil {
		t.Fatalf("EnrollDevice at outlet A: %v", err)
	}
	enrolledB, err := deviceSvc.EnrollDevice(ctx, principal, outletB.ID, outlet.DeviceKindPOS, "POS-B-1", "", nil)
	if err != nil {
		t.Fatalf("EnrollDevice at outlet B: %v", err)
	}

	credsA, err := deviceSvc.ListEdgeDeviceCredentials(ctx, org.ID, outletA.ID, 0)
	if err != nil {
		t.Fatalf("ListEdgeDeviceCredentials for outlet A: %v", err)
	}
	if len(credsA) != 1 {
		t.Fatalf("expected exactly one credential for outlet A, got %d: %+v", len(credsA), credsA)
	}
	if credsA[0].DeviceID != enrolledA.Device.ID {
		t.Fatalf("expected outlet A's credential to belong to its own device %s, got device %s", enrolledA.Device.ID, credsA[0].DeviceID)
	}
	for _, c := range credsA {
		if c.DeviceID == enrolledB.Device.ID {
			t.Fatalf("outlet A's credential pull leaked outlet B's device credential: %+v", c)
		}
	}

	credsB, err := deviceSvc.ListEdgeDeviceCredentials(ctx, org.ID, outletB.ID, 0)
	if err != nil {
		t.Fatalf("ListEdgeDeviceCredentials for outlet B: %v", err)
	}
	if len(credsB) != 1 {
		t.Fatalf("expected exactly one credential for outlet B, got %d: %+v", len(credsB), credsB)
	}
	if credsB[0].DeviceID != enrolledB.Device.ID {
		t.Fatalf("expected outlet B's credential to belong to its own device %s, got device %s", enrolledB.Device.ID, credsB[0].DeviceID)
	}
}

// TestPostgresRepository_WithTx_RealRollback_NoOrphanCredentialNoConfigBump
// closes the gap docs/RESUME.md recorded after T13 retry: atomicity of the
// credential-write-plus-config-version-bump pair had only ever been proven
// against fakeDeviceRepo's own hand-written snapshot/restore WithTx — a fake
// whose rollback semantics the same builder wrote. This drives a REAL
// pgx.Tx, not a fake, through PostgresRepository.WithTx directly: it inserts
// a device_credential row successfully, then forces the SECOND statement in
// the same transaction (BumpOutletConfigVersion) to fail by targeting an
// outlet id that does not exist, and asserts neither write survived —
// exactly the atomicity DeviceService.EnrollDevice/RotateCredential/
// RevokeCredential rely on WithTx to provide in production.
func TestPostgresRepository_WithTx_RealRollback_NoOrphanCredentialNoConfigBump(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()

	tenantSvc := tenant.NewService(tenant.NewPostgresRepository(pool))
	outletsRepo := outlet.NewPostgresRepository(pool)
	outletSvc := outlet.NewService(outletsRepo)

	org, err := tenantSvc.CreateOrganisation(ctx, "WithTx Rollback Org")
	if err != nil {
		t.Fatalf("CreateOrganisation: %v", err)
	}
	brand, err := tenantSvc.CreateBrand(ctx, org.ID, "WithTx Rollback Brand")
	if err != nil {
		t.Fatalf("CreateBrand: %v", err)
	}
	principal := outlet.Principal{TenantID: org.ID}
	o, err := outletSvc.CreateOutlet(ctx, principal, brand.ID, "WithTx Rollback Outlet", "")
	if err != nil {
		t.Fatalf("CreateOutlet: %v", err)
	}
	if o.ConfigVersion != 0 {
		t.Fatalf("expected a freshly created outlet to start at config_version 0, got %d", o.ConfigVersion)
	}

	device := outlet.Device{
		ID:        id.New(),
		OutletID:  o.ID,
		Kind:      outlet.DeviceKindPOS,
		Name:      "WithTx Rollback Device",
		CreatedAt: time.Now().UTC(),
		UpdatedAt: time.Now().UTC(),
	}
	if err := outletsRepo.InsertDevice(ctx, org.ID, device); err != nil {
		t.Fatalf("InsertDevice: %v", err)
	}

	credentialID := id.New()
	const bogusOutletID = "00000000-0000-0000-0000-000000000000"

	txErr := outletsRepo.WithTx(ctx, func(tx pgx.Tx) error {
		cred := outlet.DeviceCredential{
			ID:        credentialID,
			DeviceID:  device.ID,
			TenantID:  org.ID,
			OutletID:  o.ID,
			Label:     "rollback probe",
			CreatedAt: time.Now().UTC(),
		}
		if err := outletsRepo.InsertCredential(ctx, tx, cred, "not-a-real-hash"); err != nil {
			return err
		}
		// This targets an outlet id that does not exist, forcing
		// BumpOutletConfigVersion to return httpx.ErrNotFound — the second
		// statement in the pair fails AFTER the first one succeeded within
		// the same, still-open transaction.
		_, err := outletsRepo.BumpOutletConfigVersion(ctx, tx, bogusOutletID)
		return err
	})
	if txErr == nil {
		t.Fatal("expected WithTx to propagate the forced BumpOutletConfigVersion failure, got nil")
	}
	if !errors.Is(txErr, httpx.ErrNotFound) {
		t.Fatalf("expected the forced failure to be httpx.ErrNotFound, got %v", txErr)
	}

	// The credential insert must NOT have survived the rollback: no orphan
	// row, even though InsertCredential itself returned no error.
	var credentialCount int
	if err := pool.QueryRow(ctx, `SELECT COUNT(*) FROM device_credential WHERE id = $1`, credentialID).Scan(&credentialCount); err != nil {
		t.Fatalf("counting device_credential rows: %v", err)
	}
	if credentialCount != 0 {
		t.Fatalf("expected the credential insert to have been rolled back, found %d row(s) for id %s", credentialCount, credentialID)
	}

	// The real outlet's config_version must be untouched — the bump inside
	// the same failed transaction must not have advanced anything, even
	// though the bump that failed targeted a DIFFERENT (bogus) outlet id;
	// this proves the whole transaction rolled back, not just the statement
	// that errored.
	reread, err := outletSvc.GetOutlet(ctx, principal, o.ID)
	if err != nil {
		t.Fatalf("GetOutlet after rollback: %v", err)
	}
	if reread.ConfigVersion != 0 {
		t.Fatalf("expected outlet config_version to remain 0 after rollback, got %d", reread.ConfigVersion)
	}
}
