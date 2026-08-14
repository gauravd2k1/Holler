//! Offline invoice numbering (ADR-016 §2, task 2 of T7b).
//!
//! `invoice_series` (the DEFINITION — prefix template, reset policy,
//! padding) is cloud config, already stored by T7a's config repo functions.
//! `invoice_sequence` (the COUNTER) is edge-local and this module's whole
//! reason to exist: it renders a series' `prefix_template` and combines it
//! with the next value from [`crate::repo::next_invoice_sequence_value`],
//! always inside the caller's write transaction so the render-then-count
//! step and the invoice insert that consumes it are one atomic unit
//! (§33 — see that function's own doc comment for the crash argument).

use chrono::{DateTime, Utc};

use crate::error::{DbError, DbResult};
use crate::model::{InvoiceSeries, Outlet};

/// Derives the `{OUTLET}` token's value from `outlet.name`.
///
/// **Disclosed contract gap:** ADR-016's own worked example
/// (`'FY{FY}/{OUTLET}/'` -> `'FY26/PNQ/'`) implies a short outlet CODE like
/// an airport code, but `packages/contracts/sqlite/0001_init.sql`'s `outlet`
/// table carries no such column — only `id`/`brand_id`/`name`/`timezone`
/// (contracts frozen, ADR-008: this crate cannot add one). This function
/// derives a stable, deterministic short code from `outlet.name` instead:
/// uppercase alphanumeric characters only, first three. Two outlets sharing
/// a name prefix producing the same rendered code is a real limitation of
/// this gap, not hidden by this comment — a template that also includes a
/// numbering-unique token (`{FY}`/`{MM}` etc.) still cannot collide, since
/// uniqueness is enforced on the FULL rendered `invoice_number` per
/// `(outlet_id, series_id, invoice_number)`, not on the `{OUTLET}` token
/// alone; this only affects how readable a printed prefix is across outlets
/// sharing a brand.
pub(crate) fn derive_outlet_code(outlet_name: &str) -> String {
    let alnum: String = outlet_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_uppercase();
    if alnum.is_empty() {
        "OUT".to_string()
    } else {
        alnum.chars().take(3).collect()
    }
}

/// Splits an outlet-local `business_date` (`YYYY-MM-DD`) into
/// `(year, month, day)`. `business_date` is always this shape by contract
/// (`invoice.business_date` column comment); a malformed value is a caller
/// bug, surfaced as `InvalidInput` rather than silently truncated.
fn split_business_date(business_date: &str) -> DbResult<(i32, u32, u32)> {
    let bytes = business_date.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(DbError::InvalidInput(format!(
            "business_date {business_date:?} is not YYYY-MM-DD"
        )));
    }
    let year: i32 = business_date[0..4]
        .parse()
        .map_err(|_| DbError::InvalidInput(format!("business_date {business_date:?}: bad year")))?;
    let month: u32 = business_date[5..7]
        .parse()
        .map_err(|_| DbError::InvalidInput(format!("business_date {business_date:?}: bad month")))?;
    let day: u32 = business_date[8..10]
        .parse()
        .map_err(|_| DbError::InvalidInput(format!("business_date {business_date:?}: bad day")))?;
    Ok((year, month, day))
}

/// The Indian fiscal year (April -> March) a `business_date` falls in,
/// expressed as its END year — e.g. any date from 2025-04-01 through
/// 2026-03-31 is fiscal year "ending 2026", i.e. "FY26" in the two-digit
/// form the `{FY}` token renders.
fn fiscal_year_end(business_date: &str) -> DbResult<i32> {
    let (year, month, _day) = split_business_date(business_date)?;
    Ok(if month >= 4 { year + 1 } else { year })
}

/// Derives the `invoice_sequence.period_key` bucket for `reset_policy` at
/// `business_date` — contracts 0006's own documented shapes: `'ALL'`,
/// `'FY2026'`, `'2026-08'`, `'2026-08-12'`. A policy change is what starts a
/// fresh bucket rather than rewinding a live counter (the column comment's
/// own reasoning) — this function is pure and stateless, so callers get
/// that behaviour for free just by keying off the CURRENT `reset_policy`
/// every time, never a stored "current bucket".
pub(crate) fn compute_period_key(reset_policy: &str, business_date: &str) -> DbResult<String> {
    match reset_policy {
        "NEVER" => Ok("ALL".to_string()),
        "DAY" => {
            split_business_date(business_date)?; // validate shape
            Ok(business_date.to_string())
        }
        "MONTH" => {
            split_business_date(business_date)?;
            Ok(business_date[0..7].to_string())
        }
        "FY" => {
            let fy_end = fiscal_year_end(business_date)?;
            Ok(format!("FY{fy_end}"))
        }
        other => Err(DbError::InvalidInput(format!(
            "unknown invoice_series.reset_policy {other:?}"
        ))),
    }
}

/// Renders `template`'s tokens (`{FY} {YYYY} {MM} {DD} {OUTLET}`) against
/// `business_date` and `outlet_code`. `{FY}` renders the Indian fiscal
/// year's two-digit END year (`fiscal_year_end % 100`, zero-padded) —
/// ADR-016's own example, `'FY26/PNQ/'`.
pub(crate) fn render_prefix(template: &str, business_date: &str, outlet_code: &str) -> DbResult<String> {
    let (year, month, day) = split_business_date(business_date)?;
    let fy_end = fiscal_year_end(business_date)?;
    let fy2 = format!("{:02}", fy_end.rem_euclid(100));

    let mut out = template.to_string();
    out = out.replace("{FY}", &fy2);
    out = out.replace("{YYYY}", &year.to_string());
    out = out.replace("{MM}", &format!("{month:02}"));
    out = out.replace("{DD}", &format!("{day:02}"));
    out = out.replace("{OUTLET}", outlet_code);
    Ok(out)
}

/// Mints the next `invoice_number` for `series`, inside `tx` — the caller's
/// write transaction — so the counter bump and the invoice insert that
/// consumes the number are one atomic step (§33; see
/// [`crate::repo::next_invoice_sequence_value`]'s doc comment for the crash
/// argument). `updated_at`/`_at` should be the same instant the caller is
/// stamping on the invoice row itself.
pub(crate) fn mint_invoice_number(
    tx: &rusqlite::Transaction,
    series: &InvoiceSeries,
    outlet: &Outlet,
    business_date: &str,
    updated_at: &str,
) -> DbResult<String> {
    let period_key = compute_period_key(&series.reset_policy, business_date)?;
    let seq = crate::repo::next_invoice_sequence_value(tx, &series.id, &period_key, updated_at)?;
    let outlet_code = derive_outlet_code(&outlet.name);
    let prefix = render_prefix(&series.prefix_template, business_date, &outlet_code)?;
    let width = usize::try_from(series.padding_width).unwrap_or(6);
    Ok(format!("{prefix}{seq:0width$}"))
}

/// Parses `invoice_date` (ISO8601 UTC) into a real instant for tax-rule
/// resolution — thin re-export of [`crate::tax::parse_utc`] so callers in
/// this module do not need to reach across to the `tax` module by name.
pub(crate) fn parse_utc(s: &str) -> DbResult<DateTime<Utc>> {
    crate::tax::parse_utc(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(reset_policy: &str, padding_width: i64) -> InvoiceSeries {
        InvoiceSeries {
            id: "ser-1".to_string(),
            outlet_id: "outlet-1".to_string(),
            code: "SALES".to_string(),
            prefix_template: "FY{FY}/{OUTLET}/".to_string(),
            reset_policy: reset_policy.to_string(),
            padding_width,
            is_active: true,
            config_version: 1,
        }
    }

    #[test]
    fn derives_outlet_code_from_name() {
        assert_eq!(derive_outlet_code("Pune"), "PUN");
        assert_eq!(derive_outlet_code("MG Road Cafe"), "MGR");
        assert_eq!(derive_outlet_code(""), "OUT");
        assert_eq!(derive_outlet_code("99 Bottles"), "99B");
    }

    #[test]
    fn renders_the_adr016_worked_example() {
        // ADR-016 §2: 'FY{FY}/{OUTLET}/' with padding_width 6 yields
        // FY26/PNQ/001423 for a date in fiscal year ending 2026.
        let prefix = render_prefix("FY{FY}/{OUTLET}/", "2026-02-15", "PNQ").expect("render");
        assert_eq!(prefix, "FY26/PNQ/");
    }

    #[test]
    fn fiscal_year_boundary_is_1_april() {
        assert_eq!(fiscal_year_end("2026-03-31").unwrap(), 2026);
        assert_eq!(fiscal_year_end("2026-04-01").unwrap(), 2027);
    }

    #[test]
    fn period_key_shapes_match_the_schema_comment() {
        assert_eq!(compute_period_key("NEVER", "2026-08-12").unwrap(), "ALL");
        assert_eq!(compute_period_key("FY", "2026-08-12").unwrap(), "FY2027");
        assert_eq!(compute_period_key("FY", "2026-02-12").unwrap(), "FY2026");
        assert_eq!(compute_period_key("MONTH", "2026-08-12").unwrap(), "2026-08");
        assert_eq!(compute_period_key("DAY", "2026-08-12").unwrap(), "2026-08-12");
    }

    #[test]
    fn unknown_reset_policy_errors() {
        assert!(compute_period_key("QUARTERLY", "2026-08-12").is_err());
    }

    #[test]
    fn malformed_business_date_errors_rather_than_panics() {
        assert!(compute_period_key("DAY", "12-08-2026").is_err());
        assert!(render_prefix("{YYYY}", "not-a-date", "PNQ").is_err());
    }

    #[test]
    fn mint_invoice_number_pads_and_increments_in_memory() {
        let db = crate::Db::open_in_memory_for_tests().expect("open");
        let outlet = Outlet {
            id: "outlet-1".to_string(),
            brand_id: "brand-1".to_string(),
            name: "Pune".to_string(),
            timezone: "Asia/Kolkata".to_string(),
            config_version: 1,
            created_at: "2026-08-01T00:00:00Z".to_string(),
            updated_at: "2026-08-01T00:00:00Z".to_string(),
        };
        crate::repo::upsert_outlet(db.connection(), &outlet).expect("seed outlet");
        crate::repo::upsert_invoice_series(db.connection(), &series("FY", 6)).expect("seed series");

        let mut db = db;
        let tx = db.connection_mut().transaction().expect("tx");
        let s = series("FY", 6);
        let n1 = mint_invoice_number(&tx, &s, &outlet, "2026-08-12", "2026-08-12T10:00:00Z").expect("mint 1");
        let n2 = mint_invoice_number(&tx, &s, &outlet, "2026-08-12", "2026-08-12T10:05:00Z").expect("mint 2");
        tx.commit().expect("commit");

        assert_eq!(n1, "FY27/PUN/000001");
        assert_eq!(n2, "FY27/PUN/000002");
    }
}
