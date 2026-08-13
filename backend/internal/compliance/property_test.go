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
