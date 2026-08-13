package compliance

import (
	"fmt"

	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

// Task 2/3: compute tax over a set of lines per ADR-016 §3, supporting both
// tax-inclusive and tax-exclusive pricing.
//
// The rounding policy, verbatim from ADR-016 §3 and mirrored by
// packages/contracts/go/invoice.go's Invoice.SumsCorrectly():
//   - tax is computed per line at full paise precision,
//   - summed per component (CGST/SGST/IGST/CESS) across the invoice,
//   - each component rounded half-up to paise exactly once,
//   - the grand total is then rounded to the nearest rupee, with the delta
//     recorded in round_off_paise.
//
// "Per line at full paise precision, summed... then rounded once" is why the
// invoice-level totals below are accumulated as (paise * rate_bps) integer
// products and divided by 10000 only ONCE, at the very end — never by adding
// together each line's own (separately rounded) display components, which is
// exactly the accumulation ADR-016 §3 rejects ("line-level rounding errors
// accumulate").

// ComputeInvoice computes every line and the invoice-level totals for a
// single invoice (or a single split part — ADR-016 §4 treats a split as N
// ordinary invoices, so this is also what a split invoice computes with).
func ComputeInvoice(lines []Line) ([]LineComputation, InvoiceTotals, error) {
	computed := make([]LineComputation, 0, len(lines))

	// Invoice-level component sums are accumulated at bps scale (paise *
	// rate_bps) so that dividing by bpsDenominator happens exactly once,
	// after every line has contributed — the ADR-016 §3 requirement.
	var scaledCGST, scaledSGST, scaledIGST, scaledCess int64
	var subtotal, discount, taxable int64

	for i, line := range lines {
		lc, err := computeLine(line)
		if err != nil {
			return nil, InvoiceTotals{}, fmt.Errorf("compliance: line %d: %w", i, err)
		}
		computed = append(computed, lc)

		subtotal += int64(lc.GrossPaise)
		discount += int64(lc.DiscountPaise)
		taxable += int64(lc.TaxableValuePaise)

		t := int64(lc.TaxableValuePaise)
		scaledCGST += t * int64(rateFor(line.Rates, contracts.TaxComponentCGST))
		scaledSGST += t * int64(rateFor(line.Rates, contracts.TaxComponentSGST))
		scaledIGST += t * int64(rateFor(line.Rates, contracts.TaxComponentIGST))
		scaledCess += t * int64(rateFor(line.Rates, contracts.TaxComponentCess))
	}

	cgst := roundComponentPaise(scaledCGST)
	sgst := roundComponentPaise(scaledSGST)
	igst := roundComponentPaise(scaledIGST)
	cess := roundComponentPaise(scaledCess)

	preRound := taxable + cgst + sgst + igst + cess
	grandTotal := roundToNearestRupee(preRound)
	roundOff := grandTotal - preRound

	totals := InvoiceTotals{
		SubtotalPaise:     int(subtotal),
		DiscountPaise:     int(discount),
		TaxableValuePaise: int(taxable),
		CGSTPaise:         int(cgst),
		SGSTPaise:         int(sgst),
		IGSTPaise:         int(igst),
		CessPaise:         int(cess),
		RoundOffPaise:     int(roundOff),
		GrandTotalPaise:   int(grandTotal),
	}
	return computed, totals, nil
}

// computeLine computes one line's own display values. TaxableValuePaise and
// the per-component amounts here are for THIS LINE's own record (an
// invoice_line row / a printed line item) and are deliberately NOT what
// ComputeInvoice sums to produce the invoice-level totals (see the package
// doc above) — they exist so a printed line shows a self-consistent
// breakdown: TotalPaise always equals TaxableValuePaise plus this line's own
// tax components, to the paise, in both pricing modes (Task 3's "must not
// lose or gain a paise").
func computeLine(line Line) (LineComputation, error) {
	if line.Quantity <= 0 {
		return LineComputation{}, fmt.Errorf("%w: quantity must be positive", httpx.ErrInvalidInput)
	}
	if line.UnitPricePaise < 0 {
		return LineComputation{}, fmt.Errorf("%w: unit_price_paise must not be negative", httpx.ErrInvalidInput)
	}
	if line.DiscountPerUnitPaise < 0 {
		return LineComputation{}, fmt.Errorf("%w: discount_per_unit_paise must not be negative", httpx.ErrInvalidInput)
	}
	if line.DiscountPerUnitPaise > line.UnitPricePaise {
		return LineComputation{}, fmt.Errorf("%w: discount_per_unit_paise must not exceed unit_price_paise", httpx.ErrInvalidInput)
	}
	for _, r := range line.Rates {
		if r.RateBps < 0 {
			return LineComputation{}, fmt.Errorf("%w: rate_bps must not be negative", httpx.ErrInvalidInput)
		}
	}

	gross := int64(line.UnitPricePaise) * int64(line.Quantity)
	lineDiscount := int64(line.DiscountPerUnitPaise) * int64(line.Quantity)
	net := gross - lineDiscount // always >= 0: DiscountPerUnit <= UnitPrice, checked above

	rateSum := int64(sumRateBps(line.Rates))

	var taxableValue int64
	var lineTaxTotal int64
	switch line.PricingMode {
	case contracts.PricingModeExclusive:
		// Tax added on top of net: taxable value IS net, exactly (no
		// division, so no rounding is even possible here).
		taxableValue = net
		lineTaxTotal = roundComponentPaise(taxableValue * rateSum)
	case contracts.PricingModeInclusive:
		// Tax is embedded in net; back-compute the taxable value once
		// (half-up), then force the tax total to be EXACTLY what remains of
		// net — never re-derive it from the rounded taxable value, which
		// would risk losing or gaining a paise on reconstruction (Task 3).
		denominator := bpsDenominator + rateSum
		if denominator <= 0 {
			return LineComputation{}, fmt.Errorf("%w: total rate must be greater than -100%%", httpx.ErrInvalidInput)
		}
		taxableValue = roundHalfUpDiv(net*bpsDenominator, denominator)
		lineTaxTotal = net - taxableValue
	default:
		return LineComputation{}, fmt.Errorf("%w: pricing_mode %q is not valid", httpx.ErrInvalidInput, line.PricingMode)
	}

	weights := make([]int64, len(componentOrder))
	for i, c := range componentOrder {
		weights[i] = int64(rateFor(line.Rates, c))
	}
	shares := largestRemainderSplit(lineTaxTotal, weights)

	lc := LineComputation{
		OrderItemID:       line.OrderItemID,
		Description:       line.Description,
		HSNSAC:            line.HSNSAC,
		Quantity:          line.Quantity,
		UnitPricePaise:    line.UnitPricePaise,
		GrossPaise:        int(gross),
		DiscountPaise:     int(lineDiscount),
		TaxableValuePaise: int(taxableValue),
		TaxProfileID:      line.TaxProfileID,
		CGSTRateBps:       rateFor(line.Rates, contracts.TaxComponentCGST),
		CGSTPaise:         int(shares[0]),
		SGSTRateBps:       rateFor(line.Rates, contracts.TaxComponentSGST),
		SGSTPaise:         int(shares[1]),
		IGSTRateBps:       rateFor(line.Rates, contracts.TaxComponentIGST),
		IGSTPaise:         int(shares[2]),
		CessRateBps:       rateFor(line.Rates, contracts.TaxComponentCess),
		CessPaise:         int(shares[3]),
	}
	lc.TotalPaise = lc.TaxableValuePaise + lc.CGSTPaise + lc.SGSTPaise + lc.IGSTPaise + lc.CessPaise
	return lc, nil
}
