//! Port of `backend/internal/compliance/rounding.go`. See that file's doc
//! comments for the full reasoning; comments here are trimmed to what
//! differs in translation, not repeated verbatim.

/// Basis-point scale: 2.5% = 250 basis points, so a rate is applied to an
/// amount by multiplying by `rate_bps` and dividing by 10000.
pub const BPS_DENOMINATOR: i64 = 10_000;

/// Whole-rupee scale used for the invoice's final round-to-the-rupee step
/// (ADR-016 §3): 100 paise = ₹1.
pub const PAISE_PER_RUPEE: i64 = 100;

/// Divides `numerator` by `denominator` and rounds the quotient half-up: a
/// remainder that is at least half the denominator rounds up, otherwise
/// down. ADR-016 §3 pins half-up deliberately — banker's rounding disagrees
/// with it on exactly the ₹x.x5 cases a menu produces constantly.
///
/// Every caller in this module satisfies `denominator > 0` and
/// `numerator >= 0` by construction (rates and paise amounts are never
/// negative in this engine — a discount is subtracted before this is ever
/// called, not represented as a negative rate or amount), matching the exact
/// precondition `rounding.go`'s Go twin panics on. Asserting here rather
/// than threading a `Result` through every internal call site keeps the
/// arithmetic readable; the public entry point (`compute_invoice`) validates
/// every external input *before* any of these preconditions can be reached.
pub(super) fn round_half_up_div(numerator: i64, denominator: i64) -> i64 {
    assert!(
        denominator > 0,
        "tax: round_half_up_div requires a positive denominator"
    );
    assert!(
        numerator >= 0,
        "tax: round_half_up_div requires a non-negative numerator"
    );
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    if 2 * remainder >= denominator {
        quotient + 1
    } else {
        quotient
    }
}

/// Rounds a component's accumulated `(paise * rate_bps)` total to paise,
/// half-up, exactly once — the ADR-016 §3 invoice-level step.
pub(super) fn round_component_paise(scaled_paise: i64) -> i64 {
    round_half_up_div(scaled_paise, BPS_DENOMINATOR)
}

/// Rounds a paise amount to the nearest 100 paise, half-up — the grand-total
/// step of ADR-016 §3.
pub(super) fn round_to_nearest_rupee(amount_paise: i64) -> i64 {
    round_half_up_div(amount_paise, PAISE_PER_RUPEE) * PAISE_PER_RUPEE
}

/// Distributes `total` (an exact integer amount, e.g. one line's combined
/// tax) across `weights.len()` buckets proportionally to `weights` (e.g.
/// each component's `rate_bps`), such that the buckets sum to EXACTLY
/// `total` — no paise lost or gained, the property inclusive pricing
/// requires of a per-line split.
///
/// Method: floor-divide each bucket, then hand the leftover paise (there are
/// at most `weights.len() - 1` of them) to the buckets with the largest
/// fractional remainder, ties broken by bucket index for determinism. A
/// weight of zero always receives zero.
pub(super) fn largest_remainder_split(total: i64, weights: &[i64]) -> Vec<i64> {
    let mut out = vec![0i64; weights.len()];
    if total == 0 || weights.is_empty() {
        return out;
    }
    let weight_sum: i64 = weights.iter().sum();
    if weight_sum == 0 {
        // No positive weight to distribute against (e.g. a zero-rated line);
        // the entire total lands on the first bucket rather than being lost.
        out[0] = total;
        return out;
    }

    let mut fracs = vec![0i64; weights.len()];
    let mut allocated = 0i64;
    for (i, &w) in weights.iter().enumerate() {
        let scaled = total * w;
        out[i] = scaled / weight_sum;
        fracs[i] = scaled % weight_sum;
        allocated += out[i];
    }

    let mut leftover = total - allocated;
    let mut claimed = vec![false; weights.len()];
    while leftover > 0 {
        let mut best_idx: Option<usize> = None;
        let mut best_frac = -1i64;
        for (i, &frac) in fracs.iter().enumerate() {
            if claimed[i] {
                continue;
            }
            if frac > best_frac {
                best_frac = frac;
                best_idx = Some(i);
            }
        }
        let Some(idx) = best_idx else {
            // Should not happen: leftover is always < weights.len() by
            // construction of floor division, so there is always an
            // unclaimed bucket to receive it.
            break;
        };
        out[idx] += 1;
        claimed[idx] = true;
        leftover -= 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_up_rounds_exact_half_up() {
        assert_eq!(round_half_up_div(5, 10), 1); // 0.5 -> 1
        assert_eq!(round_half_up_div(4, 10), 0); // 0.4 -> 0
        assert_eq!(round_half_up_div(0, 10), 0);
    }

    #[test]
    fn round_to_nearest_rupee_rounds_half_up() {
        assert_eq!(round_to_nearest_rupee(79240), 79200); // remainder 40 < 50
        assert_eq!(round_to_nearest_rupee(79250), 79300); // remainder 50 == half, rounds up
        assert_eq!(round_to_nearest_rupee(79260), 79300);
    }

    #[test]
    fn largest_remainder_split_is_total_preserving() {
        let shares = largest_remainder_split(100, &[250, 250]);
        assert_eq!(shares.iter().sum::<i64>(), 100);
        assert_eq!(shares, vec![50, 50]);
    }

    #[test]
    fn largest_remainder_split_zero_weight_lands_on_first_bucket() {
        let shares = largest_remainder_split(37, &[0, 0]);
        assert_eq!(shares, vec![37, 0]);
    }

    #[test]
    fn largest_remainder_split_zero_total_is_all_zero() {
        let shares = largest_remainder_split(0, &[250, 250, 280]);
        assert_eq!(shares, vec![0, 0, 0]);
    }
}
