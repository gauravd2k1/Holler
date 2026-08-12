// Tax engine, fiscal identity and discount config — added at 0.4.0 (ADR-016,
// Milestone 3). Mirrors src/types/tax.ts.
//
// CONFIG aggregates under §50.1: cloud-owned, synced cloud→edge versioned by
// ConfigVersion, replaced wholesale at the edge. The Invoice that *uses* these
// rules is edge-authoritative — the same config/operational split ADR-011 drew
// between RestaurantTable and TableSession, and ADR-014 between Station and Kot.
//
// §31: "Do NOT scatter tax percentages throughout the application." Every rate
// in the product resolves through a TaxProfile.
//
// Field names match sqlite/0006_m3_billing.sql and postgres/0007_m3_billing.sql
// exactly.
package contracts

import "time"

type TaxComponent string

const (
	TaxComponentCGST TaxComponent = "CGST"
	TaxComponentSGST TaxComponent = "SGST"
	TaxComponentIGST TaxComponent = "IGST"
	TaxComponentCess TaxComponent = "CESS"
)

type PricingMode string

const (
	PricingModeInclusive PricingMode = "INCLUSIVE"
	PricingModeExclusive PricingMode = "EXCLUSIVE"
)

// ComplianceVersion is the versioned ruleset an invoice pins itself to. §31
// requires historical bills stay reproducible after rules change, which is only
// possible if the bill records which ruleset produced it.
type ComplianceVersion struct {
	ID            string    `json:"id"`
	OutletID      string    `json:"outlet_id"`
	Label         string    `json:"label"`
	EffectiveFrom time.Time `json:"effective_from"`
	Notes         *string   `json:"notes"`
	ConfigVersion int       `json:"config_version"`
	SchemaVersion int       `json:"schema_version"`
}

// TaxRule is a component rate inside a profile, effective-dated. A child row
// travelling in its parent's config bundle — the MenuItemVariant precedent, not
// an aggregate of its own.
//
// RateBps is integer basis points, never a float: 2.5% = 250. CLAUDE.md forbids
// floating point for money, and a rate that multiplies money inherits the rule.
type TaxRule struct {
	ID                  string       `json:"id"`
	TaxProfileID        string       `json:"tax_profile_id"`
	ComplianceVersionID string       `json:"compliance_version_id"`
	Component           TaxComponent `json:"component"`
	RateBps             int          `json:"rate_bps"`
	EffectiveFrom       time.Time    `json:"effective_from"`
	EffectiveTo         *time.Time   `json:"effective_to"`
	ConfigVersion       int          `json:"config_version"`
	SchemaVersion       int          `json:"schema_version"`
}

type TaxProfile struct {
	ID       string `json:"id"`
	OutletID string `json:"outlet_id"`
	// Stable machine code (GST_5_RESTAURANT), unique per outlet, never global.
	Code string `json:"code"`
	Name string `json:"name"`
	// Belongs to the profile, not the rule: a profile is inclusive or exclusive
	// as a whole, and mixing the two across one profile's components has no
	// coherent meaning.
	PricingMode   PricingMode `json:"pricing_mode"`
	IsDefault     bool        `json:"is_default"`
	IsActive      bool        `json:"is_active"`
	ConfigVersion int         `json:"config_version"`
	SchemaVersion int         `json:"schema_version"`
}

// OutletFiscalProfile is the seller identity printed on a GST invoice (§33).
// Effective-dated because a GSTIN or trade name can change and a reprinted
// historical invoice must carry the identity current when it was issued.
type OutletFiscalProfile struct {
	ID                string    `json:"id"`
	OutletID          string    `json:"outlet_id"`
	LegalName         string    `json:"legal_name"`
	TradeName         string    `json:"trade_name"`
	AddressLine1      string    `json:"address_line1"`
	AddressLine2      *string   `json:"address_line2"`
	City              string    `json:"city"`
	StateCode         string    `json:"state_code"` // GST state code: '27' = Maharashtra
	StateName         string    `json:"state_name"`
	Pincode           string    `json:"pincode"`
	GSTIN             string    `json:"gstin"`
	FSSAINumber       *string   `json:"fssai_number"`
	InvoiceFooterText *string   `json:"invoice_footer_text"`
	EffectiveFrom     time.Time `json:"effective_from"`
	ConfigVersion     int       `json:"config_version"`
	SchemaVersion     int       `json:"schema_version"`
}

type DiscountScope string

const (
	DiscountScopeLine DiscountScope = "LINE"
	DiscountScopeBill DiscountScope = "BILL"
)

type DiscountMethod string

const (
	DiscountMethodPercent DiscountMethod = "PERCENT"
	DiscountMethodAmount  DiscountMethod = "AMOUNT"
)

// DiscountDefinition is a discount a cashier may apply. An ad-hoc discount is
// still governed by one of these rows: the row carries the permission and
// reason requirements (§28 bill.discount / bill.discount.override).
//
// Exactly one of ValueBps and ValuePaise is set, decided by Method. A CHECK in
// both stores and a refine in the Zod schema make the half-populated state
// unrepresentable — "20% or ₹50?" has no defined answer in a tax engine.
type DiscountDefinition struct {
	ID                 string         `json:"id"`
	OutletID           string         `json:"outlet_id"`
	Code               string         `json:"code"`
	Name               string         `json:"name"`
	Scope              DiscountScope  `json:"scope"`
	Method             DiscountMethod `json:"method"`
	ValueBps           *int           `json:"value_bps"`
	ValuePaise         *int           `json:"value_paise"`
	MaxDiscountPaise   *int           `json:"max_discount_paise"`
	RequiredPermission *string        `json:"required_permission"`
	RequiresReason     bool           `json:"requires_reason"`
	IsActive           bool           `json:"is_active"`
	EffectiveFrom      time.Time      `json:"effective_from"`
	EffectiveTo        *time.Time     `json:"effective_to"`
	ConfigVersion      int            `json:"config_version"`
	SchemaVersion      int            `json:"schema_version"`
}
