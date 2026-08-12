// GST invoice and invoice numbering — added at 0.4.0 (ADR-016, Milestone 3).
// Mirrors src/types/invoice.ts.
//
// Invoice is EDGE-AUTHORITATIVE (§50.1): the outlet issues bills with the
// uplink down, and the cloud only ever REPLAYS them. No handler in this
// backend mints an invoice number or transitions an invoice — the rule ADR-014
// set for kot.status, applied to money.
//
// InvoiceSeries is CONFIG (cloud→edge): the series *definition*. The counter
// that produces the next number is edge-local (sqlite `invoice_sequence`),
// deliberately has no Go struct and no Postgres table, and never syncs.
// Mirroring it would make the cloud a second writer of invoice numbers, which
// §33's "never generate duplicate invoice numbers" forbids.
package contracts

import "time"

type InvoiceStatus string

const (
	InvoiceStatusIssued    InvoiceStatus = "ISSUED"
	InvoiceStatusCancelled InvoiceStatus = "CANCELLED"
)

type SequenceResetPolicy string

const (
	SequenceResetNever SequenceResetPolicy = "NEVER"
	SequenceResetFY    SequenceResetPolicy = "FY"
	SequenceResetMonth SequenceResetPolicy = "MONTH"
	SequenceResetDay   SequenceResetPolicy = "DAY"
)

type InvoiceSeries struct {
	ID       string `json:"id"`
	OutletID string `json:"outlet_id"`
	Code     string `json:"code"`
	// Tokens: {FY} {YYYY} {MM} {DD} {OUTLET}. 'FY{FY}/{OUTLET}/' with
	// PaddingWidth 6 renders FY26/PNQ/001423.
	PrefixTemplate string              `json:"prefix_template"`
	ResetPolicy    SequenceResetPolicy `json:"reset_policy"`
	PaddingWidth   int                 `json:"padding_width"`
	IsActive       bool                `json:"is_active"`
	ConfigVersion  int                 `json:"config_version"`
	SchemaVersion  int                 `json:"schema_version"`
}

// TaxLiabilityParty records who bears the GST liability (§32). Captured at
// issue time because direct and ECO supplies must never be combined in
// compliance reporting, and that is only possible if the classification was
// recorded when the bill was raised. Milestone 3 EXCLUDES the reporting
// outputs, not these fields.
type TaxLiabilityParty string

const (
	TaxLiabilityRestaurant TaxLiabilityParty = "RESTAURANT"
	TaxLiabilityECO        TaxLiabilityParty = "ECO"
)

type InvoiceLine struct {
	ID        string `json:"id"`
	InvoiceID string `json:"invoice_id"`
	// The order line this bills. This is what makes the split-bill conservation
	// property checkable: across a split group every order line must appear
	// exactly once in total quantity — no loss, no duplication, no double-tax.
	OrderItemID string `json:"order_item_id"`
	LineNo      int    `json:"line_no"`
	// Snapshot at issue time — never re-read from the live menu.
	Description       string  `json:"description"`
	HSNSAC            *string `json:"hsn_sac"`
	Quantity          int     `json:"quantity"`
	UnitPricePaise    int     `json:"unit_price_paise"`
	GrossPaise        int     `json:"gross_paise"`
	DiscountPaise     int     `json:"discount_paise"`
	TaxableValuePaise int     `json:"taxable_value_paise"`
	TaxProfileID      string  `json:"tax_profile_id"`
	CGSTRateBps       int     `json:"cgst_rate_bps"`
	CGSTPaise         int     `json:"cgst_paise"`
	SGSTRateBps       int     `json:"sgst_rate_bps"`
	SGSTPaise         int     `json:"sgst_paise"`
	IGSTRateBps       int     `json:"igst_rate_bps"`
	IGSTPaise         int     `json:"igst_paise"`
	CessRateBps       int     `json:"cess_rate_bps"`
	CessPaise         int     `json:"cess_paise"`
	TotalPaise        int     `json:"total_paise"`
	SchemaVersion     int     `json:"schema_version"`
}

// Invoice is a GST invoice (§33). Split bills are N invoices over one order
// sharing a SplitGroupID — each part is a real, independently numbered,
// independently payable invoice, because that is what the customer physically
// receives. There is deliberately no BillSplit entity.
type Invoice struct {
	ID       string `json:"id"`
	OutletID string `json:"outlet_id"`
	OrderID  string `json:"order_id"`

	SplitGroupID *string `json:"split_group_id"`
	SplitIndex   int     `json:"split_index"`
	SplitCount   int     `json:"split_count"`

	SeriesID      string    `json:"series_id"`
	InvoiceNumber string    `json:"invoice_number"`
	InvoiceDate   time.Time `json:"invoice_date"`
	// Outlet-local YYYY-MM-DD; the business day may cross midnight (CLAUDE.md).
	BusinessDate string `json:"business_date"`

	Status          InvoiceStatus `json:"status"`
	CancelledReason *string       `json:"cancelled_reason"`
	CancelledAt     *time.Time    `json:"cancelled_at"`

	CustomerName           *string `json:"customer_name"`
	CustomerPhone          *string `json:"customer_phone"`
	CustomerGSTIN          *string `json:"customer_gstin"`
	PlaceOfSupplyStateCode string  `json:"place_of_supply_state_code"`

	Lines []InvoiceLine `json:"lines"`

	// Money — integer paise throughout (CLAUDE.md §Money).
	SubtotalPaise     int `json:"subtotal_paise"`
	DiscountPaise     int `json:"discount_paise"`
	TaxableValuePaise int `json:"taxable_value_paise"`
	CGSTPaise         int `json:"cgst_paise"`
	SGSTPaise         int `json:"sgst_paise"`
	IGSTPaise         int `json:"igst_paise"`
	CessPaise         int `json:"cess_paise"`
	RoundOffPaise     int `json:"round_off_paise"`
	GrandTotalPaise   int `json:"grand_total_paise"`

	// Reproducibility (§31): the resolved rules AND the seller identity as they
	// stood at issue time, so a reprint after a rate or GSTIN change produces
	// the original document rather than a recomputed one.
	ComplianceVersionID string                 `json:"compliance_version_id"`
	TaxSnapshot         map[string]interface{} `json:"tax_snapshot"`
	FiscalProfile       map[string]interface{} `json:"fiscal_profile"`

	// ECO (§32) — modelled now, reported later.
	Channel              Channel           `json:"channel"`
	TaxLiabilityParty    TaxLiabilityParty `json:"tax_liability_party"`
	ECOOperatorName      *string           `json:"eco_operator_name"`
	ECOOperatorGSTIN     *string           `json:"eco_operator_gstin"`
	SupplyClassification *string           `json:"supply_classification"`

	CreatedByUserID string    `json:"created_by_user_id"`
	CreatedAt       time.Time `json:"created_at"`
	UpdatedAt       time.Time `json:"updated_at"`
	Version         int       `json:"version"`
	SchemaVersion   int       `json:"schema_version"`
}

// Channel is the sales channel an invoice was raised through. Kept as a named
// string rather than an enum because aggregator channels arrive in Milestone 6
// and the set is not closed at 0.4.0.
type Channel string

// SumsCorrectly reports whether the invoice satisfies the ADR-016 rounding
// policy: components sum to the grand total through round-off, round-off never
// exceeds half a rupee, and the total settles in whole rupees.
//
// The same policy is a CHECK in sqlite/0006 and postgres/0007 and a refine in
// the Zod schema. This method exists so an ingest handler can reject a
// malformed replay with a 422 that names the rule, rather than surfacing a raw
// constraint violation from the driver.
func (i Invoice) SumsCorrectly() bool {
	sum := i.TaxableValuePaise + i.CGSTPaise + i.SGSTPaise + i.IGSTPaise + i.CessPaise + i.RoundOffPaise
	if sum != i.GrandTotalPaise {
		return false
	}
	if i.RoundOffPaise > 50 || i.RoundOffPaise < -50 {
		return false
	}
	return i.GrandTotalPaise%100 == 0
}
