//! `business_date` (ADR-018 §9.2, contracts 0.5.0 `outlet.day_start_time`,
//! `packages/contracts/sqlite/0013_outlet_day_start.sql`):
//!
//! ```text
//! business_date(instant_utc, outlet)
//!     = date_part( (instant_utc -> outlet.timezone) - outlet.day_start_time )
//! ```
//!
//! **Computed ONCE, at write time, and stored — never recomputed on read.**
//! Every caller in this crate calls [`compute_business_date`] exactly once
//! per row and persists the result; nothing re-derives a `business_date`
//! from a stored `occurred_at` later.
//!
//! **This is the corrected implementation the 0013 migration header names.**
//! `apps/pos/src-tauri/src/commands/billing.rs::business_date_from` takes the
//! first ten characters of a UTC instant, which mis-buckets any IST outlet
//! trading between midnight and 05:30. That function is NOT called from here
//! and is out of this crate's authority to fix (a POS-side defect, filed in
//! `docs/retro.md`) — this module exists so the M4 stock ledger never
//! inherits it.
//!
//! **Deliberately infallible.** `outlet.timezone` and `outlet.day_start_time`
//! are cloud config that arrived over the wire (ADR-011/§50.1) — this module
//! never trusts a constraint that may have been written by an older schema
//! version or a malformed edit, the same posture `inventory::resolve`
//! already takes toward `recipe`/`recipe_ingredient`. A malformed timezone or
//! `day_start_time` degrades to a safe fallback (UTC / `00:00`) rather than
//! failing the write — computing a stock ledger row's `business_date` must
//! never be able to abort `confirm_order` (ADR-018 Rule "a missing or broken
//! recipe never fails a confirm", generalised to every config input on this
//! path).

use chrono::{DateTime, Duration, NaiveTime, Utc};
use chrono_tz::Tz;

/// Resolves `timezone` as an IANA identifier via `chrono-tz` (never a
/// hard-coded offset — a zone with DST must behave correctly even though
/// `Asia/Kolkata`, the default, has none). Falls back to UTC on anything
/// that does not parse, rather than erroring: see the module doc comment.
fn resolve_timezone(timezone: &str) -> Tz {
    timezone.parse::<Tz>().unwrap_or(chrono_tz::UTC)
}

/// Parses `day_start_time` (`outlet.day_start_time`, `TEXT NOT NULL DEFAULT
/// '00:00'`, local `HH:MM`) into a duration to subtract from local wall-clock
/// time. Falls back to zero (the `'00:00'` default's own meaning) on
/// anything that does not parse as `HH:MM`.
fn day_start_duration(day_start_time: &str) -> Duration {
    NaiveTime::parse_from_str(day_start_time, "%H:%M")
        .ok()
        .map(|t| t.signed_duration_since(NaiveTime::MIN))
        .unwrap_or_else(Duration::zero)
}

/// Computes the outlet-local `business_date` (`YYYY-MM-DD`) for one instant,
/// per the 0013 definition above.
///
/// `occurred_at_utc` is an RFC3339/ISO8601 UTC instant (every timestamp
/// column in this crate is a plain string — CLAUDE.md "Money / time /
/// identifiers"). Falls back to today's UTC calendar date (via
/// [`chrono::Utc::now`]) if `occurred_at_utc` itself does not parse — the
/// same infallible posture as the timezone/day-start fallbacks above; this
/// path should be unreachable in practice, since every caller sources
/// `occurred_at_utc` from a value this crate or its caller already stamped,
/// but a bad string here must degrade, not abort `confirm_order`.
pub(crate) fn compute_business_date(
    occurred_at_utc: &str,
    timezone: &str,
    day_start_time: &str,
) -> String {
    let instant: DateTime<Utc> = DateTime::parse_from_rfc3339(occurred_at_utc)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let tz = resolve_timezone(timezone);
    let local_naive = instant.with_timezone(&tz).naive_local();
    let shifted = local_naive - day_start_duration(day_start_time);
    shifted.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_day_start_is_the_plain_outlet_local_date() {
        // 2026-08-21T20:00:00Z is 2026-08-22T01:30:00+05:30 in IST — after
        // midnight local, but '00:00' day-start (correct for any outlet that
        // closes before midnight, per the 0013 header) still books it to the
        // 22nd, the plain local calendar date.
        assert_eq!(
            compute_business_date("2026-08-21T20:00:00Z", "Asia/Kolkata", "00:00"),
            "2026-08-22"
        );
    }

    #[test]
    fn a_late_night_sale_books_to_the_previous_business_day_with_a_day_start() {
        // The ADR-018 §9.2 worked example: 01:30 local with day_start_time
        // '04:00' books to the PREVIOUS date. Same instant as above.
        assert_eq!(
            compute_business_date("2026-08-21T20:00:00Z", "Asia/Kolkata", "04:00"),
            "2026-08-21"
        );
    }

    #[test]
    fn a_sale_after_day_start_books_to_the_same_local_date() {
        // 2026-08-22T05:00:00Z = 10:30 IST, well after a 04:00 day-start.
        assert_eq!(
            compute_business_date("2026-08-22T05:00:00Z", "Asia/Kolkata", "04:00"),
            "2026-08-22"
        );
    }

    #[test]
    fn iana_identifiers_are_resolved_not_hard_coded_offsets() {
        // UTC and IST disagree on the calendar date for this instant; a
        // hard-coded +05:30 offset would happen to get this one right too,
        // so this test's real job is exercising a second, non-Kolkata zone
        // through the same code path (America/New_York, UTC-4 in August, no
        // day-start correction needed since it does not cross midnight).
        assert_eq!(
            compute_business_date("2026-08-21T02:00:00Z", "America/New_York", "00:00"),
            "2026-08-20"
        );
        assert_eq!(
            compute_business_date("2026-08-21T02:00:00Z", "Asia/Kolkata", "00:00"),
            "2026-08-21"
        );
    }

    #[test]
    fn an_unresolvable_timezone_falls_back_to_utc_rather_than_erroring() {
        // Malformed config must degrade, never abort a write on this path.
        assert_eq!(
            compute_business_date("2026-08-21T23:30:00Z", "Not/AZone", "00:00"),
            "2026-08-21"
        );
    }

    #[test]
    fn a_malformed_day_start_time_falls_back_to_zero_offset() {
        assert_eq!(
            compute_business_date("2026-08-21T20:00:00Z", "Asia/Kolkata", "garbage"),
            compute_business_date("2026-08-21T20:00:00Z", "Asia/Kolkata", "00:00"),
        );
    }
}
