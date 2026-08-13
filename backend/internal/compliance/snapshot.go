package compliance

import (
	"encoding/json"
	"time"

	contracts "github.com/holler/contracts"
)

// Task 4: the tax_snapshot an invoice stores. §31 requires a historical bill
// stay reproducible after the rules change, which is only possible if the
// bill records which ruleset produced it — not just the numbers that fell
// out of applying it. This is deliberately more than the totals: it names
// the compliance version, the profile and every resolved rate, so a reprint
// six months later shows the original rules verbatim even if the outlet's
// current rates have since moved on.
type TaxSnapshot struct {
	ComplianceVersionID    string                `json:"compliance_version_id"`
	ComplianceVersionLabel string                `json:"compliance_version_label"`
	TaxProfileID           string                `json:"tax_profile_id"`
	TaxProfileCode         string                `json:"tax_profile_code"`
	PricingMode            contracts.PricingMode `json:"pricing_mode"`
	Rates                  []ResolvedRate        `json:"rates"`
	ResolvedAt             time.Time             `json:"resolved_at"`
}

// BuildTaxSnapshot assembles the snapshot for one profile/version/rate-set
// resolution. An invoice whose lines span more than one tax profile records
// one TaxSnapshot per distinct profile via BuildTaxSnapshots below.
func BuildTaxSnapshot(version contracts.ComplianceVersion, profile contracts.TaxProfile, rates []ResolvedRate, resolvedAt time.Time) TaxSnapshot {
	return TaxSnapshot{
		ComplianceVersionID:    version.ID,
		ComplianceVersionLabel: version.Label,
		TaxProfileID:           profile.ID,
		TaxProfileCode:         profile.Code,
		PricingMode:            profile.PricingMode,
		Rates:                  rates,
		ResolvedAt:             resolvedAt.UTC(),
	}
}

// ToMap renders the snapshot as the map[string]interface{} shape
// packages/contracts/go/invoice.go's Invoice.TaxSnapshot requires, via a
// JSON round-trip so the wire shape (field names, time formatting) matches
// exactly what a JSON client would see rather than Go's zero-value struct
// representation.
func (s TaxSnapshot) ToMap() (map[string]interface{}, error) {
	raw, err := json.Marshal(s)
	if err != nil {
		return nil, err
	}
	var out map[string]interface{}
	if err := json.Unmarshal(raw, &out); err != nil {
		return nil, err
	}
	return out, nil
}

// BuildTaxSnapshots resolves and renders one TaxSnapshot per distinct
// tax_profile_id used across lines, keyed by TaxProfileID so the invoice
// track can look one up per line when assembling the final invoice. Every
// profile referenced by a line must resolve or this returns an error naming
// which one failed.
func BuildTaxSnapshots(
	versions []contracts.ComplianceVersion,
	profiles []contracts.TaxProfile,
	rules []contracts.TaxRule,
	outletID string,
	lines []Line,
	at time.Time,
) (map[string]TaxSnapshot, error) {
	version, err := ResolveComplianceVersion(versions, outletID, at)
	if err != nil {
		return nil, err
	}

	profileByID := make(map[string]contracts.TaxProfile, len(profiles))
	for _, p := range profiles {
		profileByID[p.ID] = p
	}

	out := make(map[string]TaxSnapshot)
	for _, line := range lines {
		if _, ok := out[line.TaxProfileID]; ok {
			continue
		}
		profile, ok := profileByID[line.TaxProfileID]
		if !ok {
			continue
		}
		rates, err := ResolveRates(rules, profile.ID, version.ID, at)
		if err != nil {
			return nil, err
		}
		out[line.TaxProfileID] = BuildTaxSnapshot(version, profile, rates, at)
	}
	return out, nil
}
