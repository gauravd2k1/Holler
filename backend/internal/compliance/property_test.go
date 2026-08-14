package compliance

import (
	"math/rand"
	"testing"

	contracts "github.com/holler/contracts"
)

// §66 mandates two properties, generated/random rather than hand-picked,
// with interesting cases specifically at ₹x.x5 boundaries, zero-value and
// single-paise lines, many small lines (where per-line rounding would
// drift), and cess stacked on GST. randomLine below deliberately biases
// toward exactly those shapes.

const propertyIterations = 5000

// randomLine generates one line whose unit price is frequently constructed
// to land exactly on a half-paise-at-the-rate boundary (the ₹x.x5 case
// ADR-016 names), sometimes zero, sometimes a single paise, and whose rate
// set sometimes stacks a cess on top of a CGST/SGST or IGST pair.
func randomLine(r *rand.Rand, orderItemID string) Line {
	quantity := 1 + r.Intn(6)

	var unitPrice int
	switch r.Intn(5) {
	case 0:
		unitPrice = 0 // zero-value line
	case 1:
		unitPrice = 1 // single-paise line
	case 2:
		// Constructed so unitPrice*rateBps/10000 lands on exactly .5 paise
		// for an 18% rate: unitPrice = k*10000/1800 chosen so remainder is
		// exactly half — simplest reliable way is to fix a raw paise value
		// known to hit .x5 at 5%/12%/18%, e.g. multiples of 2 paise scaled.
		unitPrice = 5 + 2*r.Intn(5000) // odd-ish spread of small prices
	default:
		unitPrice = r.Intn(500000) // up to ₹5000
	}

	discountPerUnit := 0
	if r.Intn(3) == 0 && unitPrice > 0 {
		discountPerUnit = r.Intn(unitPrice + 1)
	}

	var rates []ResolvedRate
	useIGST := r.Intn(2) == 0
	if useIGST {
		rates = append(rates, ResolvedRate{Component: contracts.TaxComponentIGST, RateBps: []int{0, 500, 1200, 1800}[r.Intn(4)]})
	} else {
		half := []int{0, 250, 600, 900}[r.Intn(4)]
		rates = append(rates,
			ResolvedRate{Component: contracts.TaxComponentCGST, RateBps: half},
			ResolvedRate{Component: contracts.TaxComponentSGST, RateBps: half},
		)
	}
	if r.Intn(3) == 0 {
		rates = append(rates, ResolvedRate{Component: contracts.TaxComponentCess, RateBps: []int{0, 100, 280, 1200}[r.Intn(4)]})
	}

	mode := contracts.PricingModeExclusive
	if r.Intn(2) == 0 {
		mode = contracts.PricingModeInclusive
	}

	return Line{
		OrderItemID:          orderItemID,
		Description:          "random line",
		Quantity:             quantity,
		UnitPricePaise:       unitPrice,
		DiscountPerUnitPaise: discountPerUnit,
		TaxProfileID:         "random-profile",
		PricingMode:          mode,
		Rates:                rates,
	}
}

// TestProperty_GrandTotalReconciles is the first mandatory §66 property:
// grand_total = Σ(components) + round_off, and |round_off| <= 50 paise —
// checked here via the exact same Invoice.SumsCorrectly() the ADR names as
// binding, over thousands of randomly generated invoices.
func TestProperty_GrandTotalReconciles(t *testing.T) {
	r := rand.New(rand.NewSource(42))
	for i := 0; i < propertyIterations; i++ {
		lineCount := 1 + r.Intn(12) // "many small lines" case included
		lines := make([]Line, lineCount)
		for j := range lines {
			lines[j] = randomLine(r, "order-item")
		}

		_, totals, err := ComputeInvoice(lines)
		if err != nil {
			t.Fatalf("iteration %d: unexpected error: %v", i, err)
		}
		invoice := toInvoice(totals)
		if !invoice.SumsCorrectly() {
			t.Fatalf("iteration %d: Invoice.SumsCorrectly() failed for totals %+v (lines: %+v)", i, totals, lines)
		}
		if totals.RoundOffPaise > 50 || totals.RoundOffPaise < -50 {
			t.Fatalf("iteration %d: round_off %d exceeds the +/-50 paise bound", i, totals.RoundOffPaise)
		}
	}
}

// TestProperty_SplitGroupConservation is the second mandatory §66 property:
// across a split group, Σ(split invoice lines) = order lines exactly — no
// loss, no duplication, no double-tax — with group round-off bounded by 50
// paise * split_count.
func TestProperty_SplitGroupConservation(t *testing.T) {
	r := rand.New(rand.NewSource(1337))
	for i := 0; i < propertyIterations/5; i++ {
		orderLineCount := 1 + r.Intn(6)
		splitCount := 2 + r.Intn(4)

		orderLines := make([]Line, orderLineCount)
		for j := range orderLines {
			orderLines[j] = randomLine(r, orderItemID(j))
			// Split conservation is defined on WHOLE units per split (a
			// split bill divides items, not fractions of one item), so keep
			// quantity generous enough that a random partition across
			// splitCount parts can actually distribute it.
			orderLines[j].Quantity = 1 + r.Intn(20)
		}

		// Partition each order line's quantity across splitCount parts,
		// randomly, including zero for some parts — this is the "split N
		// ways" ADR-016 §4 describes.
		splitQuantities := make([][]int, orderLineCount)
		for j, line := range orderLines {
			splitQuantities[j] = partitionInto(r, line.Quantity, splitCount)
		}

		var sumSplitGross, sumSplitDiscount int64
		var sumSplitRoundOff int64
		for s := 0; s < splitCount; s++ {
			var splitLines []Line
			for j, line := range orderLines {
				q := splitQuantities[j][s]
				if q == 0 {
					continue // this order line has no portion in this split
				}
				partial := line
				partial.Quantity = q
				splitLines = append(splitLines, partial)
			}
			if len(splitLines) == 0 {
				continue // an empty split invoice is not issued
			}
			lineComputations, totals, err := ComputeInvoice(splitLines)
			if err != nil {
				t.Fatalf("iteration %d split %d: unexpected error: %v", i, s, err)
			}
			invoice := toInvoice(totals)
			if !invoice.SumsCorrectly() {
				t.Fatalf("iteration %d split %d: split invoice does not satisfy SumsCorrectly(): %+v", i, s, totals)
			}
			for _, lc := range lineComputations {
				sumSplitGross += int64(lc.GrossPaise)
				sumSplitDiscount += int64(lc.DiscountPaise)
			}
			sumSplitRoundOff += int64(totals.RoundOffPaise)
			if totals.RoundOffPaise > 50 || totals.RoundOffPaise < -50 {
				t.Fatalf("iteration %d split %d: round_off %d exceeds +/-50 paise", i, s, totals.RoundOffPaise)
			}
		}

		// Conservation: quantity and money for every order line, summed
		// across every split, equals the order line exactly — no loss, no
		// duplication.
		var orderGross, orderDiscount int64
		for j, line := range orderLines {
			var qSum int
			for _, q := range splitQuantities[j] {
				qSum += q
			}
			if qSum != line.Quantity {
				t.Fatalf("iteration %d line %d: split quantities sum to %d, want order quantity %d (loss or duplication)",
					i, j, qSum, line.Quantity)
			}
			orderGross += int64(line.UnitPricePaise) * int64(line.Quantity)
			orderDiscount += int64(line.DiscountPerUnitPaise) * int64(line.Quantity)
		}
		if sumSplitGross != orderGross {
			t.Fatalf("iteration %d: split gross sum %d != order gross %d", i, sumSplitGross, orderGross)
		}
		if sumSplitDiscount != orderDiscount {
			t.Fatalf("iteration %d: split discount sum %d != order discount %d", i, sumSplitDiscount, orderDiscount)
		}

		// Group round-off bound: 50 paise * split_count.
		bound := int64(50 * splitCount)
		if sumSplitRoundOff > bound || sumSplitRoundOff < -bound {
			t.Fatalf("iteration %d: group round-off %d exceeds +/-%d (50 * split_count=%d)",
				i, sumSplitRoundOff, bound, splitCount)
		}
	}
}

// TestProperty_LineComponentsReconcileWithInvoiceTotals is the THIRD §66
// property, added after the verifier's Defect 1 finding: for every generated
// invoice and every component (CGST/SGST/IGST/CESS),
//
//	Σ over lines of line.<component>Paise == invoice.<component>Paise
//
// exactly — never merely close. This is the documented reconciliation rule
// (engine.go's package doc explains WHY it holds for both pricing modes):
// invoice-level totals are *composed* from the same numbers the lines
// display, rather than the lines displaying an independent approximation
// that happens to usually agree. A printed GST invoice must never disagree
// with its own line items' columns, to the paise, ever.
func TestProperty_LineComponentsReconcileWithInvoiceTotals(t *testing.T) {
	r := rand.New(rand.NewSource(2026))
	for i := 0; i < propertyIterations; i++ {
		lineCount := 1 + r.Intn(12)
		lines := make([]Line, lineCount)
		for j := range lines {
			lines[j] = randomLine(r, "order-item")
		}

		lineComputations, totals, err := ComputeInvoice(lines)
		if err != nil {
			t.Fatalf("iteration %d: unexpected error: %v", i, err)
		}

		var sumCGST, sumSGST, sumIGST, sumCess int64
		for _, lc := range lineComputations {
			sumCGST += int64(lc.CGSTPaise)
			sumSGST += int64(lc.SGSTPaise)
			sumIGST += int64(lc.IGSTPaise)
			sumCess += int64(lc.CessPaise)
		}
		if sumCGST != int64(totals.CGSTPaise) {
			t.Fatalf("iteration %d: sum of line CGSTPaise = %d, invoice CGSTPaise = %d (lines: %+v)",
				i, sumCGST, totals.CGSTPaise, lines)
		}
		if sumSGST != int64(totals.SGSTPaise) {
			t.Fatalf("iteration %d: sum of line SGSTPaise = %d, invoice SGSTPaise = %d (lines: %+v)",
				i, sumSGST, totals.SGSTPaise, lines)
		}
		if sumIGST != int64(totals.IGSTPaise) {
			t.Fatalf("iteration %d: sum of line IGSTPaise = %d, invoice IGSTPaise = %d (lines: %+v)",
				i, sumIGST, totals.IGSTPaise, lines)
		}
		if sumCess != int64(totals.CessPaise) {
			t.Fatalf("iteration %d: sum of line CessPaise = %d, invoice CessPaise = %d (lines: %+v)",
				i, sumCess, totals.CessPaise, lines)
		}
	}
}

// mixedRateProfiles are the fixed set of distinct tax profiles the fourth
// property mixes on one invoice: a 5% split-rate food profile, an 18%
// split-rate profile, a 12%+2.8%-cess profile (cess stacked on GST), a
// 12%-only IGST profile (no CGST/SGST at all — the "component absent
// entirely" case for a different axis than 0%), and a fully exempt profile
// (no rates at all).
func mixedRateProfiles() [][]ResolvedRate {
	return [][]ResolvedRate{
		{ // 5% food (CGST+SGST)
			{Component: contracts.TaxComponentCGST, RateBps: 250},
			{Component: contracts.TaxComponentSGST, RateBps: 250},
		},
		{ // 18% (CGST+SGST)
			{Component: contracts.TaxComponentCGST, RateBps: 900},
			{Component: contracts.TaxComponentSGST, RateBps: 900},
		},
		{ // 12% + 2.8% cess stacked on GST
			{Component: contracts.TaxComponentCGST, RateBps: 600},
			{Component: contracts.TaxComponentSGST, RateBps: 600},
			{Component: contracts.TaxComponentCess, RateBps: 280},
		},
		{ // 12% IGST only — CGST/SGST/CESS are absent from this profile
			// entirely (not zero-rated: the component simply has no rule).
			{Component: contracts.TaxComponentIGST, RateBps: 1200},
		},
		{}, // fully exempt: no rates at all.
	}
}

// randomMixedRateLine generates one line under a randomly chosen profile from
// mixedRateProfiles, so a single invoice can mix several distinct rate sets —
// the shape the fourth §66 property requires (a real bill with a 5% food
// item, an 18% item, and a cess-bearing item together).
func randomMixedRateLine(r *rand.Rand, profiles [][]ResolvedRate, orderItemID string) Line {
	quantity := 1 + r.Intn(6)
	var unitPrice int
	switch r.Intn(4) {
	case 0:
		unitPrice = 0
	case 1:
		unitPrice = 1
	default:
		unitPrice = r.Intn(500000)
	}
	discountPerUnit := 0
	if r.Intn(3) == 0 && unitPrice > 0 {
		discountPerUnit = r.Intn(unitPrice + 1)
	}
	mode := contracts.PricingModeExclusive
	if r.Intn(2) == 0 {
		mode = contracts.PricingModeInclusive
	}
	profile := profiles[r.Intn(len(profiles))]
	return Line{
		OrderItemID:          orderItemID,
		Description:          "mixed-rate line",
		Quantity:             quantity,
		UnitPricePaise:       unitPrice,
		DiscountPerUnitPaise: discountPerUnit,
		TaxProfileID:         "mixed-profile",
		PricingMode:          mode,
		Rates:                profile,
	}
}

// TestProperty_MixedRateLineComponentsReconcile is the FOURTH §66 property,
// added at the human's request: the third property (line/invoice
// reconciliation) must hold not just when every line shares one profile, but
// across an invoice whose lines sit on GENUINELY DIFFERENT tax profiles —
// different rates, different component sets, some components absent
// entirely from a given line's profile. A line with no rule for a component
// must receive EXACTLY zero of it — never a stray remainder paise from
// largestRemainderSplit's allocation, which is the axis this property
// stresses that the third property (single-profile) cannot reach.
func TestProperty_MixedRateLineComponentsReconcile(t *testing.T) {
	profiles := mixedRateProfiles()
	r := rand.New(rand.NewSource(90210))
	for i := 0; i < propertyIterations; i++ {
		lineCount := 3 + r.Intn(10) // always enough lines to actually mix profiles
		lines := make([]Line, lineCount)
		for j := range lines {
			lines[j] = randomMixedRateLine(r, profiles, "order-item")
		}

		lineComputations, totals, err := ComputeInvoice(lines)
		if err != nil {
			t.Fatalf("iteration %d: unexpected error: %v", i, err)
		}

		var sumCGST, sumSGST, sumIGST, sumCess int64
		for j, lc := range lineComputations {
			sumCGST += int64(lc.CGSTPaise)
			sumSGST += int64(lc.SGSTPaise)
			sumIGST += int64(lc.IGSTPaise)
			sumCess += int64(lc.CessPaise)

			// A line whose profile carries no rule for a component must show
			// EXACTLY zero for it, regardless of how much tax other lines on
			// the same invoice generate for that component.
			if lc.CGSTRateBps == 0 && lc.CGSTPaise != 0 {
				t.Fatalf("iteration %d line %d: CGST rate is 0 but CGSTPaise = %d (rates: %+v)",
					i, j, lc.CGSTPaise, lines[j].Rates)
			}
			if lc.SGSTRateBps == 0 && lc.SGSTPaise != 0 {
				t.Fatalf("iteration %d line %d: SGST rate is 0 but SGSTPaise = %d (rates: %+v)",
					i, j, lc.SGSTPaise, lines[j].Rates)
			}
			if lc.IGSTRateBps == 0 && lc.IGSTPaise != 0 {
				t.Fatalf("iteration %d line %d: IGST rate is 0 but IGSTPaise = %d (rates: %+v)",
					i, j, lc.IGSTPaise, lines[j].Rates)
			}
			if lc.CessRateBps == 0 && lc.CessPaise != 0 {
				t.Fatalf("iteration %d line %d: Cess rate is 0 but CessPaise = %d (rates: %+v)",
					i, j, lc.CessPaise, lines[j].Rates)
			}
		}

		if sumCGST != int64(totals.CGSTPaise) {
			t.Fatalf("iteration %d: mixed-profile sum of line CGSTPaise = %d, invoice CGSTPaise = %d (lines: %+v)",
				i, sumCGST, totals.CGSTPaise, lines)
		}
		if sumSGST != int64(totals.SGSTPaise) {
			t.Fatalf("iteration %d: mixed-profile sum of line SGSTPaise = %d, invoice SGSTPaise = %d (lines: %+v)",
				i, sumSGST, totals.SGSTPaise, lines)
		}
		if sumIGST != int64(totals.IGSTPaise) {
			t.Fatalf("iteration %d: mixed-profile sum of line IGSTPaise = %d, invoice IGSTPaise = %d (lines: %+v)",
				i, sumIGST, totals.IGSTPaise, lines)
		}
		if sumCess != int64(totals.CessPaise) {
			t.Fatalf("iteration %d: mixed-profile sum of line CessPaise = %d, invoice CessPaise = %d (lines: %+v)",
				i, sumCess, totals.CessPaise, lines)
		}

		invoice := toInvoice(totals)
		if !invoice.SumsCorrectly() {
			t.Fatalf("iteration %d: mixed-profile invoice does not satisfy SumsCorrectly(): %+v", i, totals)
		}
	}
}

func orderItemID(i int) string {
	return "order-item-" + string(rune('a'+i))
}

// partitionInto randomly splits n indivisible units across k parts (each
// part >= 0), summing to exactly n — the shape of dividing one order line's
// quantity across a split bill's parts.
func partitionInto(r *rand.Rand, n, k int) []int {
	out := make([]int, k)
	for i := 0; i < n; i++ {
		out[r.Intn(k)]++
	}
	return out
}

// TestProperty_TotalsAreLineOrderIndependent is the FIFTH §66 property: "the
// discount application is order-independent where the spec says it is."
// ComputeInvoice's per-line arithmetic (computeLineBase, finishInclusiveLine)
// is applied independently to each line, and every invoice-level total this
// test checks is a SUM across lines — so permuting the input slice must never
// change subtotal, discount, taxable value, any tax component, round-off or
// grand total, even though largestRemainderSplit's tie-break "by bucket
// index" means the shuffle CAN move which individual EXCLUSIVE line receives
// a stray remainder paise. That per-line reshuffle is exactly why this
// property is pinned at the INVOICE level rather than asserting the per-line
// LineComputation slice is itself a fixed permutation of the original.
func TestProperty_TotalsAreLineOrderIndependent(t *testing.T) {
	r := rand.New(rand.NewSource(20260814))
	for i := 0; i < propertyIterations/5; i++ {
		lineCount := 2 + r.Intn(10) // need at least 2 lines for a permutation to mean anything
		lines := make([]Line, lineCount)
		for j := range lines {
			lines[j] = randomLine(r, orderItemID(j))
		}

		_, wantTotals, err := ComputeInvoice(lines)
		if err != nil {
			t.Fatalf("iteration %d: unexpected error on original order: %v", i, err)
		}

		shuffled := make([]Line, lineCount)
		copy(shuffled, lines)
		r.Shuffle(len(shuffled), func(a, b int) { shuffled[a], shuffled[b] = shuffled[b], shuffled[a] })

		_, gotTotals, err := ComputeInvoice(shuffled)
		if err != nil {
			t.Fatalf("iteration %d: unexpected error on shuffled order: %v", i, err)
		}

		if gotTotals != wantTotals {
			t.Fatalf("iteration %d: totals depend on line order.\noriginal lines: %+v\nshuffled lines: %+v\nwant: %+v\ngot:  %+v",
				i, lines, shuffled, wantTotals, gotTotals)
		}
	}
}
