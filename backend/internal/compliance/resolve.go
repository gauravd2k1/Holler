package compliance

import (
	"fmt"
	"time"

	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

// Task 1: TaxProfile / TaxRule / ComplianceVersion resolution. Rules are
// effective-dated — resolution at a past instant must return what was true
// then, not what is true now. These functions are pure: they take slices
// already loaded (by a repository, or by a test) and never touch a database
// themselves, which is what makes them exhaustively unit-testable without an
// integration harness.

// ResolveComplianceVersion returns the ComplianceVersion effective for
// outletID at instant `at`: the version with the latest EffectiveFrom that
// is still <= at. §31 requires a historical bill stay reproducible after the
// rules change, which is only possible if "effective at a past instant"
// really does mean the past ruleset, not the current one.
func ResolveComplianceVersion(versions []contracts.ComplianceVersion, outletID string, at time.Time) (contracts.ComplianceVersion, error) {
	var best contracts.ComplianceVersion
	found := false
	for _, v := range versions {
		if v.OutletID != outletID {
			continue
		}
		if v.EffectiveFrom.After(at) {
			continue
		}
		if !found || v.EffectiveFrom.After(best.EffectiveFrom) {
			best = v
			found = true
		}
	}
	if !found {
		return contracts.ComplianceVersion{}, fmt.Errorf(
			"%w: no compliance version effective for outlet %s at %s",
			httpx.ErrInvalidInput, outletID, at.UTC().Format(time.RFC3339))
	}
	return best, nil
}

// ResolveTaxProfile returns the tax profile that applies for outletID at
// `at`. itemID is accepted for interface stability: the frozen contracts
// (packages/contracts/go/menu.go) carry no menu_item -> tax_profile mapping
// yet, so every item on an outlet currently resolves to that outlet's single
// default active profile. A later contracts change adding a per-item
// override would let this function branch on itemID without changing its
// signature or any caller.
func ResolveTaxProfile(profiles []contracts.TaxProfile, outletID, itemID string, at time.Time) (contracts.TaxProfile, error) {
	_ = itemID // reserved: see doc comment above
	_ = at     // TaxProfile itself is not effective-dated; its TaxRules are
	for _, p := range profiles {
		if p.OutletID == outletID && p.IsDefault && p.IsActive {
			return p, nil
		}
	}
	return contracts.TaxProfile{}, fmt.Errorf(
		"%w: no active default tax profile for outlet %s", httpx.ErrInvalidInput, outletID)
}

// ResolveRates returns one ResolvedRate per component effective for
// profileID under complianceVersionID at instant `at`: for each component
// present in rules, the rule with the latest EffectiveFrom <= at whose
// EffectiveTo is nil or strictly after at.
//
// Returns an empty slice (not an error) if the profile+version combination
// carries no rules at all — an unusual but not invalid state (e.g. a
// zero-rated item) — but returns an error if profileID/complianceVersionID
// do not appear in rules together at all, since that combination is
// typically a caller mistake (wrong version pinned to a profile).
func ResolveRates(rules []contracts.TaxRule, profileID, complianceVersionID string, at time.Time) ([]ResolvedRate, error) {
	latest := map[contracts.TaxComponent]contracts.TaxRule{}
	anyForPair := false
	for _, r := range rules {
		if r.TaxProfileID != profileID || r.ComplianceVersionID != complianceVersionID {
			continue
		}
		anyForPair = true
		if r.EffectiveFrom.After(at) {
			continue
		}
		if r.EffectiveTo != nil && !r.EffectiveTo.After(at) {
			continue
		}
		current, ok := latest[r.Component]
		if !ok || r.EffectiveFrom.After(current.EffectiveFrom) {
			latest[r.Component] = r
		}
	}
	if !anyForPair {
		return nil, fmt.Errorf(
			"%w: no tax rules for profile %s under compliance version %s",
			httpx.ErrInvalidInput, profileID, complianceVersionID)
	}

	out := make([]ResolvedRate, 0, len(latest))
	for _, component := range componentOrder {
		if rule, ok := latest[component]; ok {
			out = append(out, ResolvedRate{Component: component, RateBps: rule.RateBps})
		}
	}
	return out, nil
}
