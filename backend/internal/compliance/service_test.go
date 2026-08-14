package compliance

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

const (
	testTenantID = "11111111-1111-7111-8111-111111111111"
	testOutletID = "22222222-2222-7222-8222-222222222222"
)

// fakeRepository is an in-memory Repository used to unit test Service
// without a database, mirroring backend/internal/kitchen's fakeRepository
// pattern.
type fakeRepository struct {
	outletVersions map[string]int
	outletTenant   map[string]string

	complianceVersions map[string]contracts.ComplianceVersion
	cvLabelTaken       map[string]bool // outletID|label

	taxProfiles map[string]contracts.TaxProfile
	tpCodeTaken map[string]bool // outletID|code

	taxRules map[string]contracts.TaxRule

	invoiceSeries   map[string]contracts.InvoiceSeries
	seriesCodeTaken map[string]bool

	discounts         map[string]contracts.DiscountDefinition
	discountCodeTaken map[string]bool

	fiscalProfiles map[string][]contracts.OutletFiscalProfile // outletID -> history

	bumpCalls int
}

func newFakeRepository() *fakeRepository {
	return &fakeRepository{
		outletVersions:     map[string]int{},
		outletTenant:       map[string]string{},
		complianceVersions: map[string]contracts.ComplianceVersion{},
		cvLabelTaken:       map[string]bool{},
		taxProfiles:        map[string]contracts.TaxProfile{},
		tpCodeTaken:        map[string]bool{},
		taxRules:           map[string]contracts.TaxRule{},
		invoiceSeries:      map[string]contracts.InvoiceSeries{},
		seriesCodeTaken:    map[string]bool{},
		discounts:          map[string]contracts.DiscountDefinition{},
		discountCodeTaken:  map[string]bool{},
		fiscalProfiles:     map[string][]contracts.OutletFiscalProfile{},
	}
}

func (f *fakeRepository) WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error {
	return fn(nil)
}

func (f *fakeRepository) BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error) {
	f.bumpCalls++
	if _, ok := f.outletTenant[outletID]; !ok {
		return 0, httpx.ErrNotFound
	}
	f.outletVersions[outletID]++
	return f.outletVersions[outletID], nil
}

func (f *fakeRepository) OutletBelongsToTenant(ctx context.Context, tenantID, outletID string) (bool, error) {
	return f.outletTenant[outletID] == tenantID, nil
}

func (f *fakeRepository) InsertComplianceVersion(ctx context.Context, tx pgx.Tx, cv contracts.ComplianceVersion) error {
	key := cv.OutletID + "|" + cv.Label
	if f.cvLabelTaken[key] {
		return httpx.ErrConflict
	}
	f.cvLabelTaken[key] = true
	f.complianceVersions[cv.ID] = cv
	return nil
}

func (f *fakeRepository) GetComplianceVersion(ctx context.Context, id string) (contracts.ComplianceVersion, error) {
	cv, ok := f.complianceVersions[id]
	if !ok {
		return contracts.ComplianceVersion{}, httpx.ErrNotFound
	}
	return cv, nil
}

func (f *fakeRepository) ComplianceVersionsSince(ctx context.Context, outletID string, sinceVersion int) ([]contracts.ComplianceVersion, error) {
	var out []contracts.ComplianceVersion
	for _, cv := range f.complianceVersions {
		if cv.OutletID == outletID && cv.ConfigVersion > sinceVersion {
			out = append(out, cv)
		}
	}
	return out, nil
}

func (f *fakeRepository) InsertTaxProfile(ctx context.Context, tx pgx.Tx, tp contracts.TaxProfile) error {
	key := tp.OutletID + "|" + tp.Code
	if f.tpCodeTaken[key] {
		return httpx.ErrConflict
	}
	f.tpCodeTaken[key] = true
	f.taxProfiles[tp.ID] = tp
	return nil
}

func (f *fakeRepository) GetTaxProfile(ctx context.Context, id string) (contracts.TaxProfile, error) {
	tp, ok := f.taxProfiles[id]
	if !ok {
		return contracts.TaxProfile{}, httpx.ErrNotFound
	}
	return tp, nil
}

func (f *fakeRepository) TaxProfilesSince(ctx context.Context, outletID string, sinceVersion int) ([]contracts.TaxProfile, error) {
	var out []contracts.TaxProfile
	for _, tp := range f.taxProfiles {
		if tp.OutletID == outletID && tp.ConfigVersion > sinceVersion {
			out = append(out, tp)
		}
	}
	return out, nil
}

func (f *fakeRepository) SetTaxProfileActive(ctx context.Context, tx pgx.Tx, id string, isActive bool, configVersion int) error {
	tp, ok := f.taxProfiles[id]
	if !ok {
		return httpx.ErrNotFound
	}
	tp.IsActive = isActive
	tp.ConfigVersion = configVersion
	f.taxProfiles[id] = tp
	return nil
}

func (f *fakeRepository) InsertTaxRule(ctx context.Context, tx pgx.Tx, tr contracts.TaxRule) error {
	f.taxRules[tr.ID] = tr
	return nil
}

func (f *fakeRepository) TaxRulesSince(ctx context.Context, outletID string, sinceVersion int) ([]contracts.TaxRule, error) {
	var out []contracts.TaxRule
	for _, tr := range f.taxRules {
		tp, ok := f.taxProfiles[tr.TaxProfileID]
		if !ok || tp.OutletID != outletID {
			continue
		}
		if tr.ConfigVersion > sinceVersion {
			out = append(out, tr)
		}
	}
	return out, nil
}

func (f *fakeRepository) InsertInvoiceSeries(ctx context.Context, tx pgx.Tx, s contracts.InvoiceSeries) error {
	key := s.OutletID + "|" + s.Code
	if f.seriesCodeTaken[key] {
		return httpx.ErrConflict
	}
	f.seriesCodeTaken[key] = true
	f.invoiceSeries[s.ID] = s
	return nil
}

func (f *fakeRepository) GetInvoiceSeries(ctx context.Context, id string) (contracts.InvoiceSeries, error) {
	s, ok := f.invoiceSeries[id]
	if !ok {
		return contracts.InvoiceSeries{}, httpx.ErrNotFound
	}
	return s, nil
}

func (f *fakeRepository) InvoiceSeriesSince(ctx context.Context, outletID string, sinceVersion int) ([]contracts.InvoiceSeries, error) {
	var out []contracts.InvoiceSeries
	for _, s := range f.invoiceSeries {
		if s.OutletID == outletID && s.ConfigVersion > sinceVersion {
			out = append(out, s)
		}
	}
	return out, nil
}

func (f *fakeRepository) SetInvoiceSeriesActive(ctx context.Context, tx pgx.Tx, id string, isActive bool, configVersion int) error {
	s, ok := f.invoiceSeries[id]
	if !ok {
		return httpx.ErrNotFound
	}
	s.IsActive = isActive
	s.ConfigVersion = configVersion
	f.invoiceSeries[id] = s
	return nil
}

func (f *fakeRepository) InsertDiscountDefinition(ctx context.Context, tx pgx.Tx, d contracts.DiscountDefinition) error {
	key := d.OutletID + "|" + d.Code
	if f.discountCodeTaken[key] {
		return httpx.ErrConflict
	}
	f.discountCodeTaken[key] = true
	f.discounts[d.ID] = d
	return nil
}

func (f *fakeRepository) GetDiscountDefinition(ctx context.Context, id string) (contracts.DiscountDefinition, error) {
	d, ok := f.discounts[id]
	if !ok {
		return contracts.DiscountDefinition{}, httpx.ErrNotFound
	}
	return d, nil
}

func (f *fakeRepository) DiscountDefinitionsSince(ctx context.Context, outletID string, sinceVersion int) ([]contracts.DiscountDefinition, error) {
	var out []contracts.DiscountDefinition
	for _, d := range f.discounts {
		if d.OutletID == outletID && d.ConfigVersion > sinceVersion {
			out = append(out, d)
		}
	}
	return out, nil
}

func (f *fakeRepository) SetDiscountDefinitionActive(ctx context.Context, tx pgx.Tx, id string, isActive bool, configVersion int) error {
	d, ok := f.discounts[id]
	if !ok {
		return httpx.ErrNotFound
	}
	d.IsActive = isActive
	d.ConfigVersion = configVersion
	f.discounts[id] = d
	return nil
}

func (f *fakeRepository) InsertFiscalProfile(ctx context.Context, tx pgx.Tx, fp contracts.OutletFiscalProfile) error {
	f.fiscalProfiles[fp.OutletID] = append(f.fiscalProfiles[fp.OutletID], fp)
	return nil
}

func (f *fakeRepository) CurrentFiscalProfile(ctx context.Context, outletID string) (*contracts.OutletFiscalProfile, error) {
	history := f.fiscalProfiles[outletID]
	if len(history) == 0 {
		return nil, nil
	}
	latest := history[0]
	for _, fp := range history[1:] {
		if fp.EffectiveFrom.After(latest.EffectiveFrom) {
			latest = fp
		}
	}
	return &latest, nil
}

func newTestService() (*Service, *fakeRepository) {
	repo := newFakeRepository()
	repo.outletTenant[testOutletID] = testTenantID
	return NewService(repo), repo
}

func ctxWithPermissions(perms ...auth.Permission) context.Context {
	return auth.WithPrincipal(context.Background(), auth.AuthenticatedPrincipal{
		UserID:      "principal-user",
		TenantID:    testTenantID,
		OutletID:    testOutletID,
		Permissions: perms,
	})
}

func authedCtx() context.Context {
	return ctxWithPermissions(auth.PermissionOutletManage)
}

// --- permission gating -----------------------------------------------------

func TestCreateComplianceVersion_RequiresOutletManagePermission(t *testing.T) {
	svc, _ := newTestService()
	_, err := svc.CreateComplianceVersion(context.Background(), testTenantID, NewComplianceVersionInput{
		OutletID: testOutletID, Label: "v1", EffectiveFrom: time.Now().UTC(),
	})
	if !errors.Is(err, httpx.ErrUnauthorized) {
		t.Fatalf("expected ErrUnauthorized for an unauthenticated caller, got %v", err)
	}

	_, err = svc.CreateComplianceVersion(ctxWithPermissions(auth.PermissionOrderCreate), testTenantID, NewComplianceVersionInput{
		OutletID: testOutletID, Label: "v1", EffectiveFrom: time.Now().UTC(),
	})
	if !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("expected ErrForbidden for a caller lacking outlet.manage, got %v", err)
	}
}

// --- config_version bump: the falsifiable property this task's gate cares
// about most --------------------------------------------------------------

func TestCreateComplianceVersion_BumpsOutletConfigVersion(t *testing.T) {
	svc, repo := newTestService()
	before := repo.outletVersions[testOutletID]

	cv, err := svc.CreateComplianceVersion(authedCtx(), testTenantID, NewComplianceVersionInput{
		OutletID: testOutletID, Label: "FY26", EffectiveFrom: time.Now().UTC(),
	})
	if err != nil {
		t.Fatalf("CreateComplianceVersion: %v", err)
	}
	after := repo.outletVersions[testOutletID]
	if after != before+1 {
		t.Fatalf("expected config_version to advance by exactly 1, before=%d after=%d", before, after)
	}
	if cv.ConfigVersion != after {
		t.Fatalf("expected the stored row's own config_version to equal the new outlet config_version, got %d want %d", cv.ConfigVersion, after)
	}
}

func TestCreateTaxProfile_WithRules_BumpsConfigVersionOnceForWholeBundle(t *testing.T) {
	svc, repo := newTestService()
	before := repo.outletVersions[testOutletID]

	tp, rules, err := svc.CreateTaxProfile(authedCtx(), testTenantID, NewTaxProfileInput{
		OutletID: testOutletID, Code: "GST5", Name: "GST 5%", PricingMode: contracts.PricingModeExclusive,
		IsDefault: true,
		Rules: []NewTaxRuleInput{
			{ComplianceVersionID: "cv-1", Component: contracts.TaxComponentCGST, RateBps: 250, EffectiveFrom: time.Now().UTC()},
			{ComplianceVersionID: "cv-1", Component: contracts.TaxComponentSGST, RateBps: 250, EffectiveFrom: time.Now().UTC()},
		},
	})
	if err != nil {
		t.Fatalf("CreateTaxProfile: %v", err)
	}
	after := repo.outletVersions[testOutletID]
	if after != before+1 {
		t.Fatalf("expected exactly one config_version bump for the profile+rules bundle, before=%d after=%d", before, after)
	}
	if len(rules) != 2 {
		t.Fatalf("expected 2 tax rules stored, got %d", len(rules))
	}
	for _, r := range rules {
		if r.ConfigVersion != tp.ConfigVersion {
			t.Fatalf("expected each tax_rule to share the profile's config_version, rule=%d profile=%d", r.ConfigVersion, tp.ConfigVersion)
		}
		if r.TaxProfileID != tp.ID {
			t.Fatalf("expected each tax_rule.tax_profile_id to point at the created profile")
		}
	}
}

func TestDeactivateTaxProfile_BumpsConfigVersion(t *testing.T) {
	svc, repo := newTestService()
	tp, _, err := svc.CreateTaxProfile(authedCtx(), testTenantID, NewTaxProfileInput{
		OutletID: testOutletID, Code: "GST5", Name: "GST 5%", PricingMode: contracts.PricingModeExclusive,
	})
	if err != nil {
		t.Fatalf("CreateTaxProfile: %v", err)
	}
	before := repo.outletVersions[testOutletID]

	deactivated, err := svc.DeactivateTaxProfile(authedCtx(), testTenantID, tp.ID)
	if err != nil {
		t.Fatalf("DeactivateTaxProfile: %v", err)
	}
	if deactivated.IsActive {
		t.Fatal("expected is_active false after deactivation")
	}
	after := repo.outletVersions[testOutletID]
	if after != before+1 {
		t.Fatalf("expected config_version to advance by exactly 1 on deactivation, before=%d after=%d", before, after)
	}
}

// --- uniqueness is tenant/outlet-scoped, never global -----------------------

func TestCreateTaxProfile_CodeUniquePerOutletNotGlobal(t *testing.T) {
	svc, repo := newTestService()
	otherOutletID := "33333333-3333-7333-8333-333333333333"
	repo.outletTenant[otherOutletID] = testTenantID

	if _, _, err := svc.CreateTaxProfile(authedCtx(), testTenantID, NewTaxProfileInput{
		OutletID: testOutletID, Code: "GST_5_RESTAURANT", Name: "GST 5%", PricingMode: contracts.PricingModeExclusive,
	}); err != nil {
		t.Fatalf("CreateTaxProfile at outlet 1: %v", err)
	}

	otherCtx := auth.WithPrincipal(context.Background(), auth.AuthenticatedPrincipal{
		UserID: "principal-user", TenantID: testTenantID, OutletID: otherOutletID,
		Permissions: []auth.Permission{auth.PermissionOutletManage},
	})
	if _, _, err := svc.CreateTaxProfile(otherCtx, testTenantID, NewTaxProfileInput{
		OutletID: otherOutletID, Code: "GST_5_RESTAURANT", Name: "GST 5%", PricingMode: contracts.PricingModeExclusive,
	}); err != nil {
		t.Fatalf("expected the SAME code to be creatable at a DIFFERENT outlet, got %v", err)
	}

	if _, _, err := svc.CreateTaxProfile(authedCtx(), testTenantID, NewTaxProfileInput{
		OutletID: testOutletID, Code: "GST_5_RESTAURANT", Name: "GST 5% dup", PricingMode: contracts.PricingModeExclusive,
	}); !errors.Is(err, httpx.ErrConflict) {
		t.Fatalf("expected a duplicate code AT THE SAME outlet to conflict, got %v", err)
	}
}

// --- discount_definition CHECK: half-populated PERCENT/AMOUNT rejected -----

func TestCreateDiscountDefinition_RejectsHalfPopulatedPercent(t *testing.T) {
	svc, _ := newTestService()
	valuePaise := 5000
	_, err := svc.CreateDiscountDefinition(authedCtx(), testTenantID, NewDiscountDefinitionInput{
		OutletID: testOutletID, Code: "FLAT50", Name: "Flat 50", Scope: contracts.DiscountScopeBill,
		Method: contracts.DiscountMethodPercent, ValuePaise: &valuePaise, // wrong field for PERCENT
		EffectiveFrom: time.Now().UTC(),
	})
	if !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput for PERCENT carrying value_paise, got %v", err)
	}
}

func TestCreateDiscountDefinition_RejectsHalfPopulatedAmount(t *testing.T) {
	svc, _ := newTestService()
	valueBps := 2000
	_, err := svc.CreateDiscountDefinition(authedCtx(), testTenantID, NewDiscountDefinitionInput{
		OutletID: testOutletID, Code: "TWENTY", Name: "20%", Scope: contracts.DiscountScopeBill,
		Method: contracts.DiscountMethodAmount, ValueBps: &valueBps, // wrong field for AMOUNT
		EffectiveFrom: time.Now().UTC(),
	})
	if !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput for AMOUNT carrying value_bps, got %v", err)
	}
}

func TestCreateDiscountDefinition_ValidPercentSucceedsAndBumpsConfigVersion(t *testing.T) {
	svc, repo := newTestService()
	before := repo.outletVersions[testOutletID]
	valueBps := 2000

	d, err := svc.CreateDiscountDefinition(authedCtx(), testTenantID, NewDiscountDefinitionInput{
		OutletID: testOutletID, Code: "TWENTY", Name: "20% off", Scope: contracts.DiscountScopeBill,
		Method: contracts.DiscountMethodPercent, ValueBps: &valueBps, EffectiveFrom: time.Now().UTC(),
	})
	if err != nil {
		t.Fatalf("CreateDiscountDefinition: %v", err)
	}
	if d.ValueBps == nil || *d.ValueBps != valueBps || d.ValuePaise != nil {
		t.Fatalf("expected exactly value_bps populated, got %+v", d)
	}
	if repo.outletVersions[testOutletID] != before+1 {
		t.Fatalf("expected config_version to advance by exactly 1")
	}
}

// --- invoice_series ----------------------------------------------------

func TestCreateInvoiceSeries_ValidatesResetPolicyAndPadding(t *testing.T) {
	svc, _ := newTestService()
	if _, err := svc.CreateInvoiceSeries(authedCtx(), testTenantID, NewInvoiceSeriesInput{
		OutletID: testOutletID, Code: "MAIN", PrefixTemplate: "FY{FY}/{OUTLET}/",
		ResetPolicy: "BOGUS", PaddingWidth: 6,
	}); !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput for an invalid reset_policy, got %v", err)
	}
	if _, err := svc.CreateInvoiceSeries(authedCtx(), testTenantID, NewInvoiceSeriesInput{
		OutletID: testOutletID, Code: "MAIN", PrefixTemplate: "FY{FY}/{OUTLET}/",
		ResetPolicy: contracts.SequenceResetFY, PaddingWidth: 0,
	}); !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput for padding_width 0, got %v", err)
	}
}

func TestDeactivateInvoiceSeries_BumpsConfigVersion(t *testing.T) {
	svc, repo := newTestService()
	series, err := svc.CreateInvoiceSeries(authedCtx(), testTenantID, NewInvoiceSeriesInput{
		OutletID: testOutletID, Code: "MAIN", PrefixTemplate: "FY{FY}/{OUTLET}/",
		ResetPolicy: contracts.SequenceResetFY, PaddingWidth: 6,
	})
	if err != nil {
		t.Fatalf("CreateInvoiceSeries: %v", err)
	}
	before := repo.outletVersions[testOutletID]
	deactivated, err := svc.DeactivateInvoiceSeries(authedCtx(), testTenantID, series.ID)
	if err != nil {
		t.Fatalf("DeactivateInvoiceSeries: %v", err)
	}
	if deactivated.IsActive {
		t.Fatal("expected is_active false")
	}
	if repo.outletVersions[testOutletID] != before+1 {
		t.Fatalf("expected config_version to advance by exactly 1")
	}
}

// --- outlet_fiscal_profile ---------------------------------------------------

func TestSetFiscalProfile_InsertsNewEffectiveDatedRowAndBumpsConfigVersion(t *testing.T) {
	svc, repo := newTestService()
	before := repo.outletVersions[testOutletID]

	fp, err := svc.SetFiscalProfile(authedCtx(), testTenantID, NewFiscalProfileInput{
		OutletID: testOutletID, LegalName: "Holler Foods Pvt Ltd", TradeName: "Holler",
		AddressLine1: "1 MG Road", City: "Pune", StateCode: "27", StateName: "Maharashtra",
		Pincode: "411001", GSTIN: "27ABCDE1234F1Z5", EffectiveFrom: time.Now().UTC(),
	})
	if err != nil {
		t.Fatalf("SetFiscalProfile: %v", err)
	}
	if repo.outletVersions[testOutletID] != before+1 {
		t.Fatalf("expected config_version to advance by exactly 1")
	}

	current, err := repo.CurrentFiscalProfile(context.Background(), testOutletID)
	if err != nil {
		t.Fatalf("CurrentFiscalProfile: %v", err)
	}
	if current == nil || current.ID != fp.ID {
		t.Fatalf("expected the newly set profile to be current, got %+v", current)
	}
}

// --- sync bundle: created config actually flows through the same read path
// the edge pulls from --------------------------------------------------

func TestSyncConfigBundle_ReturnsCreatedRowsAboveWatermark(t *testing.T) {
	svc, _ := newTestService()

	cv, err := svc.CreateComplianceVersion(authedCtx(), testTenantID, NewComplianceVersionInput{
		OutletID: testOutletID, Label: "FY26", EffectiveFrom: time.Now().UTC(),
	})
	if err != nil {
		t.Fatalf("CreateComplianceVersion: %v", err)
	}

	bundle, err := svc.SyncConfigBundle(authedCtx(), testTenantID, testOutletID, 0)
	if err != nil {
		t.Fatalf("SyncConfigBundle: %v", err)
	}
	found := false
	for _, v := range bundle.ComplianceVersions {
		if v.ID == cv.ID {
			found = true
		}
	}
	if !found {
		t.Fatalf("expected the created compliance_version to appear in a subsequent sync bundle pull, got %+v", bundle.ComplianceVersions)
	}

	// A pull with since_version at or above the row's own config_version
	// must exclude it (the watermark contract every config aggregate obeys).
	filtered, err := svc.SyncConfigBundle(authedCtx(), testTenantID, testOutletID, cv.ConfigVersion)
	if err != nil {
		t.Fatalf("SyncConfigBundle filtered: %v", err)
	}
	for _, v := range filtered.ComplianceVersions {
		if v.ID == cv.ID {
			t.Fatalf("expected the row to be excluded once since_version reaches its own config_version")
		}
	}
}

func TestSyncConfigBundle_FiscalProfileNeverFilteredBySinceVersion(t *testing.T) {
	svc, _ := newTestService()
	fp, err := svc.SetFiscalProfile(authedCtx(), testTenantID, NewFiscalProfileInput{
		OutletID: testOutletID, LegalName: "Holler Foods Pvt Ltd", TradeName: "Holler",
		AddressLine1: "1 MG Road", City: "Pune", StateCode: "27", StateName: "Maharashtra",
		Pincode: "411001", GSTIN: "27ABCDE1234F1Z5", EffectiveFrom: time.Now().UTC(),
	})
	if err != nil {
		t.Fatalf("SetFiscalProfile: %v", err)
	}

	bundle, err := svc.SyncConfigBundle(authedCtx(), testTenantID, testOutletID, fp.ConfigVersion)
	if err != nil {
		t.Fatalf("SyncConfigBundle: %v", err)
	}
	if bundle.FiscalProfile == nil || bundle.FiscalProfile.ID != fp.ID {
		t.Fatalf("expected the current fiscal profile regardless of since_version, got %+v", bundle.FiscalProfile)
	}
}
