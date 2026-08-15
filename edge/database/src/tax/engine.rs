//! Port of `backend/internal/compliance/engine.go` — Task 3/4: compute tax
//! over a set of lines per ADR-016 §3, supporting both tax-inclusive and
//! tax-exclusive pricing. This is a line-for-line translation of the Go
//! engine, not a re-derivation, because cross-engine parity (this module's
//! whole purpose) depends on both engines taking the exact same arithmetic
//! path, not merely producing the same answer on the cases someone thought
//! to test.
//!
//! The rounding policy, verbatim from ADR-016 §3:
//!   - tax is computed per line at full paise precision,
//!   - summed per component (CGST/SGST/IGST/CESS) across the invoice,
//!   - each component rounded half-up to paise exactly once,
//!   - the grand total is then rounded to the nearest rupee, with the delta
//!     recorded in `round_off_paise`.
//!
//! # Reconciliation rule
//!
//! A printed invoice's line components must sum to its own printed
//! invoice-level components, always, exactly. The two pricing modes need
//! genuinely different per-line arithmetic, so the invoice-level totals are
//! COMPOSED from the same numbers the lines display, per pricing mode,
//! rather than computed independently and hoped to agree:
//!
//!  1. INCLUSIVE lines: the line's own total tax is EXACT by construction —
//!     net minus the (once, half-up rounded) taxable value, with no further
//!     rounding possible. That exact integer is distributed across the
//!     line's own components via `largest_remainder_split`, which is
//!     total-preserving.
//!  2. EXCLUSIVE lines have no such per-line identity to preserve (tax is
//!     added on top of an already-exact taxable value), so they absorb the
//!     ADR-016 §3 "round once, at the invoice level" step: the raw
//!     `(taxable * rate_bps)` contribution is summed across every EXCLUSIVE
//!     line and rounded half-up exactly once per component, and that
//!     rounded integer is then allocated back across the EXCLUSIVE lines via
//!     `largest_remainder_split` (weighted by each line's raw contribution).
//!  3. The invoice's own component total is DEFINED as the sum of the two,
//!     so `Σ(line components) == invoice component` is an algebraic
//!     identity, not a coincidence.

use crate::error::{DbError, DbResult};

use super::domain::{
    component_value, rate_for, set_component_value, sum_rate_bps, Line, LineComputation,
    PricingMode, COMPONENT_ORDER,
};
use super::rounding::{
    largest_remainder_split, round_component_paise, round_to_nearest_rupee, BPS_DENOMINATOR,
};
use super::InvoiceTotals;

pub fn compute_invoice(lines: &[Line]) -> DbResult<(Vec<LineComputation>, InvoiceTotals)> {
    let mut computed: Vec<LineComputation> = Vec::with_capacity(lines.len());

    let mut subtotal = 0i64;
    let mut discount = 0i64;
    let mut taxable = 0i64;
    // Raw (paise * rate_bps) sums, EXCLUSIVE lines only — rounded once, at
    // invoice level, per ADR-016 §3.
    let mut exclusive_scaled = [0i64; 4];
    // Exact sums, INCLUSIVE lines only — already integer paise, no further
    // rounding: summing exact numbers cannot accumulate rounding error.
    let mut inclusive_sum = [0i64; 4];

    let mut exclusive_idx: Vec<usize> = Vec::new();
    let mut exclusive_weights: Vec<[i64; 4]> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let mut lc =
            compute_line_base(line).map_err(|e| DbError::InvalidInput(format!("line {i}: {e}")))?;

        subtotal += lc.gross_paise;
        discount += lc.discount_paise;
        taxable += lc.taxable_value_paise;

        match line.pricing_mode {
            PricingMode::Inclusive => {
                finish_inclusive_line(&mut lc, line);
                for (c, component) in COMPONENT_ORDER.iter().enumerate() {
                    inclusive_sum[c] += component_value(&lc, *component);
                }
            }
            PricingMode::Exclusive => {
                let mut weights = [0i64; 4];
                let t = lc.taxable_value_paise;
                for (c, component) in COMPONENT_ORDER.iter().enumerate() {
                    let w = t * rate_for(&line.rates, *component);
                    weights[c] = w;
                    exclusive_scaled[c] += w;
                }
                exclusive_idx.push(i);
                exclusive_weights.push(weights);
            }
        }
        computed.push(lc);
    }

    let mut exclusive_rounded = [0i64; 4];
    for c in 0..4 {
        exclusive_rounded[c] = round_component_paise(exclusive_scaled[c]);
    }

    // Allocate each EXCLUSIVE component's single invoice-level rounding back
    // across the EXCLUSIVE lines that contributed to it, via
    // largest_remainder_split (total-preserving by construction).
    for c in 0..4 {
        let weights: Vec<i64> = exclusive_weights.iter().map(|w| w[c]).collect();
        let shares = largest_remainder_split(exclusive_rounded[c], &weights);
        for (k, &line_idx) in exclusive_idx.iter().enumerate() {
            set_component_value(&mut computed[line_idx], COMPONENT_ORDER[c], shares[k]);
        }
    }
    for &line_idx in &exclusive_idx {
        let lc = &mut computed[line_idx];
        lc.total_paise =
            lc.taxable_value_paise + lc.cgst_paise + lc.sgst_paise + lc.igst_paise + lc.cess_paise;
    }

    let cgst = inclusive_sum[0] + exclusive_rounded[0];
    let sgst = inclusive_sum[1] + exclusive_rounded[1];
    let igst = inclusive_sum[2] + exclusive_rounded[2];
    let cess = inclusive_sum[3] + exclusive_rounded[3];

    let pre_round = taxable + cgst + sgst + igst + cess;
    let grand_total = round_to_nearest_rupee(pre_round);
    let round_off = grand_total - pre_round;

    let totals = InvoiceTotals {
        subtotal_paise: subtotal,
        discount_paise: discount,
        taxable_value_paise: taxable,
        cgst_paise: cgst,
        sgst_paise: sgst,
        igst_paise: igst,
        cess_paise: cess,
        round_off_paise: round_off,
        grand_total_paise: grand_total,
    };
    Ok((computed, totals))
}

/// Validates a line and computes the fields common to both pricing modes:
/// gross/discount/taxable-value and the `rate_bps` carried per component for
/// display. Tax component amounts are filled in afterward by
/// `finish_inclusive_line` (immediately) or by `compute_invoice`'s
/// cross-line allocation (for EXCLUSIVE lines, once every line has been
/// seen).
fn compute_line_base(line: &Line) -> DbResult<LineComputation> {
    if line.quantity <= 0 {
        return Err(DbError::InvalidInput("quantity must be positive".into()));
    }
    if line.unit_price_paise < 0 {
        return Err(DbError::InvalidInput(
            "unit_price_paise must not be negative".into(),
        ));
    }
    if line.discount_per_unit_paise < 0 {
        return Err(DbError::InvalidInput(
            "discount_per_unit_paise must not be negative".into(),
        ));
    }
    if line.discount_per_unit_paise > line.unit_price_paise {
        return Err(DbError::InvalidInput(
            "discount_per_unit_paise must not exceed unit_price_paise".into(),
        ));
    }
    for r in &line.rates {
        if r.rate_bps < 0 {
            return Err(DbError::InvalidInput(
                "rate_bps must not be negative".into(),
            ));
        }
    }

    let gross = line.unit_price_paise * line.quantity;
    let line_discount = line.discount_per_unit_paise * line.quantity;
    let net = gross - line_discount; // always >= 0: discount_per_unit <= unit_price, checked above

    let rate_sum = sum_rate_bps(&line.rates);

    let taxable_value = match line.pricing_mode {
        PricingMode::Exclusive => {
            // Tax added on top of net: taxable value IS net, exactly (no
            // division, so no rounding is even possible here).
            net
        }
        PricingMode::Inclusive => {
            // Tax is embedded in net; back-compute the taxable value once
            // (half-up). The remainder (net - taxable_value) is exact and is
            // finalized by finish_inclusive_line.
            let denominator = BPS_DENOMINATOR + rate_sum;
            if denominator <= 0 {
                return Err(DbError::InvalidInput(
                    "total rate must be greater than -100%".into(),
                ));
            }
            super::rounding::round_half_up_div(net * BPS_DENOMINATOR, denominator)
        }
    };

    Ok(LineComputation {
        order_item_id: line.order_item_id.clone(),
        description: line.description.clone(),
        hsn_sac: line.hsn_sac.clone(),
        quantity: line.quantity,
        unit_price_paise: line.unit_price_paise,
        gross_paise: gross,
        discount_paise: line_discount,
        taxable_value_paise: taxable_value,
        tax_profile_id: line.tax_profile_id.clone(),
        cgst_rate_bps: rate_for(&line.rates, super::domain::TaxComponent::Cgst),
        cgst_paise: 0,
        sgst_rate_bps: rate_for(&line.rates, super::domain::TaxComponent::Sgst),
        sgst_paise: 0,
        igst_rate_bps: rate_for(&line.rates, super::domain::TaxComponent::Igst),
        igst_paise: 0,
        cess_rate_bps: rate_for(&line.rates, super::domain::TaxComponent::Cess),
        cess_paise: 0,
        total_paise: 0,
    })
}

/// Fills in an INCLUSIVE line's own tax components. The line's total tax
/// (`net - taxable_value`) is EXACT — no rounding occurs here, only a
/// total-preserving distribution across the line's own components — which
/// is what guarantees `total_paise` reconstructs `gross - discount` to the
/// paise (Task 4), independent of every other line on the invoice.
fn finish_inclusive_line(lc: &mut LineComputation, line: &Line) {
    let net = lc.gross_paise - lc.discount_paise;
    let line_tax_total = net - lc.taxable_value_paise;

    let weights: Vec<i64> = COMPONENT_ORDER
        .iter()
        .map(|c| rate_for(&line.rates, *c))
        .collect();
    let shares = largest_remainder_split(line_tax_total, &weights);
    lc.cgst_paise = shares[0];
    lc.sgst_paise = shares[1];
    lc.igst_paise = shares[2];
    lc.cess_paise = shares[3];
    lc.total_paise =
        lc.taxable_value_paise + lc.cgst_paise + lc.sgst_paise + lc.igst_paise + lc.cess_paise;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tax::domain::{PricingMode, ResolvedRate, TaxComponent};

    fn gst_restaurant_5() -> Vec<ResolvedRate> {
        vec![
            ResolvedRate {
                component: TaxComponent::Cgst,
                rate_bps: 250,
            },
            ResolvedRate {
                component: TaxComponent::Sgst,
                rate_bps: 250,
            },
        ]
    }

    #[test]
    fn exclusive_pricing_worked_example() {
        let lines = vec![
            Line {
                order_item_id: "item-1".into(),
                description: "Butter Chicken".into(),
                hsn_sac: None,
                quantity: 2,
                unit_price_paise: 32000,
                discount_per_unit_paise: 0,
                tax_profile_id: "gst5".into(),
                pricing_mode: PricingMode::Exclusive,
                rates: gst_restaurant_5(),
            },
            Line {
                order_item_id: "item-2".into(),
                description: "Coke".into(),
                hsn_sac: None,
                quantity: 1,
                unit_price_paise: 6000,
                discount_per_unit_paise: 0,
                tax_profile_id: "gst5".into(),
                pricing_mode: PricingMode::Exclusive,
                rates: gst_restaurant_5(),
            },
            Line {
                order_item_id: "item-3".into(),
                description: "Sweetened Beverage".into(),
                hsn_sac: None,
                quantity: 1,
                unit_price_paise: 5000,
                discount_per_unit_paise: 0,
                tax_profile_id: "gst12cess".into(),
                pricing_mode: PricingMode::Exclusive,
                rates: vec![
                    ResolvedRate {
                        component: TaxComponent::Cgst,
                        rate_bps: 600,
                    },
                    ResolvedRate {
                        component: TaxComponent::Sgst,
                        rate_bps: 600,
                    },
                    ResolvedRate {
                        component: TaxComponent::Cess,
                        rate_bps: 280,
                    },
                ],
            },
        ];

        let (lcs, totals) = compute_invoice(&lines).expect("compute");
        assert_eq!(lcs.len(), 3);
        assert_eq!(lcs[0].taxable_value_paise, 64000);
        assert_eq!(lcs[2].total_paise, 5000 + 600 + 140);
        assert_eq!(totals.taxable_value_paise, 75000);
        assert_eq!(totals.cgst_paise, 2050);
        assert_eq!(totals.sgst_paise, 2050);
        assert_eq!(totals.cess_paise, 140);
        let pre_round = totals.taxable_value_paise
            + totals.cgst_paise
            + totals.sgst_paise
            + totals.igst_paise
            + totals.cess_paise;
        assert_eq!(pre_round, 79240);
        assert_eq!(totals.grand_total_paise, 79200);
        assert_eq!(totals.round_off_paise, -40);
    }

    #[test]
    fn inclusive_pricing_reconstructs_gross_exactly() {
        let lines = vec![
            Line {
                order_item_id: "item-1".into(),
                description: "Thali".into(),
                hsn_sac: None,
                quantity: 1,
                unit_price_paise: 10500,
                discount_per_unit_paise: 0,
                tax_profile_id: "gst5".into(),
                pricing_mode: PricingMode::Inclusive,
                rates: gst_restaurant_5(),
            },
            Line {
                order_item_id: "item-2".into(),
                description: "Lassi".into(),
                hsn_sac: None,
                quantity: 3,
                unit_price_paise: 9900,
                discount_per_unit_paise: 0,
                tax_profile_id: "gst5".into(),
                pricing_mode: PricingMode::Inclusive,
                rates: gst_restaurant_5(),
            },
        ];

        let (lcs, _) = compute_invoice(&lines).expect("compute");
        for lc in &lcs {
            let gross = lc.gross_paise - lc.discount_paise;
            let reconstructed = lc.taxable_value_paise
                + lc.cgst_paise
                + lc.sgst_paise
                + lc.igst_paise
                + lc.cess_paise;
            assert_eq!(reconstructed, gross);
            assert_eq!(lc.total_paise, gross);
        }
        assert_eq!(lcs[0].taxable_value_paise, 10000);
    }

    #[test]
    fn rejects_invalid_lines() {
        let bad = vec![
            Line {
                order_item_id: "x".into(),
                description: String::new(),
                hsn_sac: None,
                quantity: 0,
                unit_price_paise: 100,
                discount_per_unit_paise: 0,
                tax_profile_id: String::new(),
                pricing_mode: PricingMode::Exclusive,
                rates: vec![],
            },
            Line {
                order_item_id: "x".into(),
                description: String::new(),
                hsn_sac: None,
                quantity: 1,
                unit_price_paise: -1,
                discount_per_unit_paise: 0,
                tax_profile_id: String::new(),
                pricing_mode: PricingMode::Exclusive,
                rates: vec![],
            },
            Line {
                order_item_id: "x".into(),
                description: String::new(),
                hsn_sac: None,
                quantity: 1,
                unit_price_paise: 100,
                discount_per_unit_paise: 200,
                tax_profile_id: String::new(),
                pricing_mode: PricingMode::Exclusive,
                rates: vec![],
            },
        ];
        for line in bad {
            assert!(compute_invoice(&[line]).is_err());
        }
    }
}
