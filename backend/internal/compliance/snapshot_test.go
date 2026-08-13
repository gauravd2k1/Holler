package compliance

import (
	"testing"
	"time"

	contracts "github.com/holler/contracts"
)

func TestBuildTaxSnapshots_ReproducesHistoricalRules(t *testing.T) {
	outlet := "outlet-1"
	versions := []contracts.ComplianceVersion{
		{ID: "v-old", OutletID: outlet, Label: "FY25 rates", EffectiveFrom: parseTime("2025-01-01T00:00:00Z")},
		{ID: "v-new", OutletID: outlet, Label: "FY26 rates", EffectiveFrom: parseTime("2026-04-01T00:00:00Z")},
	}
	profiles := []contracts.TaxProfile{
		{ID: "p1", OutletID: outlet, Code: "GST_5_RESTAURANT", Name: "GST 5%",
			PricingMode: contracts.PricingModeExclusive, IsDefault: true, IsActive: true},
	}
	rules := []contracts.TaxRule{
		{ID: "r1-old", TaxProfileID: "p1", ComplianceVersionID: "v-old", Component: contracts.TaxComponentCGST,
			RateBps: 250, EffectiveFrom: parseTime("2025-01-01T00:00:00Z")},
		{ID: "r2-old", TaxProfileID: "p1", ComplianceVersionID: "v-old", Component: contracts.TaxComponentSGST,
			RateBps: 250, EffectiveFrom: parseTime("2025-01-01T00:00:00Z")},
		{ID: "r1-new", TaxProfileID: "p1", ComplianceVersionID: "v-new", Component: contracts.TaxComponentCGST,
			RateBps: 900, EffectiveFrom: parseTime("2026-04-01T00:00:00Z")},
		{ID: "r2-new", TaxProfileID: "p1", ComplianceVersionID: "v-new", Component: contracts.TaxComponentSGST,
			RateBps: 900, EffectiveFrom: parseTime("2026-04-01T00:00:00Z")},
	}
	lines := []Line{{TaxProfileID: "p1"}}

	historical, err := BuildTaxSnapshots(versions, profiles, rules, outlet, lines, parseTime("2025-06-01T00:00:00Z"))
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	snap := historical["p1"]
	if snap.ComplianceVersionID != "v-old" {
		t.Fatalf("expected the historical compliance version, got %q", snap.ComplianceVersionID)
	}
	if rateFor(snap.Rates, contracts.TaxComponentCGST) != 250 {
		t.Fatalf("expected the historical rate 250bps, got %d", rateFor(snap.Rates, contracts.TaxComponentCGST))
	}

	current, err := BuildTaxSnapshots(versions, profiles, rules, outlet, lines, parseTime("2026-06-01T00:00:00Z"))
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if rateFor(current["p1"].Rates, contracts.TaxComponentCGST) != 900 {
		t.Fatalf("expected the current rate 900bps, got %d", rateFor(current["p1"].Rates, contracts.TaxComponentCGST))
	}

	// A reprint of the OLD bill after the rate changed must still show the
	// original rules (§31's reproducibility requirement) — this is the same
	// call with the same historical timestamp, unaffected by "now".
	reprint, err := BuildTaxSnapshots(versions, profiles, rules, outlet, lines, parseTime("2025-06-01T00:00:00Z"))
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if rateFor(reprint["p1"].Rates, contracts.TaxComponentCGST) != 250 {
		t.Fatalf("a reprint of a historical bill must show the rate that produced it, got %d",
			rateFor(reprint["p1"].Rates, contracts.TaxComponentCGST))
	}
}

// TestBuildTaxSnapshots_CapturesEveryProfileOnMixedRateInvoice is the check
// the orchestrator asked for directly: since 0.4.2 lets different lines
// resolve to different tax profiles, the snapshot an invoice stores must
// name EVERY profile actually used, not just one — otherwise a mixed-rate
// bill is "reproducible" in name only, and a reprint after a rate change on
// the profile that wasn't captured would silently show the wrong rules.
func TestBuildTaxSnapshots_CapturesEveryProfileOnMixedRateInvoice(t *testing.T) {
	outlet := "outlet-1"
	at := parseTime("2026-06-01T00:00:00Z")
	versions := []contracts.ComplianceVersion{
		{ID: "v1", OutletID: outlet, Label: "FY26 rates", EffectiveFrom: parseTime("2026-01-01T00:00:00Z")},
	}
	profiles := []contracts.TaxProfile{
		{ID: "p-food", OutletID: outlet, Code: "GST_5_FOOD", PricingMode: contracts.PricingModeExclusive, IsDefault: true, IsActive: true},
		{ID: "p-liquor", OutletID: outlet, Code: "GST_18_LIQUOR", PricingMode: contracts.PricingModeExclusive, IsActive: true},
		{ID: "p-cess", OutletID: outlet, Code: "GST_12_CESS", PricingMode: contracts.PricingModeExclusive, IsActive: true},
	}
	rules := []contracts.TaxRule{
		{ID: "r1", TaxProfileID: "p-food", ComplianceVersionID: "v1", Component: contracts.TaxComponentCGST, RateBps: 250, EffectiveFrom: parseTime("2026-01-01T00:00:00Z")},
		{ID: "r2", TaxProfileID: "p-food", ComplianceVersionID: "v1", Component: contracts.TaxComponentSGST, RateBps: 250, EffectiveFrom: parseTime("2026-01-01T00:00:00Z")},
		{ID: "r3", TaxProfileID: "p-liquor", ComplianceVersionID: "v1", Component: contracts.TaxComponentCGST, RateBps: 900, EffectiveFrom: parseTime("2026-01-01T00:00:00Z")},
		{ID: "r4", TaxProfileID: "p-liquor", ComplianceVersionID: "v1", Component: contracts.TaxComponentSGST, RateBps: 900, EffectiveFrom: parseTime("2026-01-01T00:00:00Z")},
		{ID: "r5", TaxProfileID: "p-cess", ComplianceVersionID: "v1", Component: contracts.TaxComponentCGST, RateBps: 600, EffectiveFrom: parseTime("2026-01-01T00:00:00Z")},
		{ID: "r6", TaxProfileID: "p-cess", ComplianceVersionID: "v1", Component: contracts.TaxComponentSGST, RateBps: 600, EffectiveFrom: parseTime("2026-01-01T00:00:00Z")},
		{ID: "r7", TaxProfileID: "p-cess", ComplianceVersionID: "v1", Component: contracts.TaxComponentCess, RateBps: 280, EffectiveFrom: parseTime("2026-01-01T00:00:00Z")},
	}
	// A real mixed bill: a food line, a liquor line, and a cess-bearing line.
	lines := []Line{
		{OrderItemID: "food-item", TaxProfileID: "p-food"},
		{OrderItemID: "liquor-item", TaxProfileID: "p-liquor"},
		{OrderItemID: "sweet-beverage", TaxProfileID: "p-cess"},
	}

	snapshots, err := BuildTaxSnapshots(versions, profiles, rules, outlet, lines, at)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(snapshots) != 3 {
		t.Fatalf("expected a snapshot for all 3 profiles used on the invoice, got %d: %+v", len(snapshots), snapshots)
	}
	if rateFor(snapshots["p-food"].Rates, contracts.TaxComponentCGST) != 250 {
		t.Fatalf("food profile snapshot has the wrong CGST rate: %+v", snapshots["p-food"])
	}
	if rateFor(snapshots["p-liquor"].Rates, contracts.TaxComponentCGST) != 900 {
		t.Fatalf("liquor profile snapshot has the wrong CGST rate: %+v", snapshots["p-liquor"])
	}
	if rateFor(snapshots["p-cess"].Rates, contracts.TaxComponentCess) != 280 {
		t.Fatalf("cess profile snapshot is missing its cess rate: %+v", snapshots["p-cess"])
	}

	// Rendered to the invoice's wire shape (map[string]interface{}), all
	// three profiles must still be present as distinct keys.
	rendered, err := RenderTaxSnapshots(snapshots)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	for _, id := range []string{"p-food", "p-liquor", "p-cess"} {
		entry, ok := rendered[id].(map[string]interface{})
		if !ok {
			t.Fatalf("rendered tax_snapshot is missing profile %q entirely: %+v", id, rendered)
		}
		if entry["tax_profile_id"] != id {
			t.Fatalf("rendered snapshot for %q carries the wrong tax_profile_id: %v", id, entry["tax_profile_id"])
		}
	}
}

func TestTaxSnapshot_ToMap_RendersWireShape(t *testing.T) {
	snap := TaxSnapshot{
		ComplianceVersionID:    "v1",
		ComplianceVersionLabel: "FY26 rates",
		TaxProfileID:           "p1",
		TaxProfileCode:         "GST_5_RESTAURANT",
		PricingMode:            contracts.PricingModeExclusive,
		Rates:                  []ResolvedRate{{Component: contracts.TaxComponentCGST, RateBps: 250}},
		ResolvedAt:             time.Date(2026, 6, 1, 12, 0, 0, 0, time.UTC),
	}
	m, err := snap.ToMap()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if m["compliance_version_id"] != "v1" {
		t.Fatalf("expected snake_case wire field compliance_version_id, got %v", m)
	}
	if m["tax_profile_code"] != "GST_5_RESTAURANT" {
		t.Fatalf("expected tax_profile_code in map, got %v", m)
	}
}
