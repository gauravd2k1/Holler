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
// # Reconciliation rule (added after a verifier-found defect: a printed
// invoice's line components must sum to its own printed invoice-level
// components, always, exactly — never approximately)
//
// The two pricing modes need genuinely different per-line arithmetic, and
// naively applying one invoice-level rounding pass over the raw (paise *
// rate_bps) sum — while independently deciding each line's own display
// values by a different route — lets the two drift apart. The fix composes
// the invoice-level totals FROM the same numbers the lines display, per
// pricing mode, so drift is structurally impossible rather than merely rare:
//
//  1. INCLUSIVE lines: the line's own total tax is EXACT by construction —
//     net minus the (once, half-up rounded) taxable value, with no further
//     rounding possible. That exact integer is distributed across the
//     line's own components via largestRemainderSplit, which is
//     total-preserving. This is what makes back-computing a tax-inclusive
//     price lose or gain nothing (Task 3) — an invariant a verifier
//     independently confirmed. It must not change.
//  2. EXCLUSIVE lines have no such per-line identity to preserve (tax is
//     added on top of an already-exact taxable value), so they are free to
//     absorb the ADR-016 §3 "round once, at the invoice level" step: the
//     raw (taxable*rate_bps) contribution is summed across every EXCLUSIVE
//     line and rounded half-up exactly once per component, and that rounded
//     integer is then allocated back across the EXCLUSIVE lines via
//     largestRemainderSplit (weighted by each line's raw contribution) so
//     they still print a self-consistent, total-preserving breakdown.
//  3. The invoice's own component total is then DEFINED as the sum of the
//     two: (exact sum of INCLUSIVE lines' components) + (the single
//     invoice-level rounded EXCLUSIVE total). Since every line's own
//     printed component is either a literal summand of term 1 or an exact
//     partition (via largestRemainderSplit) of term 2, Σ(line components)
//     == invoice component is an algebraic identity, not a coincidence.
//
// largestRemainderSplit therefore has two distinct, still-necessary roles:
// distributing one INCLUSIVE line's own exact tax across its components,
// and distributing the invoice-level EXCLUSIVE total across EXCLUSIVE
// lines.
func ComputeInvoice(lines []Line) ([]LineComputation, InvoiceTotals, error) {
	computed := make([]LineComputation, len(lines))

	var subtotal, discount, taxable int64
	// Raw (paise * rate_bps) sums, EXCLUSIVE lines only — rounded once, at
	// invoice level, per ADR-016 §3.
	var exclusiveScaled [4]int64 // indexed by componentOrder
	// Exact sums, INCLUSIVE lines only — already integer paise, no further
	// rounding: summing exact numbers cannot accumulate rounding error.
	var inclusiveSum [4]int64

	exclusiveIdx := make([]int, 0, len(lines))
	exclusiveWeights := make([][4]int64, 0, len(lines))

	for i, line := range lines {
		lc, err := computeLineBase(line)
		if err != nil {
			return nil, InvoiceTotals{}, fmt.Errorf("compliance: line %d: %w", i, err)
		}

		subtotal += int64(lc.GrossPaise)
		discount += int64(lc.DiscountPaise)
		taxable += int64(lc.TaxableValuePaise)

		switch line.PricingMode {
		case contracts.PricingModeInclusive:
			finishInclusiveLine(&lc, line)
			for c := range componentOrder {
				inclusiveSum[c] += int64(componentValue(lc, componentOrder[c]))
			}
		case contracts.PricingModeExclusive:
			var weights [4]int64
			t := int64(lc.TaxableValuePaise)
			for c := range componentOrder {
				w := t * int64(rateFor(line.Rates, componentOrder[c]))
				weights[c] = w
				exclusiveScaled[c] += w
			}
			exclusiveIdx = append(exclusiveIdx, i)
			exclusiveWeights = append(exclusiveWeights, weights)
		}
		computed[i] = lc
	}

	var exclusiveRounded [4]int64
	for c := range componentOrder {
		exclusiveRounded[c] = roundComponentPaise(exclusiveScaled[c])
	}

	// Allocate each EXCLUSIVE component's single invoice-level rounding back
	// across the EXCLUSIVE lines that contributed to it, via
	// largestRemainderSplit (total-preserving by construction).
	for c := range componentOrder {
		weights := make([]int64, len(exclusiveIdx))
		for k := range exclusiveIdx {
			weights[k] = exclusiveWeights[k][c]
		}
		shares := largestRemainderSplit(exclusiveRounded[c], weights)
		for k, lineIdx := range exclusiveIdx {
			setComponentValue(&computed[lineIdx], componentOrder[c], int(shares[k]))
		}
	}
	for _, lineIdx := range exclusiveIdx {
		lc := &computed[lineIdx]
		lc.TotalPaise = lc.TaxableValuePaise + lc.CGSTPaise + lc.SGSTPaise + lc.IGSTPaise + lc.CessPaise
	}

	cgst := inclusiveSum[0] + exclusiveRounded[0]
	sgst := inclusiveSum[1] + exclusiveRounded[1]
	igst := inclusiveSum[2] + exclusiveRounded[2]
	cess := inclusiveSum[3] + exclusiveRounded[3]

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

// componentValue/setComponentValue index a LineComputation's four tax
// component fields by componentOrder position, so the accumulation and
// allocation loops above can iterate generically instead of repeating
// per-component code four times over.
func componentValue(lc LineComputation, c contracts.TaxComponent) int {
	switch c {
	case contracts.TaxComponentCGST:
		return lc.CGSTPaise
	case contracts.TaxComponentSGST:
		return lc.SGSTPaise
	case contracts.TaxComponentIGST:
		return lc.IGSTPaise
	default:
		return lc.CessPaise
	}
}

func setComponentValue(lc *LineComputation, c contracts.TaxComponent, v int) {
	switch c {
	case contracts.TaxComponentCGST:
		lc.CGSTPaise = v
	case contracts.TaxComponentSGST:
		lc.SGSTPaise = v
	case contracts.TaxComponentIGST:
		lc.IGSTPaise = v
	default:
		lc.CessPaise = v
	}
}

// computeLineBase validates a line and computes the fields common to both
// pricing modes: Gross/Discount/TaxableValue and the rate_bps carried per
// component for display. Tax component amounts are filled in afterward by
// finishInclusiveLine (immediately) or by ComputeInvoice's cross-line
// allocation (for EXCLUSIVE lines, once every line has been seen).
func computeLineBase(line Line) (LineComputation, error) {
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
	if line.PricingMode != contracts.PricingModeExclusive && line.PricingMode != contracts.PricingModeInclusive {
		return LineComputation{}, fmt.Errorf("%w: pricing_mode %q is not valid", httpx.ErrInvalidInput, line.PricingMode)
	}

	gross := int64(line.UnitPricePaise) * int64(line.Quantity)
	lineDiscount := int64(line.DiscountPerUnitPaise) * int64(line.Quantity)
	net := gross - lineDiscount // always >= 0: DiscountPerUnit <= UnitPrice, checked above

	rateSum := int64(sumRateBps(line.Rates))

	var taxableValue int64
	switch line.PricingMode {
	case contracts.PricingModeExclusive:
		// Tax added on top of net: taxable value IS net, exactly (no
		// division, so no rounding is even possible here).
		taxableValue = net
	case contracts.PricingModeInclusive:
		// Tax is embedded in net; back-compute the taxable value once
		// (half-up). The remainder (net - taxableValue) is exact and is
		// finalized by finishInclusiveLine.
		denominator := bpsDenominator + rateSum
		if denominator <= 0 {
			return LineComputation{}, fmt.Errorf("%w: total rate must be greater than -100%%", httpx.ErrInvalidInput)
		}
		taxableValue = roundHalfUpDiv(net*bpsDenominator, denominator)
	}

	return LineComputation{
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
		SGSTRateBps:       rateFor(line.Rates, contracts.TaxComponentSGST),
		IGSTRateBps:       rateFor(line.Rates, contracts.TaxComponentIGST),
		CessRateBps:       rateFor(line.Rates, contracts.TaxComponentCess),
	}, nil
}

// finishInclusiveLine fills in an INCLUSIVE line's own tax components. The
// line's total tax (net - taxableValue) is EXACT — no rounding occurs here,
// only a total-preserving distribution across the line's own components —
// which is what guarantees TotalPaise reconstructs gross-discount to the
// paise (Task 3), independent of every other line on the invoice.
func finishInclusiveLine(lc *LineComputation, line Line) {
	net := int64(lc.GrossPaise - lc.DiscountPaise)
	lineTaxTotal := net - int64(lc.TaxableValuePaise)

	weights := make([]int64, len(componentOrder))
	for i, c := range componentOrder {
		weights[i] = int64(rateFor(line.Rates, c))
	}
	shares := largestRemainderSplit(lineTaxTotal, weights)
	lc.CGSTPaise = int(shares[0])
	lc.SGSTPaise = int(shares[1])
	lc.IGSTPaise = int(shares[2])
	lc.CessPaise = int(shares[3])
	lc.TotalPaise = lc.TaxableValuePaise + lc.CGSTPaise + lc.SGSTPaise + lc.IGSTPaise + lc.CessPaise
}
