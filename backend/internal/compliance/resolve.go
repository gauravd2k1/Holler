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
// `at`, given itemTaxProfileID — the item's OWN pinned profile
// (contracts.MenuItem.TaxProfileID, 0.4.2). This is a pure function: the
// caller (whoever already loaded the MenuItem) passes its TaxProfileID
// straight through rather than this function looking an item up itself.
//
// Fallback is explicit and is the WHOLE function, deliberately not buried in
// a longer branch: nil means "use the outlet's default profile" (0.4.2's own
// wording), which is what keeps the common single-rate restaurant
// configuration-free — nothing to set on any item, every line resolves to
// the one outlet-wide profile with no per-item data at all.
//
// A NON-nil itemTaxProfileID that names a profile which doesn't resolve
// (wrong outlet, inactive, or simply absent from profiles) is a config
// error and returns one — it must never silently fall back to the outlet
// default, which would hide exactly the kind of misconfiguration a mixed-
// rate menu (e.g. a liquor item pinned to the wrong profile) most needs
// surfaced.
//
// itemTaxProfileID is resolution INPUT only. The result feeds a Line's
// TaxProfileID, which computeLineBase/finishInclusiveLine snapshot into
// LineComputation/InvoiceLine at billing time — re-pointing the menu item to
// a different profile tomorrow never touches a bill already issued today
// (§31; see reproducibility_test.go's pinning test).
func ResolveTaxProfile(profiles []contracts.TaxProfile, outletID string, itemTaxProfileID *string, at time.Time) (contracts.TaxProfile, error) {
	_ = at // TaxProfile itself is not effective-dated; its TaxRules are.

	if itemTaxProfileID != nil {
		for _, p := range profiles {
			if p.ID == *itemTaxProfileID && p.OutletID == outletID && p.IsActive {
				return p, nil
			}
		}
		return contracts.TaxProfile{}, fmt.Errorf(
			"%w: item's tax_profile_id %s is not an active tax profile for outlet %s",
			httpx.ErrInvalidInput, *itemTaxProfileID, outletID)
	}

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
