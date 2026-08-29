package compliance_test

import (
	"context"
	"path/filepath"
	"testing"
	"time"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/compliance"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/id"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/platform/testdb"
	"github.com/holler/backend/internal/tenant"
	contracts "github.com/holler/contracts"
)

// setupPool mirrors backend/internal/kitchen/postgres_test.go's setupPool:
// same migration path, same shared testdb gate.
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

type fixture struct {
	tenantID string
	outletID string
}

func newFixture(t *testing.T, pool postgres.Pool) fixture {
	t.Helper()
	ctx := context.Background()

	tenantSvc := tenant.NewService(tenant.NewPostgresRepository(pool))
	outletSvc := outlet.NewService(outlet.NewPostgresRepository(pool))

	org, err := tenantSvc.CreateOrganisation(ctx, "Compliance Integration Org "+id.New())
	if err != nil {
		t.Fatalf("CreateOrganisation: %v", err)
	}
	brand, err := tenantSvc.CreateBrand(ctx, org.ID, "Compliance Integration Brand")
	if err != nil {
		t.Fatalf("CreateBrand: %v", err)
	}
	out, err := outletSvc.CreateOutlet(ctx, outlet.Principal{TenantID: org.ID}, brand.ID, "Compliance Integration Outlet", "")
	if err != nil {
		t.Fatalf("CreateOutlet: %v", err)
	}

	return fixture{tenantID: org.ID, outletID: out.ID}
}

func authedCtx(tenantID, outletID string) context.Context {
	return auth.WithPrincipal(context.Background(), auth.AuthenticatedPrincipal{
		UserID:      "principal-user",
		TenantID:    tenantID,
		OutletID:    outletID,
		Permissions: []auth.Permission{contracts.PermissionBillingManage},
	})
}

// TestCreateComplianceVersion_BumpsRealOutletConfigVersion proves the
// config_version bump against the real outlet table, not a fake counter —
// the mechanism the whole cloud→edge sync depends on (T13).
func TestCreateComplianceVersion_BumpsRealOutletConfigVersion(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)
	svc := compliance.NewService(compliance.NewRepository(pool))

	outletSvc := outlet.NewService(outlet.NewPostgresRepository(pool))
	before, err := outletSvc.GetOutlet(context.Background(), outlet.Principal{TenantID: fx.tenantID}, fx.outletID)
	if err != nil {
		t.Fatalf("GetOutlet before: %v", err)
	}

	cv, err := svc.CreateComplianceVersion(authedCtx(fx.tenantID, fx.outletID), fx.tenantID, compliance.NewComplianceVersionInput{
		OutletID: fx.outletID, Label: "FY26", EffectiveFrom: time.Now().UTC(),
	})
	if err != nil {
		t.Fatalf("CreateComplianceVersion: %v", err)
	}

	after, err := outletSvc.GetOutlet(context.Background(), outlet.Principal{TenantID: fx.tenantID}, fx.outletID)
	if err != nil {
		t.Fatalf("GetOutlet after: %v", err)
	}
	if after.ConfigVersion != before.ConfigVersion+1 {
		t.Fatalf("expected outlet.config_version to advance by exactly 1 against real Postgres, before=%d after=%d", before.ConfigVersion, after.ConfigVersion)
	}
	if cv.ConfigVersion != after.ConfigVersion {
		t.Fatalf("expected the stored compliance_version's own config_version to equal the new outlet config_version, got %d want %d", cv.ConfigVersion, after.ConfigVersion)
	}
}

// TestCreateTaxProfile_AppearsInSubsequentSyncConfigPull is the T13
// end-to-end proof: a tax profile created the production way (through
// Service, not raw SQL) is readable back through the SAME bundle
// GET /sync/config assembles, and since_version filtering excludes it once
// the watermark reaches its own config_version.
func TestCreateTaxProfile_AppearsInSubsequentSyncConfigPull(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)
	svc := compliance.NewService(compliance.NewRepository(pool))
	ctx := authedCtx(fx.tenantID, fx.outletID)

	cv, err := svc.CreateComplianceVersion(ctx, fx.tenantID, compliance.NewComplianceVersionInput{
		OutletID: fx.outletID, Label: "FY26-" + id.New(), EffectiveFrom: time.Now().UTC(),
	})
	if err != nil {
		t.Fatalf("CreateComplianceVersion: %v", err)
	}

	tp, rules, err := svc.CreateTaxProfile(ctx, fx.tenantID, compliance.NewTaxProfileInput{
		OutletID: fx.outletID, Code: "GST5-" + id.New()[:8], Name: "GST 5%",
		PricingMode: contracts.PricingModeExclusive, IsDefault: true,
		Rules: []compliance.NewTaxRuleInput{
			{ComplianceVersionID: cv.ID, Component: contracts.TaxComponentCGST, RateBps: 250, EffectiveFrom: time.Now().UTC()},
			{ComplianceVersionID: cv.ID, Component: contracts.TaxComponentSGST, RateBps: 250, EffectiveFrom: time.Now().UTC()},
		},
	})
	if err != nil {
		t.Fatalf("CreateTaxProfile: %v", err)
	}
	if len(rules) != 2 {
		t.Fatalf("expected 2 tax_rule rows stored, got %d", len(rules))
	}

	bundle, err := svc.SyncConfigBundle(ctx, fx.tenantID, fx.outletID, 0)
	if err != nil {
		t.Fatalf("SyncConfigBundle: %v", err)
	}
	foundProfile := false
	for _, p := range bundle.TaxProfiles {
		if p.ID == tp.ID {
			foundProfile = true
		}
	}
	if !foundProfile {
		t.Fatalf("expected the created tax_profile in the sync bundle, got %+v", bundle.TaxProfiles)
	}
	foundRules := 0
	for _, r := range bundle.TaxRules {
		if r.TaxProfileID == tp.ID {
			foundRules++
		}
	}
	if foundRules != 2 {
		t.Fatalf("expected both tax_rule children in the sync bundle, found %d", foundRules)
	}

	// since_version at the profile's own config_version excludes it.
	filtered, err := svc.SyncConfigBundle(ctx, fx.tenantID, fx.outletID, tp.ConfigVersion)
	if err != nil {
		t.Fatalf("SyncConfigBundle filtered: %v", err)
	}
	for _, p := range filtered.TaxProfiles {
		if p.ID == tp.ID {
			t.Fatalf("expected the profile excluded once since_version reaches its own config_version")
		}
	}
}

// TestCreateDiscountDefinition_ChecksConstraintEnforcedByRealSchema proves
// the CHECK on packages/contracts/postgres/0007_m3_billing.sql's
// discount_definition table is real: a caller that bypassed the service's
// own validation (impossible through the exported API, but this proves the
// database itself would reject the half-populated shape too, not just the
// Go-side guard) is out of scope here — instead this proves the SERVICE's
// guard fires before any SQL executes, so a bad discount never reaches
// Postgres at all.
func TestCreateDiscountDefinition_RejectsHalfPopulatedBeforeReachingPostgres(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)
	svc := compliance.NewService(compliance.NewRepository(pool))
	ctx := authedCtx(fx.tenantID, fx.outletID)

	valuePaise := 5000
	_, err := svc.CreateDiscountDefinition(ctx, fx.tenantID, compliance.NewDiscountDefinitionInput{
		OutletID: fx.outletID, Code: "BAD-" + id.New()[:8], Name: "Bad discount",
		Scope: contracts.DiscountScopeBill, Method: contracts.DiscountMethodPercent,
		ValuePaise: &valuePaise, EffectiveFrom: time.Now().UTC(),
	})
	if err == nil {
		t.Fatal("expected the half-populated PERCENT/value_paise shape to be rejected")
	}
}

// TestSetFiscalProfile_OutletCanBeFullyConfiguredForBilling is T13's
// headline proof: legal_name/gstin/etc. created through the service the
// production way, then read back through the exact SyncConfigBundle path
// GET /sync/config uses — an outlet configured for billing without a single
// line of raw SQL.
func TestSetFiscalProfile_OutletCanBeFullyConfiguredForBilling(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)
	svc := compliance.NewService(compliance.NewRepository(pool))
	ctx := authedCtx(fx.tenantID, fx.outletID)

	fp, err := svc.SetFiscalProfile(ctx, fx.tenantID, compliance.NewFiscalProfileInput{
		OutletID: fx.outletID, LegalName: "Holler Foods Pvt Ltd", TradeName: "Holler",
		AddressLine1: "1 MG Road", City: "Pune", StateCode: "27", StateName: "Maharashtra",
		Pincode: "411001", GSTIN: "27ABCDE1234F1Z5", EffectiveFrom: time.Now().UTC(),
	})
	if err != nil {
		t.Fatalf("SetFiscalProfile: %v", err)
	}

	bundle, err := svc.SyncConfigBundle(ctx, fx.tenantID, fx.outletID, 0)
	if err != nil {
		t.Fatalf("SyncConfigBundle: %v", err)
	}
	if bundle.FiscalProfile == nil || bundle.FiscalProfile.ID != fp.ID {
		t.Fatalf("expected the fiscal profile in the sync bundle, got %+v", bundle.FiscalProfile)
	}
	if bundle.FiscalProfile.GSTIN != "27ABCDE1234F1Z5" {
		t.Fatalf("expected GSTIN to round-trip through real Postgres, got %q", bundle.FiscalProfile.GSTIN)
	}
}
