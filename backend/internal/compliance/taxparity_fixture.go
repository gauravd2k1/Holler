package compliance

import (
	"encoding/json"

	contracts "github.com/holler/contracts"
)

// This file is the committed, runnable regeneration path for
// edge/database/tests/fixtures/tax_parity.json (T14, following the T7a
// verification gate's ruling that an uncommitted generator "is not
// acceptable long-term").
//
// edge/database/tests/tax_parity.rs asserts the Rust tax engine reproduces
// this package's ComputeInvoice output byte-for-byte, across the 10 cases
// defined in taxParityCases below. If ANYTHING in this package's arithmetic
// changes — a rate table, a rounding tweak, an allocation order — the JSON
// this file emits changes too, and taxparity_fixture_test.go's
// TestGenerateTaxParityFixture_MatchesCommittedFixture fails the moment
// `go test ./internal/compliance/...` runs, rather than silently
// desynchronising the two engines until an accountant finds the paise.
//
// To regenerate edge/database/tests/fixtures/tax_parity.json by hand after a
// deliberate engine change (this package does NOT write into edge/database
// itself — that tree belongs to a different track):
//
//	go test ./internal/compliance/ -run TestGenerateTaxParityFixture -write-fixture=<path>
//
// then copy <path> over edge/database/tests/fixtures/tax_parity.json and let
// the Rust suite (cargo test -p holler_edge_database --test tax_parity)
// confirm the two engines still agree.

// The JSON shape below matches edge/database/tests/fixtures/tax_parity.json
// field-for-field and in field order — see tax_parity_test.go's committed
// fixture diff for why the order matters (byte-for-byte comparison, not
// semantic JSON equality).

type taxParityRate struct {
	Component string `json:"component"`
	RateBps   int    `json:"rate_bps"`
}

type taxParityLine struct {
	OrderItemID          string          `json:"order_item_id"`
	Description          string          `json:"description"`
	Quantity             int             `json:"quantity"`
	UnitPricePaise       int             `json:"unit_price_paise"`
	DiscountPerUnitPaise int             `json:"discount_per_unit_paise"`
	TaxProfileID         string          `json:"tax_profile_id"`
	PricingMode          string          `json:"pricing_mode"`
	Rates                []taxParityRate `json:"rates"`
}

type taxParityLineComputation struct {
	OrderItemID       string `json:"order_item_id"`
	Quantity          int    `json:"quantity"`
	UnitPricePaise    int    `json:"unit_price_paise"`
	GrossPaise        int    `json:"gross_paise"`
	DiscountPaise     int    `json:"discount_paise"`
	TaxableValuePaise int    `json:"taxable_value_paise"`
	TaxProfileID      string `json:"tax_profile_id"`
	CGSTRateBps       int    `json:"cgst_rate_bps"`
	CGSTPaise         int    `json:"cgst_paise"`
	SGSTRateBps       int    `json:"sgst_rate_bps"`
	SGSTPaise         int    `json:"sgst_paise"`
	IGSTRateBps       int    `json:"igst_rate_bps"`
	IGSTPaise         int    `json:"igst_paise"`
	CessRateBps       int    `json:"cess_rate_bps"`
	CessPaise         int    `json:"cess_paise"`
	TotalPaise        int    `json:"total_paise"`
}

type taxParityTotals struct {
	SubtotalPaise     int `json:"subtotal_paise"`
	DiscountPaise     int `json:"discount_paise"`
	TaxableValuePaise int `json:"taxable_value_paise"`
	CGSTPaise         int `json:"cgst_paise"`
	SGSTPaise         int `json:"sgst_paise"`
	IGSTPaise         int `json:"igst_paise"`
	CessPaise         int `json:"cess_paise"`
	RoundOffPaise     int `json:"round_off_paise"`
	GrandTotalPaise   int `json:"grand_total_paise"`
}

type taxParityCase struct {
	Name             string                     `json:"name"`
	Lines            []taxParityLine            `json:"lines"`
	LineComputations []taxParityLineComputation `json:"line_computations"`
	Totals           taxParityTotals            `json:"totals"`
}

type taxParityFixture struct {
	ComputeCases []taxParityCase `json:"compute_cases"`
}

// taxParityCaseInput is one named case's raw lines, before ComputeInvoice
// runs — the transcription step a human must get right; everything
// downstream (line_computations, totals) is derived by this package's own
// engine, not hand-typed, so it can never itself drift from ComputeInvoice.
type taxParityCaseInput struct {
	Name  string
	Lines []taxParityLine
}

// taxParityCases is the case list edge/database/tests/tax_parity.rs's doc
// comment names as mandatory: exclusive and inclusive worked examples, zero
// and single-paise lines, a ₹x.x5 boundary, 15 small lines, cess on GST,
// IGST-only, fully exempt, both pricing modes on one invoice, and a
// three-profile mixed-rate bill. Order and content must match
// edge/database/tests/fixtures/tax_parity.json exactly.
func taxParityCases() []taxParityCaseInput {
	rate := func(component string, bps int) taxParityRate {
		return taxParityRate{Component: component, RateBps: bps}
	}
	gst5 := []taxParityRate{rate("CGST", 250), rate("SGST", 250)}
	gst12cess := []taxParityRate{rate("CGST", 600), rate("SGST", 600), rate("CESS", 280)}
	gst18 := []taxParityRate{rate("CGST", 900), rate("SGST", 900)}
	igst12 := []taxParityRate{rate("IGST", 1200)}

	line := func(id, desc string, qty, price, discount int, profile string, mode string, rates []taxParityRate) taxParityLine {
		return taxParityLine{
			OrderItemID:          id,
			Description:          desc,
			Quantity:             qty,
			UnitPricePaise:       price,
			DiscountPerUnitPaise: discount,
			TaxProfileID:         profile,
			PricingMode:          mode,
			Rates:                rates,
		}
	}

	smallLines := make([]taxParityLine, 0, 15)
	for i := 0; i < 15; i++ {
		id := "small-" + itoa(i)
		price := 13 + i
		smallLines = append(smallLines, line(id, "Small Line", 1, price, 0, "gst5", "EXCLUSIVE", gst5))
	}

	return []taxParityCaseInput{
		{
			Name: "exclusive_worked_example",
			Lines: []taxParityLine{
				line("item-1", "Butter Chicken", 2, 32000, 0, "gst5", "EXCLUSIVE", gst5),
				line("item-2", "Coke", 1, 6000, 0, "gst5", "EXCLUSIVE", gst5),
				line("item-3", "Sweetened Beverage", 1, 5000, 0, "gst12cess", "EXCLUSIVE", gst12cess),
			},
		},
		{
			Name: "inclusive_reconstructs_gross",
			Lines: []taxParityLine{
				line("item-1", "Thali", 1, 10500, 0, "gst5", "INCLUSIVE", gst5),
				line("item-2", "Lassi", 3, 9900, 0, "gst5", "INCLUSIVE", gst5),
			},
		},
		{
			Name: "zero_and_single_paise_lines",
			Lines: []taxParityLine{
				line("free-item", "Complimentary Papad", 1, 0, 0, "gst5", "EXCLUSIVE", gst5),
				line("one-paise", "Rounding stress", 1, 1, 0, "gst5", "EXCLUSIVE", gst5),
			},
		},
		{
			Name: "half_paise_boundary_18pct",
			Lines: []taxParityLine{
				line("boundary-1", "Boundary Item", 1, 25, 0, "gst18", "EXCLUSIVE", gst18),
				line("boundary-2", "Boundary Item Inclusive", 1, 1025, 0, "gst5", "INCLUSIVE", gst5),
			},
		},
		{
			Name:  "many_small_lines",
			Lines: smallLines,
		},
		{
			Name: "cess_stacked_on_gst",
			Lines: []taxParityLine{
				line("cess-1", "Aerated Drink", 3, 4500, 0, "gst12cess", "EXCLUSIVE", gst12cess),
				line("cess-2", "Aerated Drink Inclusive", 2, 5600, 0, "gst12cess", "INCLUSIVE", gst12cess),
			},
		},
		{
			Name: "igst_only",
			Lines: []taxParityLine{
				line("igst-1", "Interstate Delivery Item", 4, 18500, 500, "igst12", "EXCLUSIVE", igst12),
			},
		},
		{
			Name: "fully_exempt",
			Lines: []taxParityLine{
				line("exempt-1", "Fresh Fruit Plate", 2, 12000, 0, "exempt", "EXCLUSIVE", []taxParityRate{}),
			},
		},
		{
			Name: "mixed_pricing_modes_one_invoice",
			Lines: []taxParityLine{
				line("mix-1", "Butter Naan (exclusive)", 4, 4000, 0, "gst5", "EXCLUSIVE", gst5),
				line("mix-2", "Thali (inclusive)", 2, 31500, 0, "gst5", "INCLUSIVE", gst5),
				line("mix-3", "Beer (inclusive, IGST)", 1, 22000, 1000, "igst12", "INCLUSIVE", igst12),
			},
		},
		{
			Name: "mixed_rate_three_profiles",
			Lines: []taxParityLine{
				line("prof-a-1", "Veg Thali", 2, 25000, 0, "food5", "EXCLUSIVE", gst5),
				line("prof-a-2", "Roti", 6, 2500, 0, "food5", "INCLUSIVE", gst5),
				line("prof-b-1", "Imported Whiskey", 1, 85000, 0, "liquor18", "EXCLUSIVE", gst18),
				line("prof-b-2", "Local Beer", 3, 18000, 500, "liquor18", "INCLUSIVE", gst18),
				line("prof-c-1", "Packaged Aerated Drink", 5, 6000, 0, "cess12", "EXCLUSIVE", gst12cess),
				line("prof-c-2", "Ice Cream Sundae", 2, 15000, 0, "cess12", "INCLUSIVE", gst12cess),
			},
		},
	}
}

// itoa avoids importing strconv solely for a loop counter suffix — kept
// tiny and local since it's only ever used to build small-0..small-14.
func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	digits := [20]byte{}
	i := len(digits)
	for n > 0 {
		i--
		digits[i] = byte('0' + n%10)
		n /= 10
	}
	return string(digits[i:])
}

// toEngineLine converts one fixture-shaped input line into this package's
// own Line type, so the fixture is generated by running the SAME
// ComputeInvoice a real invoice goes through — never a hand-computed
// parallel arithmetic path.
func (l taxParityLine) toEngineLine() (Line, error) {
	rates := make([]ResolvedRate, 0, len(l.Rates))
	for _, r := range l.Rates {
		rates = append(rates, ResolvedRate{
			Component: contracts.TaxComponent(r.Component),
			RateBps:   r.RateBps,
		})
	}
	mode := contracts.PricingModeExclusive
	if l.PricingMode == string(contracts.PricingModeInclusive) {
		mode = contracts.PricingModeInclusive
	}
	return Line{
		OrderItemID:          l.OrderItemID,
		Description:          l.Description,
		Quantity:             l.Quantity,
		UnitPricePaise:       l.UnitPricePaise,
		DiscountPerUnitPaise: l.DiscountPerUnitPaise,
		TaxProfileID:         l.TaxProfileID,
		PricingMode:          mode,
		Rates:                rates,
	}, nil
}

// generateTaxParityFixture runs every taxParityCases() input through this
// package's live ComputeInvoice and assembles the JSON shape
// edge/database/tests/fixtures/tax_parity.json expects.
func generateTaxParityFixture() (taxParityFixture, error) {
	cases := taxParityCases()
	fixture := taxParityFixture{ComputeCases: make([]taxParityCase, 0, len(cases))}

	for _, c := range cases {
		engineLines := make([]Line, 0, len(c.Lines))
		for _, l := range c.Lines {
			el, err := l.toEngineLine()
			if err != nil {
				return taxParityFixture{}, err
			}
			engineLines = append(engineLines, el)
		}

		lcs, totals, err := ComputeInvoice(engineLines)
		if err != nil {
			return taxParityFixture{}, err
		}

		lineComputations := make([]taxParityLineComputation, 0, len(lcs))
		for _, lc := range lcs {
			lineComputations = append(lineComputations, taxParityLineComputation{
				OrderItemID:       lc.OrderItemID,
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
			})
		}

		fixture.ComputeCases = append(fixture.ComputeCases, taxParityCase{
			Name:             c.Name,
			Lines:            c.Lines,
			LineComputations: lineComputations,
			Totals: taxParityTotals{
				SubtotalPaise:     totals.SubtotalPaise,
				DiscountPaise:     totals.DiscountPaise,
				TaxableValuePaise: totals.TaxableValuePaise,
				CGSTPaise:         totals.CGSTPaise,
				SGSTPaise:         totals.SGSTPaise,
				IGSTPaise:         totals.IGSTPaise,
				CessPaise:         totals.CessPaise,
				RoundOffPaise:     totals.RoundOffPaise,
				GrandTotalPaise:   totals.GrandTotalPaise,
			},
		})
	}

	return fixture, nil
}

// marshalTaxParityFixture renders fixture with the same 2-space indent and
// no trailing newline that edge/database/tests/fixtures/tax_parity.json is
// committed with, so a byte-for-byte diff against the committed file is
// meaningful rather than an artifact of formatting differences.
func marshalTaxParityFixture(fixture taxParityFixture) ([]byte, error) {
	return json.MarshalIndent(fixture, "", "  ")
}
