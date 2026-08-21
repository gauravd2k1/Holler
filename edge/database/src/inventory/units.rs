//! Two-tier unit conversion (ADR-018 §4).
//!
//! **Tier 1** is the frozen dimensional map — kg→g, l→ml, dozen→piece.
//! Physical constants, not configuration: mirrored byte-for-byte from
//! `DimensionalConversions` in `packages/contracts/go/inventory.go` and
//! `DIMENSIONAL_CONVERSIONS` in `packages/contracts/src/types/inventory.ts`.
//! No config write path exists for these, on purpose — giving them one
//! would only create a way to get physics wrong per tenant.
//!
//! **Tier 2** is `item_unit_conversion`: a per-item pack ratio
//! (`inventory_item_id`, `pack_unit_label`, `numerator`, `denominator`).
//! This is also where cross-dimension (density) conversion lives — oil is
//! bought in kg and cooked in ml, and density varies per ingredient, so
//! g↔ml is never a physical constant and has no place in Tier 1.
//!
//! Every quantity in this module is an integer count of MICRO-units of a
//! dimension's canonical unit (gram, litre, piece). No float, anywhere.

use super::rational::Rational;

/// Fixes what a stored `*_micro` value means. Mirrors the `dimension`
/// `CHECK` on `inventory_item` (`'MASS' | 'VOLUME' | 'COUNT'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dimension {
    Mass,
    Volume,
    Count,
}

impl Dimension {
    pub fn as_str(self) -> &'static str {
        match self {
            Dimension::Mass => "MASS",
            Dimension::Volume => "VOLUME",
            Dimension::Count => "COUNT",
        }
    }

    /// Parses the stored `inventory_item.dimension` / `item_unit_conversion
    /// .source_dimension` string. `None` on anything else — the schema's
    /// own `CHECK` should make that impossible, but this module never
    /// trusts a constraint that may have been written by an older schema
    /// version arriving over the wire (the same posture the resolver's
    /// cycle/depth backstop takes toward the cloud's write-time guard).
    pub fn parse(s: &str) -> Option<Dimension> {
        match s {
            "MASS" => Some(Dimension::Mass),
            "VOLUME" => Some(Dimension::Volume),
            "COUNT" => Some(Dimension::Count),
            _ => None,
        }
    }
}

/// One Tier 1 entry: `1` of the named unit equals `micro` micro-units of
/// its dimension's canonical unit.
#[derive(Debug, Clone, Copy)]
pub struct DimensionalConversion {
    pub dimension: Dimension,
    pub micro: i64,
}

/// THE FROZEN TIER 1 MAP. Keep in exact agreement with `DimensionalConversions`
/// (Go) and `DIMENSIONAL_CONVERSIONS` (TypeScript) — a value changed here
/// without changing both of those is a contract drift, not a bug fix.
pub const DIMENSIONAL_CONVERSIONS: &[(&str, DimensionalConversion)] = &[
    (
        "mg",
        DimensionalConversion {
            dimension: Dimension::Mass,
            micro: 1_000,
        },
    ),
    (
        "g",
        DimensionalConversion {
            dimension: Dimension::Mass,
            micro: 1_000_000,
        },
    ),
    (
        "kg",
        DimensionalConversion {
            dimension: Dimension::Mass,
            micro: 1_000_000_000,
        },
    ),
    (
        "ml",
        DimensionalConversion {
            dimension: Dimension::Volume,
            micro: 1_000,
        },
    ),
    (
        "l",
        DimensionalConversion {
            dimension: Dimension::Volume,
            micro: 1_000_000,
        },
    ),
    (
        "piece",
        DimensionalConversion {
            dimension: Dimension::Count,
            micro: 1_000_000,
        },
    ),
    (
        "dozen",
        DimensionalConversion {
            dimension: Dimension::Count,
            micro: 12_000_000,
        },
    ),
];

/// The `UNKNOWN_UNIT` resolver outcome's building block: a label that is
/// neither a Tier 1 physical constant nor a Tier 2 per-item conversion the
/// caller supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownUnit;

/// Tier 1: looks up a unit label case-insensitively — matching the SQL
/// `CHECK (lower(pack_unit_label) NOT IN (...))` on `item_unit_conversion`,
/// which reserves exactly these labels for Tier 1 — and converts an
/// integer quantity of that unit to an exact integer count of micro-units.
/// Exact: every Tier 1 factor is an integer multiple of `1`, so no rational
/// is ever needed for this tier.
pub fn convert_tier1(unit_label: &str, quantity: i128) -> Result<(Dimension, i128), UnknownUnit> {
    let lower = unit_label.to_ascii_lowercase();
    DIMENSIONAL_CONVERSIONS
        .iter()
        .find(|(label, _)| *label == lower)
        .map(|(_, conv)| (conv.dimension, quantity * conv.micro as i128))
        .ok_or(UnknownUnit)
}

/// Tier 2: a per-item pack conversion (`item_unit_conversion`), expressed
/// as an integer ratio — `1` of the pack unit equals `numerator /
/// denominator` micro-units of `source_dimension`'s canonical unit. Both
/// tiers are ratio multiplications, never decimal factors, for the same
/// reason money is paise.
///
/// Returns the exact result as a reduced `(numerator, denominator)` pair
/// rather than rounding: a pack ratio is one of the `…` factors ADR-018
/// §5's `applied_micro = round_half_away_from_zero(recipe_qty × line_qty ×
/// pack_ratio × …)` formula chains before the single rounding step happens
/// at the leaf — pair with [`round_ratio_half_away_from_zero`] once no
/// further chaining remains. `numerator`/`denominator` must both be `> 0`
/// (the schema's own `CHECK`); `None` on `i128` overflow, the same
/// defensive posture as every other checked operation on this path.
pub fn convert_tier2(
    quantity_of_pack_units: i128,
    numerator: i64,
    denominator: i64,
) -> Option<(i128, i128)> {
    Rational::from_int(quantity_of_pack_units)
        .checked_mul_ratio(numerator as i128, denominator as i128)
        .map(|r| (r.num, r.den))
}

/// Rounds an exact `numerator / denominator` ratio to the nearest integer,
/// half away from zero (ADR-018 §5) — the public entry point for a caller
/// (e.g. a Tier 2-only conversion with no recipe tree above it) that has
/// finished chaining ratios and needs the same rounding rule the resolver
/// uses at its leaves. `denominator` must be `> 0`.
pub fn round_ratio_half_away_from_zero(numerator: i128, denominator: i128) -> i128 {
    Rational {
        num: numerator,
        den: denominator,
    }
    .round_half_away_from_zero()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier1_mirrors_the_frozen_map_exactly() {
        assert_eq!(
            convert_tier1("kg", 1).unwrap(),
            (Dimension::Mass, 1_000_000_000)
        );
        assert_eq!(convert_tier1("g", 1).unwrap(), (Dimension::Mass, 1_000_000));
        assert_eq!(convert_tier1("mg", 1).unwrap(), (Dimension::Mass, 1_000));
        assert_eq!(convert_tier1("l", 1).unwrap(), (Dimension::Volume, 1_000_000));
        assert_eq!(convert_tier1("ml", 1).unwrap(), (Dimension::Volume, 1_000));
        assert_eq!(
            convert_tier1("piece", 1).unwrap(),
            (Dimension::Count, 1_000_000)
        );
        assert_eq!(
            convert_tier1("dozen", 1).unwrap(),
            (Dimension::Count, 12_000_000)
        );
    }

    #[test]
    fn tier1_scales_by_integer_quantity_exactly() {
        // 50 kg sack -> 5e10 micro-grams, the ADR-018 §3 worked example.
        assert_eq!(
            convert_tier1("kg", 50).unwrap(),
            (Dimension::Mass, 50_000_000_000)
        );
    }

    #[test]
    fn tier1_is_case_insensitive_and_rejects_unknown_labels() {
        assert_eq!(convert_tier1("KG", 2).unwrap().1, 2_000_000_000);
        assert_eq!(convert_tier1("packet", 1), Err(UnknownUnit));
        assert_eq!(convert_tier1("crate", 1), Err(UnknownUnit));
    }

    #[test]
    fn tier2_is_a_per_item_ratio_not_a_decimal_factor() {
        // "1 packet paneer = 200g" as a Tier 2 row: numerator/denominator
        // are exact integer micro-units of the item's own canonical unit.
        let (num, den) = convert_tier2(3, 200_000_000, 1).unwrap();
        // 3 packets * 200_000_000 micro-grams/packet
        assert_eq!(round_ratio_half_away_from_zero(num, den), 600_000_000);
    }

    #[test]
    fn tier2_carries_a_non_terminating_ratio_exactly_until_rounded() {
        // A ratio that does not divide evenly (e.g. 1 tray = 1/3 of a
        // dozen-equivalent unit some outlet defines) must not lose
        // precision before the caller's single rounding step.
        let (num, den) = convert_tier2(1, 1, 3).unwrap();
        assert_eq!(round_ratio_half_away_from_zero(num, den), 0); // 0.333 -> 0
        let (num2, den2) = convert_tier2(2, 1, 3).unwrap(); // now 2/3
        assert_eq!(round_ratio_half_away_from_zero(num2, den2), 1); // 0.667 -> 1
    }
}
