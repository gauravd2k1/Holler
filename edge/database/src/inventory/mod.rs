//! Inventory units, integer conversion and recipe resolution (Milestone 4,
//! track T1, ADR-018).
//!
//! **Pure arithmetic and resolution only.** No ledger writes, no Tauri
//! commands, no UI — those are T2 (deduction into `stock_ledger_entry` /
//! `stock_deduction_gap`) and T5. Nothing in this module opens a
//! transaction or performs a write; [`resolve::resolve_recipe_for_variant`]
//! only reads `recipe` / `recipe_ingredient` / `inventory_item`.
//!
//! Module layout:
//!   - `rational`  — the `i128` exact-rational accumulator (`pub(crate)`,
//!     an internal primitive — the `tax::rounding` precedent).
//!   - `units`     — the two-tier conversion scheme (ADR-018 §4): Tier 1 is
//!     the frozen dimensional map; Tier 2 is the per-item pack ratio.
//!   - `resolve`   — recipe resolution, transitive through sub-recipes,
//!     with an independent cycle/depth backstop (ADR-018 §7).
//!
//! **No float appears anywhere in the quantity path.** Every quantity is an
//! integer count of micro-units; intermediate accumulation is `i128`
//! numerator/denominator, never a `f32`/`f64`.
//!
//! **`yield_factor_ppm` is INERT in M4.** It exists on `recipe_ingredient`
//! and `inventory_item`, defaults to the identity (`1_000_000`), and
//! nothing in this module reads it — ADR-018 §8. Applying it here would be
//! exactly the silent-correctness defect the ADR calls out.

mod rational;
mod resolve;
mod units;

pub use resolve::{
    resolve_recipe_for_variant, GapReason, RecipeResolution, ResolveOutcome, ResolvedLeaf,
    MAX_RECIPE_DEPTH,
};
pub use units::{
    convert_tier1, convert_tier2, round_ratio_half_away_from_zero, Dimension,
    DimensionalConversion, UnknownUnit, DIMENSIONAL_CONVERSIONS,
};

// `rational::Rational` itself is deliberately NOT re-exported: it is an
// internal arithmetic primitive (the `tax::rounding` precedent). Every
// public entry point that needs to expose an exact intermediate result
// (`convert_tier2`) does so as a plain `(numerator, denominator)` pair
// instead, paired with `round_ratio_half_away_from_zero` for a caller that
// has finished chaining ratios.
