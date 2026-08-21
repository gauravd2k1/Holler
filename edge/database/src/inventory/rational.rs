//! Exact-rational accumulator for recipe resolution (ADR-018 §5). `i128`
//! numerator/denominator, reduced by GCD after every operation so an
//! eight-level sub-recipe chain (`MAX_RECIPE_DEPTH`) does not overflow
//! before it is ever rounded. **This type never rounds itself** except via
//! [`Rational::round_half_away_from_zero`], which the caller invokes
//! exactly once, at the leaf — never per level.
//!
//! `pub(crate)`: this is the resolver's internal arithmetic primitive, the
//! same posture `edge/database/src/tax/rounding.rs` takes with its own
//! functions — exposing it invites a caller to reimplement the ADR-018 §5
//! policy piecemeal instead of going through the resolver.

/// An exact rational number. `den` is always `> 0` by construction; sign
/// lives entirely on `num`. Every arithmetic operation is checked — an
/// overflow here means the *defensive* backstop (an adversarial or
/// corrupted config graph), not a code path a well-formed recipe can ever
/// reach, and it must degrade to `None` rather than panic: this arithmetic
/// runs inside `confirm_order`'s transaction, on the outlet's only SQLite
/// writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rational {
    pub num: i128,
    pub den: i128,
}

impl Rational {
    pub fn from_int(n: i128) -> Self {
        Rational { num: n, den: 1 }
    }

    pub fn zero() -> Self {
        Rational { num: 0, den: 1 }
    }

    /// `self * (n / d)`, reduced. `d` must be `> 0` — every caller in this
    /// crate supplies either a `CHECK (… > 0)`-guarded stored ratio or the
    /// literal constant `1_000_000`.
    pub fn checked_mul_ratio(self, n: i128, d: i128) -> Option<Rational> {
        debug_assert!(d > 0, "Rational::checked_mul_ratio requires d > 0");
        let num = self.num.checked_mul(n)?;
        let den = self.den.checked_mul(d)?;
        Some(Rational { num, den }.reduced())
    }

    /// `self + other`, reduced.
    pub fn checked_add(self, other: Rational) -> Option<Rational> {
        let num = self
            .num
            .checked_mul(other.den)?
            .checked_add(other.num.checked_mul(self.den)?)?;
        let den = self.den.checked_mul(other.den)?;
        Some(Rational { num, den }.reduced())
    }

    fn reduced(self) -> Rational {
        if self.num == 0 {
            return Rational { num: 0, den: 1 };
        }
        let g = gcd(self.num.unsigned_abs(), self.den.unsigned_abs());
        if g <= 1 {
            return self;
        }
        Rational {
            num: self.num / (g as i128),
            den: self.den / (g as i128),
        }
    }

    /// Rounds to the nearest integer, **half away from zero** (ADR-018 §5):
    /// the sign is taken out first and reapplied, so a modifier delta of
    /// `-0.5` micro-units rounds to `-1`, not to `0`. Implemented on
    /// integers only: `(2n + d) / (2d)` under truncating division, on the
    /// magnitude, with the sign reapplied afterwards.
    pub fn round_half_away_from_zero(self) -> i128 {
        if self.num == 0 {
            return 0;
        }
        let negative = self.num < 0;
        let n = self.num.unsigned_abs();
        let d = self.den.unsigned_abs();
        let rounded = ((2 * n + d) / (2 * d)) as i128;
        if negative {
            -rounded
        } else {
            rounded
        }
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_int_and_zero() {
        assert_eq!(Rational::from_int(5), Rational { num: 5, den: 1 });
        assert_eq!(Rational::zero(), Rational { num: 0, den: 1 });
    }

    #[test]
    fn checked_mul_ratio_reduces() {
        // 5 * (2/4) = 10/4 = 5/2, reduced.
        let r = Rational::from_int(5).checked_mul_ratio(2, 4).unwrap();
        assert_eq!(r, Rational { num: 5, den: 2 });
    }

    #[test]
    fn checked_add_reduces() {
        // 1/2 + 1/2 = 1
        let r = Rational::from_int(1)
            .checked_mul_ratio(1, 2)
            .unwrap()
            .checked_add(Rational::from_int(1).checked_mul_ratio(1, 2).unwrap())
            .unwrap();
        assert_eq!(r, Rational::from_int(1));
    }

    #[test]
    fn round_half_away_from_zero_positive_half_rounds_up() {
        // 1/2 -> 1 (half away from zero, not banker's -> 0)
        let half = Rational { num: 1, den: 2 };
        assert_eq!(half.round_half_away_from_zero(), 1);
    }

    #[test]
    fn round_half_away_from_zero_negative_half_rounds_down_in_magnitude() {
        // -1/2 -> -1, never 0. This is the exact case ADR-018 §5 calls out:
        // "No Onion" at -0.5 micro-units must round to -1.
        let neg_half = Rational { num: -1, den: 2 };
        assert_eq!(neg_half.round_half_away_from_zero(), -1);
    }

    #[test]
    fn round_half_away_from_zero_below_half_rounds_toward_zero() {
        let below = Rational { num: 4, den: 10 }; // 0.4
        assert_eq!(below.round_half_away_from_zero(), 0);
        let neg_below = Rational { num: -4, den: 10 };
        assert_eq!(neg_below.round_half_away_from_zero(), 0);
    }

    #[test]
    fn round_once_disagrees_with_round_each_level() {
        // The exact case ADR-018 §5 exists to protect: rounding at each of
        // three levels drifts from rounding once at the end.
        //
        // Level 1: 1 unit of parent needs 1/3 unit of a sub-recipe.
        // Level 2: 1 unit of that sub-recipe needs 1/3 unit of a further
        //          sub-recipe.
        // Level 3 (leaf): 1 unit of THAT needs 5 micro-units of the raw
        //          ingredient.
        //
        // Exact answer: 1 * (1/3) * (1/3) * 5 = 5/9 -> round-once = 1.
        //
        // Rounding at each level: round(1/3 * 1) = 0 immediately collapses
        // the whole chain to 0 — a different (wrong) answer, which is
        // exactly the drift this decision exists to prevent.
        let exact = Rational::from_int(1)
            .checked_mul_ratio(1, 3)
            .unwrap()
            .checked_mul_ratio(1, 3)
            .unwrap()
            .checked_mul_ratio(5, 1)
            .unwrap();
        assert_eq!(exact.round_half_away_from_zero(), 1);

        let per_level_step1 = Rational::from_int(1).checked_mul_ratio(1, 3).unwrap();
        let per_level_step1_rounded = per_level_step1.round_half_away_from_zero();
        assert_eq!(per_level_step1_rounded, 0, "0.333 rounds to 0 at this level");
        // Once collapsed to an integer 0 at level 1, every subsequent level
        // multiplies 0 by something and stays 0 forever — permanently wrong
        // versus the round-once answer of 1.
    }

    #[test]
    fn overflow_returns_none_rather_than_panicking() {
        let huge = Rational::from_int(i128::MAX / 2);
        assert!(huge.checked_mul_ratio(3, 1).is_none());
    }
}
