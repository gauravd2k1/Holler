//! Discount resolution — pure functions, no SQLite/Tauri concerns (CLAUDE.md
//! "business logic outside UI components / command handlers"). Turns a
//! cashier's choice of one `discount_definition` row (contracts 0.4.0,
//! ADR-016) into the per-unit paise number `edge/database`'s tax engine
//! actually applies (`InvoiceLineShare::discount_per_unit_paise`).
//!
//! `holler_edge_database::model::InvoiceLineShare::discount_per_unit_paise`'s
//! own doc comment is explicit: "Discount POLICY (`discount_definition`
//! resolution) is out of T7b's scope — this crate only applies a number it
//! is given". This module is that policy. It is deliberately NOT a re-do of
//! the edge's tax arithmetic (CLAUDE.md: "Do not recompute tax in TypeScript
//! or in the Tauri layer") — it only turns a governance row plus one line's
//! `unit_price_paise` into the single input number the edge already knows
//! how to consume, using integer basis-point arithmetic throughout (never a
//! float — CLAUDE.md §Money).
//!
//! `requires_reason` and `required_permission` are enforced here, not merely
//! displayed: a discount that demands a reason is rejected outright without
//! one, and one naming a permission is rejected outright for a caller
//! lacking it (ADR-016/§28, task requirement: "binding, not advisory").
//!
//! Scope: only `DiscountScope::LINE` is implemented. A `BILL`-scope
//! definition is rejected with a distinct, legible code rather than silently
//! narrowed into a line discount it was never defined as — BILL scope is
//! unimplemented, reported as such, not faked.
//!
//! This module does NOT itself validate that the number it produces is a
//! *legal* discount (non-negative, not exceeding `unit_price_paise`) — that
//! guard belongs to `edge/database/src/tax/engine.rs::compute_line_base`,
//! and is deliberately left to fire on whatever this module hands it,
//! exactly as it would on any other caller. A malformed `discount_definition`
//! (e.g. a cloud misconfiguration with a negative `value_paise`, which
//! nothing at the SQLite layer prevents on write) must not be trusted here
//! either — it must still be caught, just one layer further in.

use holler_edge_database::model::DiscountDefinition;

use crate::error::AppError;

/// Basis points denominator — 10000 bps = 100%, matching
/// `packages/contracts/src/types/tax.ts` `RateBpsSchema` and
/// `edge/database/src/tax/rounding.rs::BPS_DENOMINATOR`.
const BPS_DENOMINATOR: i64 = 10_000;

/// Half-up rounding of a non-negative `numerator / denominator`, integer
/// arithmetic only. Mirrors the *policy* of
/// `edge/database/src/tax/rounding.rs::round_half_up_div` (not its code,
/// which is `pub(crate)` to that crate) so a PERCENT discount rounds the
/// same way every other basis-point money figure in this product does.
fn round_half_up_div(numerator: i64, denominator: i64) -> i64 {
    (numerator + denominator / 2) / denominator
}

/// Resolves `def` against one line's `unit_price_paise`, returning the
/// per-unit paise figure to carry on `InvoiceLineShare::discount_per_unit_paise`.
pub fn resolve_line_discount_per_unit_paise(
    def: &DiscountDefinition,
    unit_price_paise: i64,
    reason: Option<&str>,
    caller_permissions: &[String],
) -> Result<i64, AppError> {
    if def.scope != "LINE" {
        return Err(AppError {
            code: "DISCOUNT_SCOPE_NOT_SUPPORTED",
            message: format!(
                "discount '{}' is a BILL-scope discount — applying a bill-level discount is not \
                 yet implemented in this build; only LINE-scope discounts can be applied here",
                def.code
            ),
        });
    }

    if !def.is_active {
        return Err(AppError {
            code: "DISCOUNT_NOT_ACTIVE",
            message: format!("discount '{}' is not currently active", def.code),
        });
    }

    if def.requires_reason {
        let has_reason = reason.map(|r| !r.trim().is_empty()).unwrap_or(false);
        if !has_reason {
            return Err(AppError {
                code: "DISCOUNT_REASON_REQUIRED",
                message: format!(
                    "discount '{}' requires a reason before it can be applied",
                    def.code
                ),
            });
        }
    }

    if let Some(perm) = &def.required_permission {
        if !caller_permissions.iter().any(|p| p == perm) {
            return Err(AppError {
                code: "DISCOUNT_PERMISSION_DENIED",
                message: format!(
                    "applying discount '{}' requires the '{perm}' permission, which this user does \
                     not have",
                    def.code
                ),
            });
        }
    }

    match def.method.as_str() {
        "PERCENT" => {
            let bps = def.value_bps.ok_or_else(|| AppError {
                code: "DISCOUNT_MISCONFIGURED",
                message: format!("discount '{}' is PERCENT but carries no value_bps", def.code),
            })?;
            let computed = round_half_up_div(unit_price_paise * bps, BPS_DENOMINATOR);
            Ok(match def.max_discount_paise {
                Some(max) if computed > max => max,
                _ => computed,
            })
        }
        "AMOUNT" => def.value_paise.ok_or_else(|| AppError {
            code: "DISCOUNT_MISCONFIGURED",
            message: format!("discount '{}' is AMOUNT but carries no value_paise", def.code),
        }),
        other => Err(AppError {
            code: "DISCOUNT_MISCONFIGURED",
            message: format!("discount '{}' has unknown method '{other}'", def.code),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn percent_def(value_bps: i64, max_discount_paise: Option<i64>) -> DiscountDefinition {
        DiscountDefinition {
            id: "def-1".into(),
            outlet_id: "outlet-1".into(),
            code: "STAFF10".into(),
            name: "Staff 10%".into(),
            scope: "LINE".into(),
            method: "PERCENT".into(),
            value_bps: Some(value_bps),
            value_paise: None,
            max_discount_paise,
            required_permission: None,
            requires_reason: false,
            is_active: true,
            effective_from: "2020-01-01T00:00:00Z".into(),
            effective_to: None,
            config_version: 1,
        }
    }

    fn amount_def(value_paise: i64) -> DiscountDefinition {
        DiscountDefinition {
            id: "def-2".into(),
            outlet_id: "outlet-1".into(),
            code: "FLAT50".into(),
            name: "Flat Rs.50 off".into(),
            scope: "LINE".into(),
            method: "AMOUNT".into(),
            value_bps: None,
            value_paise: Some(value_paise),
            max_discount_paise: None,
            required_permission: None,
            requires_reason: false,
            is_active: true,
            effective_from: "2020-01-01T00:00:00Z".into(),
            effective_to: None,
            config_version: 1,
        }
    }

    #[test]
    fn percent_discount_is_computed_by_integer_basis_points_not_float() {
        // 10% off Rs.325.00 (32500 paise): 32500 * 1000 / 10000 = 3250 paise
        // exactly — no float division is involved.
        let def = percent_def(1000, None);
        let got = resolve_line_discount_per_unit_paise(&def, 32_500, None, &[]).expect("resolves");
        assert_eq!(got, 3_250);
    }

    #[test]
    fn percent_discount_rounds_half_up() {
        // 33.33% (3333 bps) of Rs.1.00 (100 paise): 100*3333/10000 = 33.33 ->
        // half-up rounds to 33 (raw remainder 3300/10000 < half), exercised
        // with a case that actually crosses the half boundary below.
        let def = percent_def(3333, None);
        assert_eq!(
            resolve_line_discount_per_unit_paise(&def, 100, None, &[]).unwrap(),
            33
        );
        // 15% (1500 bps) of Rs.0.33 (33 paise): 33*1500 = 49500;
        // 49500/10000 = 4.95 -> half-up rounds to 5.
        let def2 = percent_def(1500, None);
        assert_eq!(
            resolve_line_discount_per_unit_paise(&def2, 33, None, &[]).unwrap(),
            5
        );
    }

    #[test]
    fn max_discount_paise_caps_a_percent_discount() {
        // 50% of Rs.500.00 (50000 paise) = 25000 paise, capped to 5000.
        let def = percent_def(5000, Some(5_000));
        let got = resolve_line_discount_per_unit_paise(&def, 50_000, None, &[]).expect("resolves");
        assert_eq!(got, 5_000);
    }

    #[test]
    fn amount_discount_is_the_configured_paise_value_verbatim() {
        let def = amount_def(5_000);
        let got = resolve_line_discount_per_unit_paise(&def, 32_500, None, &[]).expect("resolves");
        assert_eq!(got, 5_000);
    }

    #[test]
    fn requires_reason_blocks_application_without_one() {
        let mut def = amount_def(1_000);
        def.requires_reason = true;
        let err =
            resolve_line_discount_per_unit_paise(&def, 10_000, None, &[]).expect_err("must reject");
        assert_eq!(err.code, "DISCOUNT_REASON_REQUIRED");

        let err_blank = resolve_line_discount_per_unit_paise(&def, 10_000, Some("   "), &[])
            .expect_err("blank reason must also reject");
        assert_eq!(err_blank.code, "DISCOUNT_REASON_REQUIRED");

        let ok = resolve_line_discount_per_unit_paise(&def, 10_000, Some("manager approved"), &[])
            .expect("a real reason satisfies the gate");
        assert_eq!(ok, 1_000);
    }

    #[test]
    fn required_permission_blocks_a_caller_lacking_it() {
        let mut def = amount_def(1_000);
        def.required_permission = Some("bill.discount.override".into());
        let err = resolve_line_discount_per_unit_paise(&def, 10_000, None, &[])
            .expect_err("caller lacks the permission");
        assert_eq!(err.code, "DISCOUNT_PERMISSION_DENIED");

        let ok = resolve_line_discount_per_unit_paise(
            &def,
            10_000,
            None,
            &["bill.discount.override".to_string()],
        )
        .expect("caller has the permission");
        assert_eq!(ok, 1_000);
    }

    #[test]
    fn bill_scope_is_rejected_as_unimplemented_not_silently_narrowed() {
        let mut def = amount_def(1_000);
        def.scope = "BILL".into();
        let err = resolve_line_discount_per_unit_paise(&def, 10_000, None, &[])
            .expect_err("BILL scope must not be silently applied as a line discount");
        assert_eq!(err.code, "DISCOUNT_SCOPE_NOT_SUPPORTED");
    }

    #[test]
    fn inactive_discount_is_rejected() {
        let mut def = amount_def(1_000);
        def.is_active = false;
        let err = resolve_line_discount_per_unit_paise(&def, 10_000, None, &[])
            .expect_err("inactive discount must be rejected");
        assert_eq!(err.code, "DISCOUNT_NOT_ACTIVE");
    }
}
