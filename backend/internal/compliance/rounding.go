// Package compliance is the cloud-side tax engine (Milestone 3, ADR-016,
// docs/spec/compliance.md). §31: "do NOT scatter tax percentages throughout
// the application" — every rate resolves through a TaxProfile, and this
// package is the one place the arithmetic that turns a rate into money
// lives.
//
// This is the REFERENCE implementation and CONFIG authority (tax_profile,
// tax_rule, compliance_version are CLOUD_TO_EDGE per ADR-016 §1). The
// Invoice that USES these rules is edge-authoritative (§50.1) — the edge
// bills a customer with the uplink down, and a parallel track builds an
// edge engine that must agree with this one exactly. The outputs here are
// the fixtures that edge engine is tested against.
//
// No floating point anywhere, at any intermediate step (CLAUDE.md forbids it
// for money, and a rate that multiplies money inherits the rule). Every
// function in this file operates on int64 paise/basis-point arithmetic only.
package compliance

// bpsDenominator is the basis-point scale: 2.5% = 250 basis points, so a rate
// is applied to an amount by multiplying by rateBps and dividing by 10000.
// Named rather than a bare 10000 littering every call site (CLAUDE.md "no
// magic numbers").
const bpsDenominator = 10000

// paiseToRupeeDenominator is the whole-rupee scale used for the invoice's
// final round-to-the-rupee step (ADR-016 §3): 100 paise = ₹1.
const paiseToRupeeDenominator = 100

// roundHalfUpDiv divides numerator by denominator and rounds the quotient
// half-up: a remainder that is at least half the denominator rounds up,
// otherwise down. ADR-016 §3 pins half-up deliberately — banker's rounding
// disagrees with it on exactly the ₹x.x5 cases a menu produces constantly.
//
// Requires denominator > 0 and numerator >= 0; every caller in this package
// satisfies that (rates and paise amounts are never negative here — a
// discount is subtracted before this is ever called, not represented as a
// negative rate or amount).
func roundHalfUpDiv(numerator, denominator int64) int64 {
	if denominator <= 0 {
		panic("compliance: roundHalfUpDiv requires a positive denominator")
	}
	if numerator < 0 {
		panic("compliance: roundHalfUpDiv requires a non-negative numerator")
	}
	quotient := numerator / denominator
	remainder := numerator % denominator
	if 2*remainder >= denominator {
		quotient++
	}
	return quotient
}

// roundComponentPaise rounds a component's accumulated (paise * rate_bps)
// total to paise, half-up, exactly once — the ADR-016 §3 invoice-level step.
func roundComponentPaise(scaledPaise int64) int64 {
	return roundHalfUpDiv(scaledPaise, bpsDenominator)
}

// roundToNearestRupee rounds a paise amount to the nearest 100 paise,
// half-up — the grand-total step of ADR-016 §3.
func roundToNearestRupee(amountPaise int64) int64 {
	return roundHalfUpDiv(amountPaise, paiseToRupeeDenominator) * paiseToRupeeDenominator
}

// largestRemainderSplit distributes total (an exact integer amount, e.g. one
// line's combined tax) across len(weights) buckets proportionally to weights
// (e.g. each component's rate_bps), such that the buckets sum to EXACTLY
// total — no paise lost or gained, which is the property Task 3 (inclusive
// pricing) requires of a per-line split.
//
// Method: floor-divide each bucket, then hand the leftover paise (there are
// at most len(weights)-1 of them) to the buckets with the largest fractional
// remainder, ties broken by bucket index for determinism. A weight of zero
// always receives zero.
func largestRemainderSplit(total int64, weights []int64) []int64 {
	out := make([]int64, len(weights))
	if total == 0 || len(weights) == 0 {
		return out
	}
	var weightSum int64
	for _, w := range weights {
		weightSum += w
	}
	if weightSum == 0 {
		// No positive weight to distribute against (e.g. a zero-rated line);
		// the entire total lands on the first bucket rather than being lost.
		out[0] = total
		return out
	}

	type remainder struct {
		index int
		frac  int64
	}
	remainders := make([]remainder, len(weights))
	var allocated int64
	for i, w := range weights {
		scaled := total * w
		out[i] = scaled / weightSum
		remainders[i] = remainder{index: i, frac: scaled % weightSum}
		allocated += out[i]
	}

	leftover := total - allocated
	// Stable selection of the largest remainders without sorting the whole
	// slice destructively: simple selection is fine at this size (at most 4
	// tax components per line).
	for leftover > 0 {
		bestIdx := -1
		var bestFrac int64 = -1
		for _, r := range remainders {
			if r.frac < 0 {
				continue // already claimed
			}
			if r.frac > bestFrac {
				bestFrac = r.frac
				bestIdx = r.index
			}
		}
		if bestIdx == -1 {
			// Should not happen: leftover is always < len(weights) by
			// construction of floor division, so there is always an
			// unclaimed bucket to receive it.
			break
		}
		out[bestIdx]++
		remainders[bestIdx].frac = -1
		leftover--
	}
	return out
}
