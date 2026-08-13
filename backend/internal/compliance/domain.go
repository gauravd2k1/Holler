package compliance

import (
	contracts "github.com/holler/contracts"
)

// ResolvedRate is one tax component's rate as resolved for a moment in time
// (Task 1). RateBps is integer basis points — 2.5% = 250 — never a float
// (Task 5).
type ResolvedRate struct {
	Component contracts.TaxComponent `json:"component"`
	RateBps   int                    `json:"rate_bps"`
}

// componentOrder is the fixed, deterministic iteration order for tax
// components: CGST, SGST, IGST, then CESS stacked on top. Used everywhere a
// stable order matters (largest-remainder distribution, snapshot rendering)
// so two runs over identical input produce byte-identical output.
var componentOrder = []contracts.TaxComponent{
	contracts.TaxComponentCGST,
	contracts.TaxComponentSGST,
	contracts.TaxComponentIGST,
	contracts.TaxComponentCess,
}

// Line is one billable line handed to the engine. Money fields are per-unit
// so that splitting a line's Quantity across N split invoices (ADR-016 §4)
// distributes Gross/Discount exactly, with no residual paise to reconcile —
// the "no loss, no duplication" half of the §66 split-group property.
type Line struct {
	// OrderItemID is the order line this bills — what makes the split-group
	// conservation property checkable (ADR-016 §4).
	OrderItemID string
	Description string
	HSNSAC      *string
	Quantity    int
	// UnitPricePaise is tax-INCLUSIVE or tax-EXCLUSIVE depending on
	// PricingMode/TaxProfile — Task 3.
	UnitPricePaise int
	// DiscountPerUnitPaise is subtracted from UnitPricePaise before tax.
	// Per-unit (not a whole-line lump) so a split of Quantity distributes the
	// discount exactly, the same reasoning as UnitPricePaise above.
	DiscountPerUnitPaise int
	TaxProfileID         string
	PricingMode          contracts.PricingMode
	// Rates is this line's resolved rates (Task 1's ResolveRates output),
	// carried on the line because different lines in one invoice may sit
	// under different tax profiles (e.g. a liquor item taxed differently
	// from food on the same bill).
	Rates []ResolvedRate
}

// LineComputation is one computed invoice_line, field-for-field compatible
// with packages/contracts/go/invoice.go's InvoiceLine (minus ID/InvoiceID/
// LineNo/SchemaVersion, which the invoice-assembly caller assigns).
type LineComputation struct {
	OrderItemID string
	Description string
	HSNSAC      *string
	Quantity    int

	UnitPricePaise    int
	GrossPaise        int
	DiscountPaise     int
	TaxableValuePaise int

	TaxProfileID string

	CGSTRateBps int
	CGSTPaise   int
	SGSTRateBps int
	SGSTPaise   int
	IGSTRateBps int
	IGSTPaise   int
	CessRateBps int
	CessPaise   int

	TotalPaise int
}

// InvoiceTotals is the invoice-level money summary, field-for-field
// compatible with the money fields on packages/contracts/go/invoice.go's
// Invoice. These are the AUTHORITATIVE totals (ADR-016 §3): computed from
// the raw, unrounded per-component sum across every line and rounded once —
// never by summing each line's own (separately rounded) display components.
type InvoiceTotals struct {
	SubtotalPaise     int
	DiscountPaise     int
	TaxableValuePaise int
	CGSTPaise         int
	SGSTPaise         int
	IGSTPaise         int
	CessPaise         int
	RoundOffPaise     int
	GrandTotalPaise   int
}

// ToContractsInvoiceLine copies a LineComputation into a
// contracts.InvoiceLine, leaving ID/InvoiceID/LineNo/SchemaVersion for the
// caller to fill in (they are outside this engine's concern: ids are minted
// edge-side per §74, line_no is assignment order).
func (lc LineComputation) ToContractsInvoiceLine() contracts.InvoiceLine {
	return contracts.InvoiceLine{
		OrderItemID:       lc.OrderItemID,
		Description:       lc.Description,
		HSNSAC:            lc.HSNSAC,
		Quantity:          lc.Quantity,
		UnitPricePaise:    lc.UnitPricePaise,
		GrossPaise:        lc.GrossPaise,
		DiscountPaise:     lc.DiscountPaise,
		TaxableValuePaise: lc.TaxableValuePaise,
		TaxProfileID:      lc.TaxProfileID,
		CGSTRateBps:       lc.CGSTRateBps,
		CGSTPaise:         lc.CGSTPaise,
		SGSTRateBps:       lc.SGSTRateBps,
		SGSTPaise:         lc.SGSTPaise,
		IGSTRateBps:       lc.IGSTRateBps,
		IGSTPaise:         lc.IGSTPaise,
		CessRateBps:       lc.CessRateBps,
		CessPaise:         lc.CessPaise,
		TotalPaise:        lc.TotalPaise,
	}
}

// rateFor returns component's rate_bps within rates, 0 if the line's tax
// profile carries no rule for that component (e.g. an IGST-only profile has
// no CGST/SGST rule).
func rateFor(rates []ResolvedRate, component contracts.TaxComponent) int {
	for _, r := range rates {
		if r.Component == component {
			return r.RateBps
		}
	}
	return 0
}

func sumRateBps(rates []ResolvedRate) int {
	total := 0
	for _, r := range rates {
		total += r.RateBps
	}
	return total
}
