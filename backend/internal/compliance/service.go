package compliance

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"

	contracts "github.com/holler/contracts"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
)

// permConfigManage gates every write in this file. No purpose-built
// "billing.manage"/"compliance.manage" permission exists in the frozen
// contracts.Permission enum (packages/contracts/go/identity.go); outlet.manage
// is the closest existing permission for "may configure fiscal identity,
// tax rules, invoice numbering and discounts at this outlet" — the same
// judgment backend/internal/kitchen already made for POST /printers. Noted
// in this task's final report as a candidate for a dedicated permission in a
// future contracts bump.
const permConfigManage = auth.PermissionOutletManage

// Service holds the T13 config write path's business logic: create/
// deactivate for compliance_version, tax_profile (+ its child tax_rule
// rows), invoice_series, discount_definition, and set/replace for
// outlet_fiscal_profile. Every write bumps outlet.config_version exactly
// once (ADR-016) — the mechanism the whole cloud→edge sync depends on.
type Service struct {
	repo Repository
	now  func() time.Time
}

func NewService(repo Repository) *Service {
	return &Service{repo: repo, now: time.Now}
}

func requirePermission(ctx context.Context) (auth.AuthenticatedPrincipal, error) {
	principal, ok := auth.PrincipalFromContext(ctx)
	if !ok {
		return auth.AuthenticatedPrincipal{}, httpx.ErrUnauthorized
	}
	for _, p := range principal.Permissions {
		if p == permConfigManage {
			return principal, nil
		}
	}
	return auth.AuthenticatedPrincipal{}, httpx.ErrForbidden
}

func (s *Service) requireOutletInTenant(ctx context.Context, tenantID, outletID string) error {
	if strings.TrimSpace(tenantID) == "" {
		return httpx.ErrUnauthorized
	}
	outletID = strings.TrimSpace(outletID)
	if outletID == "" {
		return fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	ok, err := s.repo.OutletBelongsToTenant(ctx, tenantID, outletID)
	if err != nil {
		return err
	}
	if !ok {
		return httpx.ErrForbidden
	}
	return nil
}

// --- compliance_version ------------------------------------------------

// NewComplianceVersionInput is what a caller supplies to define a new
// versioned ruleset an invoice can later pin itself to (§31).
type NewComplianceVersionInput struct {
	OutletID      string
	Label         string
	EffectiveFrom time.Time
	Notes         *string
}

func (s *Service) CreateComplianceVersion(ctx context.Context, tenantID string, in NewComplianceVersionInput) (contracts.ComplianceVersion, error) {
	if _, err := requirePermission(ctx); err != nil {
		return contracts.ComplianceVersion{}, err
	}
	if err := s.requireOutletInTenant(ctx, tenantID, in.OutletID); err != nil {
		return contracts.ComplianceVersion{}, err
	}
	if strings.TrimSpace(in.Label) == "" {
		return contracts.ComplianceVersion{}, fmt.Errorf("%w: label is required", httpx.ErrInvalidInput)
	}
	if in.EffectiveFrom.IsZero() {
		return contracts.ComplianceVersion{}, fmt.Errorf("%w: effective_from is required", httpx.ErrInvalidInput)
	}

	cv := contracts.ComplianceVersion{
		ID:            id.New(),
		OutletID:      in.OutletID,
		Label:         in.Label,
		EffectiveFrom: in.EffectiveFrom.UTC(),
		Notes:         in.Notes,
		SchemaVersion: 1,
	}

	err := s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, in.OutletID)
		if err != nil {
			return err
		}
		cv.ConfigVersion = newVersion
		return s.repo.InsertComplianceVersion(ctx, tx, cv)
	})
	if err != nil {
		return contracts.ComplianceVersion{}, err
	}
	return cv, nil
}

// --- tax_profile (+ child tax_rule rows) ----------------------------------

// NewTaxRuleInput is one component rate inside a NewTaxProfileInput's
// bundle. tax_rule is never independently addressable (§Task 2 note): it
// only ever arrives as part of creating the profile it belongs to.
type NewTaxRuleInput struct {
	ComplianceVersionID string
	Component           contracts.TaxComponent
	RateBps             int
	EffectiveFrom       time.Time
	EffectiveTo         *time.Time
}

// NewTaxProfileInput is what a caller supplies to define a new tax profile,
// with its initial set of component rates in the same bundle.
type NewTaxProfileInput struct {
	OutletID    string
	Code        string
	Name        string
	PricingMode contracts.PricingMode
	IsDefault   bool
	Rules       []NewTaxRuleInput
}

// CreateTaxProfile inserts a tax_profile and its tax_rule children in one
// transaction, bumping outlet.config_version exactly once for the whole
// bundle.
func (s *Service) CreateTaxProfile(ctx context.Context, tenantID string, in NewTaxProfileInput) (contracts.TaxProfile, []contracts.TaxRule, error) {
	if _, err := requirePermission(ctx); err != nil {
		return contracts.TaxProfile{}, nil, err
	}
	if err := s.requireOutletInTenant(ctx, tenantID, in.OutletID); err != nil {
		return contracts.TaxProfile{}, nil, err
	}
	if err := validateNewTaxProfile(in); err != nil {
		return contracts.TaxProfile{}, nil, err
	}

	tp := contracts.TaxProfile{
		ID:            id.New(),
		OutletID:      in.OutletID,
		Code:          in.Code,
		Name:          in.Name,
		PricingMode:   in.PricingMode,
		IsDefault:     in.IsDefault,
		IsActive:      true,
		SchemaVersion: 1,
	}
	rules := make([]contracts.TaxRule, 0, len(in.Rules))

	err := s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, in.OutletID)
		if err != nil {
			return err
		}
		tp.ConfigVersion = newVersion
		if err := s.repo.InsertTaxProfile(ctx, tx, tp); err != nil {
			return err
		}
		for _, ruleIn := range in.Rules {
			tr := contracts.TaxRule{
				ID:                  id.New(),
				TaxProfileID:        tp.ID,
				ComplianceVersionID: ruleIn.ComplianceVersionID,
				Component:           ruleIn.Component,
				RateBps:             ruleIn.RateBps,
				EffectiveFrom:       ruleIn.EffectiveFrom.UTC(),
				EffectiveTo:         ruleIn.EffectiveTo,
				ConfigVersion:       newVersion,
				SchemaVersion:       1,
			}
			if err := s.repo.InsertTaxRule(ctx, tx, tr); err != nil {
				return err
			}
			rules = append(rules, tr)
		}
		return nil
	})
	if err != nil {
		return contracts.TaxProfile{}, nil, err
	}
	return tp, rules, nil
}

func validateNewTaxProfile(in NewTaxProfileInput) error {
	if strings.TrimSpace(in.Code) == "" {
		return fmt.Errorf("%w: code is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.Name) == "" {
		return fmt.Errorf("%w: name is required", httpx.ErrInvalidInput)
	}
	if in.PricingMode != contracts.PricingModeInclusive && in.PricingMode != contracts.PricingModeExclusive {
		return fmt.Errorf("%w: pricing_mode must be INCLUSIVE or EXCLUSIVE", httpx.ErrInvalidInput)
	}
	for _, r := range in.Rules {
		if err := validateNewTaxRule(r); err != nil {
			return err
		}
	}
	return nil
}

func validateNewTaxRule(in NewTaxRuleInput) error {
	if strings.TrimSpace(in.ComplianceVersionID) == "" {
		return fmt.Errorf("%w: tax rule compliance_version_id is required", httpx.ErrInvalidInput)
	}
	switch in.Component {
	case contracts.TaxComponentCGST, contracts.TaxComponentSGST, contracts.TaxComponentIGST, contracts.TaxComponentCess:
	default:
		return fmt.Errorf("%w: tax rule component %q is not valid", httpx.ErrInvalidInput, in.Component)
	}
	if in.RateBps < 0 || in.RateBps > 10000 {
		return fmt.Errorf("%w: tax rule rate_bps must be between 0 and 10000", httpx.ErrInvalidInput)
	}
	if in.EffectiveFrom.IsZero() {
		return fmt.Errorf("%w: tax rule effective_from is required", httpx.ErrInvalidInput)
	}
	return nil
}

// DeactivateTaxProfile flips is_active to false. It does not delete the row
// (a profile pinned into a historical invoice's tax_snapshot must remain
// resolvable, §31) and it bumps outlet.config_version so the edge learns of
// the change.
func (s *Service) DeactivateTaxProfile(ctx context.Context, tenantID, profileID string) (contracts.TaxProfile, error) {
	if _, err := requirePermission(ctx); err != nil {
		return contracts.TaxProfile{}, err
	}
	current, err := s.repo.GetTaxProfile(ctx, profileID)
	if err != nil {
		return contracts.TaxProfile{}, err
	}
	if err := s.requireOutletInTenant(ctx, tenantID, current.OutletID); err != nil {
		return contracts.TaxProfile{}, err
	}

	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, current.OutletID)
		if err != nil {
			return err
		}
		current.ConfigVersion = newVersion
		return s.repo.SetTaxProfileActive(ctx, tx, profileID, false, newVersion)
	})
	if err != nil {
		return contracts.TaxProfile{}, err
	}
	current.IsActive = false
	return current, nil
}

// --- invoice_series ----------------------------------------------------

// NewInvoiceSeriesInput is what a caller supplies to define a numbering
// series. The counter that issues the next number stays edge-local (§33) —
// this only defines the series itself.
type NewInvoiceSeriesInput struct {
	OutletID       string
	Code           string
	PrefixTemplate string
	ResetPolicy    contracts.SequenceResetPolicy
	PaddingWidth   int
}

func (s *Service) CreateInvoiceSeries(ctx context.Context, tenantID string, in NewInvoiceSeriesInput) (contracts.InvoiceSeries, error) {
	if _, err := requirePermission(ctx); err != nil {
		return contracts.InvoiceSeries{}, err
	}
	if err := s.requireOutletInTenant(ctx, tenantID, in.OutletID); err != nil {
		return contracts.InvoiceSeries{}, err
	}
	if err := validateNewInvoiceSeries(in); err != nil {
		return contracts.InvoiceSeries{}, err
	}

	series := contracts.InvoiceSeries{
		ID:             id.New(),
		OutletID:       in.OutletID,
		Code:           in.Code,
		PrefixTemplate: in.PrefixTemplate,
		ResetPolicy:    in.ResetPolicy,
		PaddingWidth:   in.PaddingWidth,
		IsActive:       true,
		SchemaVersion:  1,
	}

	err := s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, in.OutletID)
		if err != nil {
			return err
		}
		series.ConfigVersion = newVersion
		return s.repo.InsertInvoiceSeries(ctx, tx, series)
	})
	if err != nil {
		return contracts.InvoiceSeries{}, err
	}
	return series, nil
}

func validateNewInvoiceSeries(in NewInvoiceSeriesInput) error {
	if strings.TrimSpace(in.Code) == "" {
		return fmt.Errorf("%w: code is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.PrefixTemplate) == "" {
		return fmt.Errorf("%w: prefix_template is required", httpx.ErrInvalidInput)
	}
	switch in.ResetPolicy {
	case contracts.SequenceResetNever, contracts.SequenceResetFY, contracts.SequenceResetMonth, contracts.SequenceResetDay:
	default:
		return fmt.Errorf("%w: reset_policy %q is not valid", httpx.ErrInvalidInput, in.ResetPolicy)
	}
	if in.PaddingWidth < 1 || in.PaddingWidth > 12 {
		return fmt.Errorf("%w: padding_width must be between 1 and 12", httpx.ErrInvalidInput)
	}
	return nil
}

func (s *Service) DeactivateInvoiceSeries(ctx context.Context, tenantID, seriesID string) (contracts.InvoiceSeries, error) {
	if _, err := requirePermission(ctx); err != nil {
		return contracts.InvoiceSeries{}, err
	}
	current, err := s.repo.GetInvoiceSeries(ctx, seriesID)
	if err != nil {
		return contracts.InvoiceSeries{}, err
	}
	if err := s.requireOutletInTenant(ctx, tenantID, current.OutletID); err != nil {
		return contracts.InvoiceSeries{}, err
	}

	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, current.OutletID)
		if err != nil {
			return err
		}
		current.ConfigVersion = newVersion
		return s.repo.SetInvoiceSeriesActive(ctx, tx, seriesID, false, newVersion)
	})
	if err != nil {
		return contracts.InvoiceSeries{}, err
	}
	current.IsActive = false
	return current, nil
}

// --- discount_definition -------------------------------------------------

// NewDiscountDefinitionInput is what a caller supplies to define a
// discount a cashier may apply. Exactly one of ValueBps/ValuePaise must be
// set, decided by Method — enforced here with a clear 400 rather than
// letting the CHECK constraint surface as a raw driver error.
type NewDiscountDefinitionInput struct {
	OutletID           string
	Code               string
	Name               string
	Scope              contracts.DiscountScope
	Method             contracts.DiscountMethod
	ValueBps           *int
	ValuePaise         *int
	MaxDiscountPaise   *int
	RequiredPermission *string
	RequiresReason     bool
	EffectiveFrom      time.Time
	EffectiveTo        *time.Time
}

func (s *Service) CreateDiscountDefinition(ctx context.Context, tenantID string, in NewDiscountDefinitionInput) (contracts.DiscountDefinition, error) {
	if _, err := requirePermission(ctx); err != nil {
		return contracts.DiscountDefinition{}, err
	}
	if err := s.requireOutletInTenant(ctx, tenantID, in.OutletID); err != nil {
		return contracts.DiscountDefinition{}, err
	}
	if err := validateNewDiscountDefinition(in); err != nil {
		return contracts.DiscountDefinition{}, err
	}

	d := contracts.DiscountDefinition{
		ID:                 id.New(),
		OutletID:           in.OutletID,
		Code:               in.Code,
		Name:               in.Name,
		Scope:              in.Scope,
		Method:             in.Method,
		ValueBps:           in.ValueBps,
		ValuePaise:         in.ValuePaise,
		MaxDiscountPaise:   in.MaxDiscountPaise,
		RequiredPermission: in.RequiredPermission,
		RequiresReason:     in.RequiresReason,
		IsActive:           true,
		EffectiveFrom:      in.EffectiveFrom.UTC(),
		EffectiveTo:        in.EffectiveTo,
		SchemaVersion:      1,
	}

	err := s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, in.OutletID)
		if err != nil {
			return err
		}
		d.ConfigVersion = newVersion
		return s.repo.InsertDiscountDefinition(ctx, tx, d)
	})
	if err != nil {
		return contracts.DiscountDefinition{}, err
	}
	return d, nil
}

// validateNewDiscountDefinition rejects the half-populated PERCENT/AMOUNT
// shape at the service boundary — "20% or ₹50?" has no defined answer — with
// a clear httpx.ErrInvalidInput rather than letting
// packages/contracts/postgres/0007_m3_billing.sql's CHECK constraint surface
// as an opaque driver error.
func validateNewDiscountDefinition(in NewDiscountDefinitionInput) error {
	if strings.TrimSpace(in.Code) == "" {
		return fmt.Errorf("%w: code is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.Name) == "" {
		return fmt.Errorf("%w: name is required", httpx.ErrInvalidInput)
	}
	if in.Scope != contracts.DiscountScopeLine && in.Scope != contracts.DiscountScopeBill {
		return fmt.Errorf("%w: scope must be LINE or BILL", httpx.ErrInvalidInput)
	}
	if in.EffectiveFrom.IsZero() {
		return fmt.Errorf("%w: effective_from is required", httpx.ErrInvalidInput)
	}
	switch in.Method {
	case contracts.DiscountMethodPercent:
		if in.ValueBps == nil || in.ValuePaise != nil {
			return fmt.Errorf("%w: method PERCENT requires value_bps and forbids value_paise", httpx.ErrInvalidInput)
		}
		if *in.ValueBps < 0 || *in.ValueBps > 10000 {
			return fmt.Errorf("%w: value_bps must be between 0 and 10000", httpx.ErrInvalidInput)
		}
	case contracts.DiscountMethodAmount:
		if in.ValuePaise == nil || in.ValueBps != nil {
			return fmt.Errorf("%w: method AMOUNT requires value_paise and forbids value_bps", httpx.ErrInvalidInput)
		}
		if *in.ValuePaise < 0 {
			return fmt.Errorf("%w: value_paise must not be negative", httpx.ErrInvalidInput)
		}
	default:
		return fmt.Errorf("%w: method must be PERCENT or AMOUNT", httpx.ErrInvalidInput)
	}
	return nil
}

func (s *Service) DeactivateDiscountDefinition(ctx context.Context, tenantID, discountID string) (contracts.DiscountDefinition, error) {
	if _, err := requirePermission(ctx); err != nil {
		return contracts.DiscountDefinition{}, err
	}
	current, err := s.repo.GetDiscountDefinition(ctx, discountID)
	if err != nil {
		return contracts.DiscountDefinition{}, err
	}
	if err := s.requireOutletInTenant(ctx, tenantID, current.OutletID); err != nil {
		return contracts.DiscountDefinition{}, err
	}

	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, current.OutletID)
		if err != nil {
			return err
		}
		current.ConfigVersion = newVersion
		return s.repo.SetDiscountDefinitionActive(ctx, tx, discountID, false, newVersion)
	})
	if err != nil {
		return contracts.DiscountDefinition{}, err
	}
	current.IsActive = false
	return current, nil
}

// --- outlet_fiscal_profile -------------------------------------------------

// NewFiscalProfileInput is what a caller supplies to set the outlet's
// seller identity. A change (GSTIN, trade name, ...) always inserts a NEW
// effective-dated row rather than mutating the current one — a reprinted
// historical invoice must carry the identity that was current when it was
// issued (§33).
type NewFiscalProfileInput struct {
	OutletID          string
	LegalName         string
	TradeName         string
	AddressLine1      string
	AddressLine2      *string
	City              string
	StateCode         string
	StateName         string
	Pincode           string
	GSTIN             string
	FSSAINumber       *string
	InvoiceFooterText *string
	EffectiveFrom     time.Time
}

func (s *Service) SetFiscalProfile(ctx context.Context, tenantID string, in NewFiscalProfileInput) (contracts.OutletFiscalProfile, error) {
	if _, err := requirePermission(ctx); err != nil {
		return contracts.OutletFiscalProfile{}, err
	}
	if err := s.requireOutletInTenant(ctx, tenantID, in.OutletID); err != nil {
		return contracts.OutletFiscalProfile{}, err
	}
	if err := validateNewFiscalProfile(in); err != nil {
		return contracts.OutletFiscalProfile{}, err
	}

	fp := contracts.OutletFiscalProfile{
		ID:                id.New(),
		OutletID:          in.OutletID,
		LegalName:         in.LegalName,
		TradeName:         in.TradeName,
		AddressLine1:      in.AddressLine1,
		AddressLine2:      in.AddressLine2,
		City:              in.City,
		StateCode:         in.StateCode,
		StateName:         in.StateName,
		Pincode:           in.Pincode,
		GSTIN:             in.GSTIN,
		FSSAINumber:       in.FSSAINumber,
		InvoiceFooterText: in.InvoiceFooterText,
		EffectiveFrom:     in.EffectiveFrom.UTC(),
		SchemaVersion:     1,
	}

	err := s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, in.OutletID)
		if err != nil {
			return err
		}
		fp.ConfigVersion = newVersion
		return s.repo.InsertFiscalProfile(ctx, tx, fp)
	})
	if err != nil {
		return contracts.OutletFiscalProfile{}, err
	}
	return fp, nil
}

func validateNewFiscalProfile(in NewFiscalProfileInput) error {
	if strings.TrimSpace(in.LegalName) == "" {
		return fmt.Errorf("%w: legal_name is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.TradeName) == "" {
		return fmt.Errorf("%w: trade_name is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.AddressLine1) == "" {
		return fmt.Errorf("%w: address_line1 is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.City) == "" {
		return fmt.Errorf("%w: city is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.StateCode) == "" {
		return fmt.Errorf("%w: state_code is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.StateName) == "" {
		return fmt.Errorf("%w: state_name is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.Pincode) == "" {
		return fmt.Errorf("%w: pincode is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.GSTIN) == "" {
		return fmt.Errorf("%w: gstin is required", httpx.ErrInvalidInput)
	}
	if in.EffectiveFrom.IsZero() {
		return fmt.Errorf("%w: effective_from is required", httpx.ErrInvalidInput)
	}
	return nil
}

// --- GET /sync/config bundle -----------------------------------------------

// ConfigBundle is this context's contribution to GET /sync/config
// (contracts 0.4.0, ADR-016), mirroring backend/internal/kitchen.ConfigBundle
// exactly: everything newer than the caller's since_version, plus the
// outlet's single current fiscal profile (nullable, never filtered by
// since_version — the edge always needs to know what applies NOW).
type ConfigBundle struct {
	ComplianceVersions  []contracts.ComplianceVersion
	TaxProfiles         []contracts.TaxProfile
	TaxRules            []contracts.TaxRule
	InvoiceSeries       []contracts.InvoiceSeries
	DiscountDefinitions []contracts.DiscountDefinition
	FiscalProfile       *contracts.OutletFiscalProfile
}

func (s *Service) SyncConfigBundle(ctx context.Context, tenantID, outletID string, sinceVersion int) (ConfigBundle, error) {
	if err := s.requireOutletInTenant(ctx, tenantID, outletID); err != nil {
		return ConfigBundle{}, err
	}

	versions, err := s.repo.ComplianceVersionsSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	profiles, err := s.repo.TaxProfilesSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	rules, err := s.repo.TaxRulesSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	series, err := s.repo.InvoiceSeriesSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	discounts, err := s.repo.DiscountDefinitionsSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	fiscalProfile, err := s.repo.CurrentFiscalProfile(ctx, outletID)
	if err != nil {
		return ConfigBundle{}, err
	}

	return ConfigBundle{
		ComplianceVersions:  emptyIfNilVersions(versions),
		TaxProfiles:         emptyIfNilProfiles(profiles),
		TaxRules:            emptyIfNilRules(rules),
		InvoiceSeries:       emptyIfNilSeries(series),
		DiscountDefinitions: emptyIfNilDiscounts(discounts),
		FiscalProfile:       fiscalProfile,
	}, nil
}

func emptyIfNilVersions(v []contracts.ComplianceVersion) []contracts.ComplianceVersion {
	if v == nil {
		return []contracts.ComplianceVersion{}
	}
	return v
}

func emptyIfNilProfiles(v []contracts.TaxProfile) []contracts.TaxProfile {
	if v == nil {
		return []contracts.TaxProfile{}
	}
	return v
}

func emptyIfNilRules(v []contracts.TaxRule) []contracts.TaxRule {
	if v == nil {
		return []contracts.TaxRule{}
	}
	return v
}

func emptyIfNilSeries(v []contracts.InvoiceSeries) []contracts.InvoiceSeries {
	if v == nil {
		return []contracts.InvoiceSeries{}
	}
	return v
}

func emptyIfNilDiscounts(v []contracts.DiscountDefinition) []contracts.DiscountDefinition {
	if v == nil {
		return []contracts.DiscountDefinition{}
	}
	return v
}
