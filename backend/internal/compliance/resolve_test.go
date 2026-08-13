package compliance

import (
	"testing"
	"time"

	contracts "github.com/holler/contracts"
)

func parseTime(s string) time.Time {
	parsed, err := time.Parse(time.RFC3339, s)
	if err != nil {
		panic(err)
	}
	return parsed
}

func TestResolveComplianceVersion_PastInstantReturnsPastRuleset(t2 *testing.T) {
	outlet := "outlet-1"
	versions := []contracts.ComplianceVersion{
		{ID: "v1", OutletID: outlet, Label: "pre-rate-change", EffectiveFrom: parseTime("2025-01-01T00:00:00Z")},
		{ID: "v2", OutletID: outlet, Label: "post-rate-change", EffectiveFrom: parseTime("2026-04-01T00:00:00Z")},
		{ID: "v-other-outlet", OutletID: "outlet-2", Label: "irrelevant", EffectiveFrom: parseTime("2025-06-01T00:00:00Z")},
	}

	got, err := ResolveComplianceVersion(versions, outlet, parseTime("2025-06-01T00:00:00Z"))
	if err != nil {
		t2.Fatalf("unexpected error: %v", err)
	}
	if got.ID != "v1" {
		t2.Fatalf("resolving at a past instant should return the ruleset live then, got %q", got.ID)
	}

	got, err = ResolveComplianceVersion(versions, outlet, parseTime("2026-06-01T00:00:00Z"))
	if err != nil {
		t2.Fatalf("unexpected error: %v", err)
	}
	if got.ID != "v2" {
		t2.Fatalf("resolving after the rate change should return the new ruleset, got %q", got.ID)
	}

	if _, err := ResolveComplianceVersion(versions, outlet, parseTime("2024-01-01T00:00:00Z")); err == nil {
		t2.Fatalf("resolving before any version existed should error")
	}
}

// TestResolveComplianceVersion_EffectiveFromBoundaryIsInclusive covers
// Defect 3: at == EffectiveFrom must resolve to that version (the version
// takes effect AT that instant, not strictly after it).
func TestResolveComplianceVersion_EffectiveFromBoundaryIsInclusive(t2 *testing.T) {
	outlet := "outlet-1"
	changeInstant := parseTime("2026-04-01T00:00:00Z")
	versions := []contracts.ComplianceVersion{
		{ID: "v1", OutletID: outlet, Label: "pre-rate-change", EffectiveFrom: parseTime("2025-01-01T00:00:00Z")},
		{ID: "v2", OutletID: outlet, Label: "post-rate-change", EffectiveFrom: changeInstant},
	}

	got, err := ResolveComplianceVersion(versions, outlet, changeInstant)
	if err != nil {
		t2.Fatalf("unexpected error: %v", err)
	}
	if got.ID != "v2" {
		t2.Fatalf("at == EffectiveFrom must resolve to the NEW version (inclusive boundary), got %q", got.ID)
	}

	justBefore := changeInstant.Add(-time.Nanosecond)
	got, err = ResolveComplianceVersion(versions, outlet, justBefore)
	if err != nil {
		t2.Fatalf("unexpected error: %v", err)
	}
	if got.ID != "v1" {
		t2.Fatalf("one nanosecond before EffectiveFrom must still resolve to the OLD version, got %q", got.ID)
	}
}

// TestResolveRates_EffectiveFromAndEffectiveToBoundaries covers Defect 3 for
// TaxRule: at == EffectiveFrom is inclusive (the new rate applies from that
// instant), at == EffectiveTo is exclusive (the old rate no longer applies
// at that instant — EffectiveTo is "up to but not including").
func TestResolveRates_EffectiveFromAndEffectiveToBoundaries(t2 *testing.T) {
	profileID, versionID := "profile-1", "version-1"
	changeInstant := parseTime("2026-01-01T00:00:00Z")
	rules := []contracts.TaxRule{
		{ID: "r1", TaxProfileID: profileID, ComplianceVersionID: versionID, Component: contracts.TaxComponentCGST,
			RateBps: 250, EffectiveFrom: parseTime("2025-01-01T00:00:00Z"), EffectiveTo: timeptr(changeInstant)},
		{ID: "r2", TaxProfileID: profileID, ComplianceVersionID: versionID, Component: contracts.TaxComponentCGST,
			RateBps: 900, EffectiveFrom: changeInstant},
	}

	// at == EffectiveTo of r1 == EffectiveFrom of r2: EffectiveTo is
	// exclusive, so r1 must NOT apply and r2 (EffectiveFrom inclusive) must.
	atBoundary, err := ResolveRates(rules, profileID, versionID, changeInstant)
	if err != nil {
		t2.Fatalf("unexpected error: %v", err)
	}
	if rateFor(atBoundary, contracts.TaxComponentCGST) != 900 {
		t2.Fatalf("at == EffectiveTo/EffectiveFrom boundary must resolve to the NEW rate 900bps, got %d",
			rateFor(atBoundary, contracts.TaxComponentCGST))
	}

	justBefore := changeInstant.Add(-time.Nanosecond)
	beforeBoundary, err := ResolveRates(rules, profileID, versionID, justBefore)
	if err != nil {
		t2.Fatalf("unexpected error: %v", err)
	}
	if rateFor(beforeBoundary, contracts.TaxComponentCGST) != 250 {
		t2.Fatalf("one nanosecond before the boundary must still resolve to the OLD rate 250bps, got %d",
			rateFor(beforeBoundary, contracts.TaxComponentCGST))
	}
}

func TestResolveRates_EffectiveDatedComponentChange(t2 *testing.T) {
	profileID, versionID := "profile-1", "version-1"
	rules := []contracts.TaxRule{
		{ID: "r1", TaxProfileID: profileID, ComplianceVersionID: versionID, Component: contracts.TaxComponentCGST,
			RateBps: 250, EffectiveFrom: parseTime("2025-01-01T00:00:00Z"), EffectiveTo: timeptr(parseTime("2026-01-01T00:00:00Z"))},
		{ID: "r2", TaxProfileID: profileID, ComplianceVersionID: versionID, Component: contracts.TaxComponentCGST,
			RateBps: 900, EffectiveFrom: parseTime("2026-01-01T00:00:00Z")},
		{ID: "r3", TaxProfileID: profileID, ComplianceVersionID: versionID, Component: contracts.TaxComponentSGST,
			RateBps: 250, EffectiveFrom: parseTime("2025-01-01T00:00:00Z")},
	}

	before, err := ResolveRates(rules, profileID, versionID, parseTime("2025-06-01T00:00:00Z"))
	if err != nil {
		t2.Fatalf("unexpected error: %v", err)
	}
	if rateFor(before, contracts.TaxComponentCGST) != 250 {
		t2.Fatalf("expected the pre-change CGST rate 250bps, got %d", rateFor(before, contracts.TaxComponentCGST))
	}

	after, err := ResolveRates(rules, profileID, versionID, parseTime("2026-06-01T00:00:00Z"))
	if err != nil {
		t2.Fatalf("unexpected error: %v", err)
	}
	if rateFor(after, contracts.TaxComponentCGST) != 900 {
		t2.Fatalf("expected the post-change CGST rate 900bps, got %d", rateFor(after, contracts.TaxComponentCGST))
	}
	if rateFor(after, contracts.TaxComponentSGST) != 250 {
		t2.Fatalf("SGST rate should be unaffected by the CGST-only change, got %d", rateFor(after, contracts.TaxComponentSGST))
	}

	if _, err := ResolveRates(rules, "unknown-profile", versionID, parseTime("2026-06-01T00:00:00Z")); err == nil {
		t2.Fatalf("resolving an unknown profile/version pair should error")
	}
}

func TestResolveTaxProfile_DefaultActiveOnly(t2 *testing.T) {
	outlet := "outlet-1"
	profiles := []contracts.TaxProfile{
		{ID: "p-inactive", OutletID: outlet, Code: "OLD", IsDefault: true, IsActive: false},
		{ID: "p-nondefault", OutletID: outlet, Code: "SPECIAL", IsDefault: false, IsActive: true},
		{ID: "p-default", OutletID: outlet, Code: "GST_5_RESTAURANT", IsDefault: true, IsActive: true},
	}
	got, err := ResolveTaxProfile(profiles, outlet, "any-item", parseTime("2026-01-01T00:00:00Z"))
	if err != nil {
		t2.Fatalf("unexpected error: %v", err)
	}
	if got.ID != "p-default" {
		t2.Fatalf("expected the active default profile, got %q", got.ID)
	}

	if _, err := ResolveTaxProfile(profiles, "outlet-with-no-profiles", "any-item", parseTime("2026-01-01T00:00:00Z")); err == nil {
		t2.Fatalf("expected an error when no active default profile exists")
	}
}

func timeptr(t time.Time) *time.Time { return &t }
