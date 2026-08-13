package compliance

import (
	"testing"

	contracts "github.com/holler/contracts"
)

// gstRestaurant5 is a realistic split-rate profile: 2.5% CGST + 2.5% SGST =
// 5% GST, the common India restaurant rate.
func gstRestaurant5() []ResolvedRate {
	return []ResolvedRate{
		{Component: contracts.TaxComponentCGST, RateBps: 250},
		{Component: contracts.TaxComponentSGST, RateBps: 250},
	}
}

func TestComputeInvoice_ExclusivePricing_WorkedExample(t *testing.T) {
	// A dine-in bill: 2x Butter Chicken @ ₹320 (with a ₹20/unit modifier
	// delta already folded into unit price), 1x Coke @ ₹60, plus a cess-
	// bearing item (a sweetened beverage) 1x @ ₹50 with 12% GST + 2.8% cess
	// stacked on top — exercising "cess stacked on GST" explicitly.
	lines := []Line{
		{
			OrderItemID: "item-1", Description: "Butter Chicken", Quantity: 2,
			UnitPricePaise: 32000, TaxProfileID: "gst5",
			PricingMode: contracts.PricingModeExclusive, Rates: gstRestaurant5(),
		},
		{
			OrderItemID: "item-2", Description: "Coke", Quantity: 1,
			UnitPricePaise: 6000, TaxProfileID: "gst5",
			PricingMode: contracts.PricingModeExclusive, Rates: gstRestaurant5(),
		},
		{
			OrderItemID: "item-3", Description: "Sweetened Beverage", Quantity: 1,
			UnitPricePaise: 5000, TaxProfileID: "gst12cess",
			PricingMode: contracts.PricingModeExclusive,
			Rates: []ResolvedRate{
				{Component: contracts.TaxComponentCGST, RateBps: 600},
				{Component: contracts.TaxComponentSGST, RateBps: 600},
				{Component: contracts.TaxComponentCess, RateBps: 280},
			},
		},
	}

	lineComputations, totals, err := ComputeInvoice(lines)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(lineComputations) != 3 {
		t.Fatalf("expected 3 lines, got %d", len(lineComputations))
	}

	// Line 1: taxable = 64000 (2*32000), no discount.
	if lineComputations[0].TaxableValuePaise != 64000 {
		t.Fatalf("line 1 taxable = %d, want 64000", lineComputations[0].TaxableValuePaise)
	}
	// Line 3: taxable 5000 @ 12% = 600 tax, + 2.8% cess = 140 -> total tax 740.
	if lineComputations[2].TotalPaise != 5000+600+140 {
		t.Fatalf("line 3 total = %d, want %d", lineComputations[2].TotalPaise, 5000+600+140)
	}

	// Invoice totals: taxable = 64000+6000+5000 = 75000.
	// CGST = 64000*2.5% + 6000*2.5% + 5000*6% = 1600+150+300 = 2050
	// SGST = same = 2050
	// CESS = 5000*2.8% = 140
	if totals.TaxableValuePaise != 75000 {
		t.Fatalf("taxable = %d, want 75000", totals.TaxableValuePaise)
	}
	if totals.CGSTPaise != 2050 || totals.SGSTPaise != 2050 {
		t.Fatalf("cgst=%d sgst=%d, want 2050/2050", totals.CGSTPaise, totals.SGSTPaise)
	}
	if totals.CessPaise != 140 {
		t.Fatalf("cess = %d, want 140", totals.CessPaise)
	}
	preRound := totals.TaxableValuePaise + totals.CGSTPaise + totals.SGSTPaise + totals.IGSTPaise + totals.CessPaise
	if preRound != 79240 {
		t.Fatalf("pre-round total = %d, want 79240", preRound)
	}
	// 79240 -> nearest rupee 79200 (remainder 40 < 50, rounds down).
	if totals.GrandTotalPaise != 79200 {
		t.Fatalf("grand total = %d, want 79200", totals.GrandTotalPaise)
	}
	if totals.RoundOffPaise != -40 {
		t.Fatalf("round off = %d, want -40", totals.RoundOffPaise)
	}

	invoice := toInvoice(totals)
	if !invoice.SumsCorrectly() {
		t.Fatalf("invoice does not satisfy Invoice.SumsCorrectly(): %+v", invoice)
	}
}

func TestComputeInvoice_InclusivePricing_ReconstructsGrossExactly(t *testing.T) {
	// ₹105 inclusive of 5% GST (2.5+2.5). Taxable = 105*10000/10500 = 100
	// exactly (a clean case), but exercised alongside a messier one below.
	lines := []Line{
		{
			OrderItemID: "item-1", Description: "Thali", Quantity: 1,
			UnitPricePaise: 10500, TaxProfileID: "gst5",
			PricingMode: contracts.PricingModeInclusive, Rates: gstRestaurant5(),
		},
		{
			// A price that does NOT divide evenly: ₹99 inclusive of 5%.
			OrderItemID: "item-2", Description: "Lassi", Quantity: 3,
			UnitPricePaise: 9900, TaxProfileID: "gst5",
			PricingMode: contracts.PricingModeInclusive, Rates: gstRestaurant5(),
		},
	}

	lineComputations, _, err := ComputeInvoice(lines)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	for i, lc := range lineComputations {
		gross := lc.GrossPaise - lc.DiscountPaise
		reconstructed := lc.TaxableValuePaise + lc.CGSTPaise + lc.SGSTPaise + lc.IGSTPaise + lc.CessPaise
		if reconstructed != gross {
			t.Fatalf("line %d: taxable+components = %d, want exactly gross-discount %d (no paise lost or gained)",
				i, reconstructed, gross)
		}
		if lc.TotalPaise != gross {
			t.Fatalf("line %d: TotalPaise = %d, want %d", i, lc.TotalPaise, gross)
		}
	}

	// Line 1: taxable = 10500*10000/10500 = 100.00 exactly = 10000 paise.
	if lineComputations[0].TaxableValuePaise != 10000 {
		t.Fatalf("line 1 taxable = %d, want 10000", lineComputations[0].TaxableValuePaise)
	}
}

func TestComputeInvoice_ZeroValueAndSinglePaiseLines(t *testing.T) {
	lines := []Line{
		{OrderItemID: "free-item", Description: "Complimentary Papad", Quantity: 1,
			UnitPricePaise: 0, TaxProfileID: "gst5", PricingMode: contracts.PricingModeExclusive, Rates: gstRestaurant5()},
		{OrderItemID: "one-paise", Description: "Rounding stress", Quantity: 1,
			UnitPricePaise: 1, TaxProfileID: "gst5", PricingMode: contracts.PricingModeExclusive, Rates: gstRestaurant5()},
	}
	_, totals, err := ComputeInvoice(lines)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	invoice := toInvoice(totals)
	if !invoice.SumsCorrectly() {
		t.Fatalf("invoice does not satisfy Invoice.SumsCorrectly(): %+v", invoice)
	}
}

func TestComputeInvoice_RejectsInvalidLines(t *testing.T) {
	badLines := [][]Line{
		{{OrderItemID: "x", Quantity: 0, UnitPricePaise: 100, PricingMode: contracts.PricingModeExclusive}},
		{{OrderItemID: "x", Quantity: 1, UnitPricePaise: -1, PricingMode: contracts.PricingModeExclusive}},
		{{OrderItemID: "x", Quantity: 1, UnitPricePaise: 100, DiscountPerUnitPaise: 200, PricingMode: contracts.PricingModeExclusive}},
		{{OrderItemID: "x", Quantity: 1, UnitPricePaise: 100, PricingMode: "BOGUS"}},
	}
	for i, lines := range badLines {
		if _, _, err := ComputeInvoice(lines); err == nil {
			t.Fatalf("case %d: expected an error, got none", i)
		}
	}
}

// toInvoice builds just enough of a contracts.Invoice to exercise
// Invoice.SumsCorrectly(), the mandated check per the orchestrator's brief.
func toInvoice(totals InvoiceTotals) contracts.Invoice {
	return contracts.Invoice{
		TaxableValuePaise: totals.TaxableValuePaise,
		CGSTPaise:         totals.CGSTPaise,
		SGSTPaise:         totals.SGSTPaise,
		IGSTPaise:         totals.IGSTPaise,
		CessPaise:         totals.CessPaise,
		RoundOffPaise:     totals.RoundOffPaise,
		GrandTotalPaise:   totals.GrandTotalPaise,
	}
}
