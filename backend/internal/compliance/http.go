package compliance

import (
	"net/http"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

// Handler wires the compliance config write path onto a shared router. Every
// route here is permission-gated on the HUMAN-auth group (auth.Authenticate
// + PermissionOutletManage) — these are management decisions, not device
// replay, so they are never mounted under outlet.DeviceAuthenticate
// (CLAUDE.md's "config aggregates ... cloud owns them" rule, ADR-016).
type Handler struct {
	svc *Service
}

func NewHandler(svc *Service) *Handler {
	return &Handler{svc: svc}
}

// Mount registers every write route this task adds. There is no
// packages/contracts/openapi/openapi.yaml path for any of these today (only
// the response schemas the aggregates already ride in on GET /sync/config) —
// this task's report flags that gap for the orchestrator to close with an
// additive OpenAPI update; the routes below are shaped consistently with the
// sibling config routes that ARE documented (POST /stations, POST
// /outlets/{outletId}/tables).
func (h *Handler) Mount(r chi.Router) {
	r.With(auth.RequirePermission(auth.PermissionOutletManage)).Post("/outlets/{outletId}/compliance-versions", h.createComplianceVersion)

	r.With(auth.RequirePermission(auth.PermissionOutletManage)).Post("/outlets/{outletId}/tax-profiles", h.createTaxProfile)
	r.With(auth.RequirePermission(auth.PermissionOutletManage)).Post("/outlets/{outletId}/tax-profiles/{taxProfileId}/deactivate", h.deactivateTaxProfile)

	r.With(auth.RequirePermission(auth.PermissionOutletManage)).Post("/outlets/{outletId}/invoice-series", h.createInvoiceSeries)
	r.With(auth.RequirePermission(auth.PermissionOutletManage)).Post("/outlets/{outletId}/invoice-series/{seriesId}/deactivate", h.deactivateInvoiceSeries)

	r.With(auth.RequirePermission(auth.PermissionOutletManage)).Post("/outlets/{outletId}/discount-definitions", h.createDiscountDefinition)
	r.With(auth.RequirePermission(auth.PermissionOutletManage)).Post("/outlets/{outletId}/discount-definitions/{discountId}/deactivate", h.deactivateDiscountDefinition)

	r.With(auth.RequirePermission(auth.PermissionOutletManage)).Post("/outlets/{outletId}/fiscal-profile", h.setFiscalProfile)
}

func (h *Handler) principalTenant(r *http.Request) (string, bool) {
	p, ok := auth.PrincipalFromContext(r.Context())
	if !ok {
		return "", false
	}
	return p.TenantID, true
}

// --- compliance_version ------------------------------------------------

type createComplianceVersionRequest struct {
	Label         string    `json:"label"`
	EffectiveFrom time.Time `json:"effective_from"`
	Notes         *string   `json:"notes,omitempty"`
}

func (h *Handler) createComplianceVersion(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	outletID := chi.URLParam(r, "outletId")
	var req createComplianceVersionRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	cv, err := h.svc.CreateComplianceVersion(r.Context(), tenantID, NewComplianceVersionInput{
		OutletID: outletID, Label: req.Label, EffectiveFrom: req.EffectiveFrom, Notes: req.Notes,
	})
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusCreated, cv)
}

// --- tax_profile ---------------------------------------------------------

type createTaxRuleRequest struct {
	ComplianceVersionID string                 `json:"compliance_version_id"`
	Component           contracts.TaxComponent `json:"component"`
	RateBps             int                    `json:"rate_bps"`
	EffectiveFrom       time.Time              `json:"effective_from"`
	EffectiveTo         *time.Time             `json:"effective_to,omitempty"`
}

type createTaxProfileRequest struct {
	Code        string                 `json:"code"`
	Name        string                 `json:"name"`
	PricingMode contracts.PricingMode  `json:"pricing_mode"`
	IsDefault   bool                   `json:"is_default"`
	Rules       []createTaxRuleRequest `json:"rules,omitempty"`
}

type taxProfileBundleResponse struct {
	contracts.TaxProfile
	Rules []contracts.TaxRule `json:"rules"`
}

func (h *Handler) createTaxProfile(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	outletID := chi.URLParam(r, "outletId")
	var req createTaxProfileRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	rules := make([]NewTaxRuleInput, len(req.Rules))
	for i, ruleReq := range req.Rules {
		rules[i] = NewTaxRuleInput{
			ComplianceVersionID: ruleReq.ComplianceVersionID,
			Component:           ruleReq.Component,
			RateBps:             ruleReq.RateBps,
			EffectiveFrom:       ruleReq.EffectiveFrom,
			EffectiveTo:         ruleReq.EffectiveTo,
		}
	}
	tp, storedRules, err := h.svc.CreateTaxProfile(r.Context(), tenantID, NewTaxProfileInput{
		OutletID: outletID, Code: req.Code, Name: req.Name, PricingMode: req.PricingMode,
		IsDefault: req.IsDefault, Rules: rules,
	})
	if err != nil {
		httpx.Error(w, err)
		return
	}
	if storedRules == nil {
		storedRules = []contracts.TaxRule{}
	}
	httpx.JSON(w, http.StatusCreated, taxProfileBundleResponse{TaxProfile: tp, Rules: storedRules})
}

func (h *Handler) deactivateTaxProfile(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	profileID := chi.URLParam(r, "taxProfileId")
	tp, err := h.svc.DeactivateTaxProfile(r.Context(), tenantID, profileID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, tp)
}

// --- invoice_series ----------------------------------------------------

type createInvoiceSeriesRequest struct {
	Code           string                        `json:"code"`
	PrefixTemplate string                        `json:"prefix_template"`
	ResetPolicy    contracts.SequenceResetPolicy `json:"reset_policy"`
	PaddingWidth   int                           `json:"padding_width"`
}

func (h *Handler) createInvoiceSeries(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	outletID := chi.URLParam(r, "outletId")
	var req createInvoiceSeriesRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	series, err := h.svc.CreateInvoiceSeries(r.Context(), tenantID, NewInvoiceSeriesInput{
		OutletID: outletID, Code: req.Code, PrefixTemplate: req.PrefixTemplate,
		ResetPolicy: req.ResetPolicy, PaddingWidth: req.PaddingWidth,
	})
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusCreated, series)
}

func (h *Handler) deactivateInvoiceSeries(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	seriesID := chi.URLParam(r, "seriesId")
	series, err := h.svc.DeactivateInvoiceSeries(r.Context(), tenantID, seriesID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, series)
}

// --- discount_definition -------------------------------------------------

type createDiscountDefinitionRequest struct {
	Code               string                   `json:"code"`
	Name               string                   `json:"name"`
	Scope              contracts.DiscountScope  `json:"scope"`
	Method             contracts.DiscountMethod `json:"method"`
	ValueBps           *int                     `json:"value_bps,omitempty"`
	ValuePaise         *int                     `json:"value_paise,omitempty"`
	MaxDiscountPaise   *int                     `json:"max_discount_paise,omitempty"`
	RequiredPermission *string                  `json:"required_permission,omitempty"`
	RequiresReason     bool                     `json:"requires_reason"`
	EffectiveFrom      time.Time                `json:"effective_from"`
	EffectiveTo        *time.Time               `json:"effective_to,omitempty"`
}

func (h *Handler) createDiscountDefinition(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	outletID := chi.URLParam(r, "outletId")
	var req createDiscountDefinitionRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	d, err := h.svc.CreateDiscountDefinition(r.Context(), tenantID, NewDiscountDefinitionInput{
		OutletID: outletID, Code: req.Code, Name: req.Name, Scope: req.Scope, Method: req.Method,
		ValueBps: req.ValueBps, ValuePaise: req.ValuePaise, MaxDiscountPaise: req.MaxDiscountPaise,
		RequiredPermission: req.RequiredPermission, RequiresReason: req.RequiresReason,
		EffectiveFrom: req.EffectiveFrom, EffectiveTo: req.EffectiveTo,
	})
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusCreated, d)
}

func (h *Handler) deactivateDiscountDefinition(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	discountID := chi.URLParam(r, "discountId")
	d, err := h.svc.DeactivateDiscountDefinition(r.Context(), tenantID, discountID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, d)
}

// --- outlet_fiscal_profile ---------------------------------------------------

type setFiscalProfileRequest struct {
	LegalName         string    `json:"legal_name"`
	TradeName         string    `json:"trade_name"`
	AddressLine1      string    `json:"address_line1"`
	AddressLine2      *string   `json:"address_line2,omitempty"`
	City              string    `json:"city"`
	StateCode         string    `json:"state_code"`
	StateName         string    `json:"state_name"`
	Pincode           string    `json:"pincode"`
	GSTIN             string    `json:"gstin"`
	FSSAINumber       *string   `json:"fssai_number,omitempty"`
	InvoiceFooterText *string   `json:"invoice_footer_text,omitempty"`
	EffectiveFrom     time.Time `json:"effective_from"`
}

func (h *Handler) setFiscalProfile(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	outletID := chi.URLParam(r, "outletId")
	var req setFiscalProfileRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	fp, err := h.svc.SetFiscalProfile(r.Context(), tenantID, NewFiscalProfileInput{
		OutletID: outletID, LegalName: req.LegalName, TradeName: req.TradeName,
		AddressLine1: req.AddressLine1, AddressLine2: req.AddressLine2, City: req.City,
		StateCode: req.StateCode, StateName: req.StateName, Pincode: req.Pincode, GSTIN: req.GSTIN,
		FSSAINumber: req.FSSAINumber, InvoiceFooterText: req.InvoiceFooterText, EffectiveFrom: req.EffectiveFrom,
	})
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusCreated, fp)
}
