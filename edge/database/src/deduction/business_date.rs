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
//! ============================================================================
//! INFALLIBLE BY CONSTRUCTION, NOT BY `unwrap_or`
//! ============================================================================
//!
//! An earlier version of this module resolved a malformed `outlet.timezone`
//! with `.parse::<Tz>().unwrap_or(chrono_tz::UTC)`. That is the exact defect
//! this milestone exists to remove, one layer down: `business_date_from`
//! (above) computes a UTC date while claiming outlet-local, and a silent
//! `unwrap_or` reintroduces the same failure — a STORED, PLAUSIBLE-LOOKING,
//! WRONG business date, with no signal anywhere that it happened. **A silent
//! fallback to a different valid value is worse than a panic**: a panic is
//! visible, and a wrong-but-plausible date in an append-only ledger is not.
//!
//! [`OutletTimezone`] and [`DayStartTime`] are constructible only through a
//! checked `parse`. [`compute_business_date`] takes the typed values (and a
//! typed `DateTime<Utc>`, never a raw string) and cannot fail — not because
//! it swallows a failure, but because by the time a value of either type
//! exists, it has already been proven to parse. The unparseable case is
//! rejected where the string is first read off the wire/DB (`repo::
//! upsert_outlet` for `timezone`; here, defensively, for `day_start_time` —
//! see that type's doc comment for the residual gap this leaves), never
//! absorbed where money and stock are computed.

use chrono::{DateTime, Duration, NaiveTime, Utc};
use chrono_tz::Tz;

use crate::error::DbError;

/// A validated `outlet.timezone` — proof, by construction, that the string
/// it came from names a real IANA zone. The checked boundary is
/// [`repo::upsert_outlet`](crate::repo::upsert_outlet), the actual
/// config-apply path for this column; an unparseable value is rejected
/// there, as a whole-write config defect, never reaching this type.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OutletTimezone(Tz);

impl OutletTimezone {
    /// The one constructor. Never call `.unwrap_or(_)` on the `Err` arm —
    /// see the module doc comment for exactly why that shape is the defect
    /// this type exists to make impossible to write by accident.
    pub(crate) fn parse(s: &str) -> Result<Self, DbError> {
        s.parse::<Tz>()
            .map(OutletTimezone)
            .map_err(|_| DbError::InvalidInput(format!("outlet.timezone {s:?} is not a valid IANA timezone identifier")))
    }
}

/// A validated `outlet.day_start_time` (`HH:MM`, `00`–`23` / `00`–`59`).
///
/// **The gap this comment used to name is closed (M4 T4b).** `day_start_time`
/// now has a dedicated config-apply write path,
/// [`repo::upsert_outlet_day_start_time`](crate::repo::upsert_outlet_day_start_time),
/// which validates through [`DayStartTime::parse`] before writing anything —
/// the same posture `repo::upsert_outlet` already takes for `timezone` — so
/// an unparseable value rejects the whole bundle apply rather than landing
/// as the migration's own `DEFAULT '00:00'` forever. [`DayStartTime::parse`]
/// is still called defensively at the one place this crate reads the column
/// for computation (`repo::get_outlet_business_date_config`), and a value
/// that fails to parse there propagates as a real `DbError` — never a
/// silent substitution — rather than being absorbed into a fabricated
/// offset. Two paths validate the same rule for two different reasons: the
/// write path rejects a bad bundle loudly at apply time; the read path is a
/// second, independent check that never trusts "it must be valid, it was
/// validated on the way in" from a different transaction on a different
/// day.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DayStartTime(Duration);

impl DayStartTime {
    pub(crate) fn parse(s: &str) -> Result<Self, DbError> {
        let time = NaiveTime::parse_from_str(s, "%H:%M").map_err(|_| {
            DbError::InvalidInput(format!(
                "outlet.day_start_time {s:?} is not a valid HH:MM time"
            ))
        })?;
        Ok(DayStartTime(time.signed_duration_since(NaiveTime::MIN)))
    }
}

/// Computes the outlet-local `business_date` (`YYYY-MM-DD`) for one instant,
/// per the 0013 definition above. Takes only already-validated types, so
/// this function itself cannot fail — there is no fallback branch anywhere
/// in its body, because there is nothing left that could still be invalid.
pub(crate) fn compute_business_date(
    occurred_at: DateTime<Utc>,
    timezone: &OutletTimezone,
    day_start_time: &DayStartTime,
) -> String {
    let local_naive = occurred_at.with_timezone(&timezone.0).naive_local();
    let shifted = local_naive - day_start_time.0;
    shifted.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compute(occurred_at_utc: &str, timezone: &str, day_start_time: &str) -> String {
        let occurred_at = DateTime::parse_from_rfc3339(occurred_at_utc)
            .expect("test fixture instant must parse")
            .with_timezone(&Utc);
        let tz = OutletTimezone::parse(timezone).expect("test fixture timezone must parse");
        let ds = DayStartTime::parse(day_start_time).expect("test fixture day_start_time must parse");
        compute_business_date(occurred_at, &tz, &ds)
    }

    #[test]
    fn default_day_start_is_the_plain_outlet_local_date() {
        // 2026-08-21T20:00:00Z is 2026-08-22T01:30:00+05:30 in IST — after
        // midnight local, but '00:00' day-start (correct for any outlet that
        // closes before midnight, per the 0013 header) still books it to the
        // 22nd, the plain local calendar date.
        assert_eq!(compute("2026-08-21T20:00:00Z", "Asia/Kolkata", "00:00"), "2026-08-22");
    }

    #[test]
    fn a_late_night_sale_books_to_the_previous_business_day_with_a_day_start() {
        // The ADR-018 §9.2 worked example: 01:30 local with day_start_time
        // '04:00' books to the PREVIOUS date. Same instant as above.
        assert_eq!(compute("2026-08-21T20:00:00Z", "Asia/Kolkata", "04:00"), "2026-08-21");
    }

    #[test]
    fn a_sale_after_day_start_books_to_the_same_local_date() {
        // 2026-08-22T05:00:00Z = 10:30 IST, well after a 04:00 day-start.
        assert_eq!(compute("2026-08-22T05:00:00Z", "Asia/Kolkata", "04:00"), "2026-08-22");
    }

    #[test]
    fn iana_identifiers_are_resolved_not_hard_coded_offsets() {
        // UTC and IST disagree on the calendar date for this instant; a
        // hard-coded +05:30 offset would happen to get this one right too,
        // so this test's real job is exercising a second, non-Kolkata zone
        // through the same code path (America/New_York, UTC-4 in August, no
        // day-start correction needed since it does not cross midnight).
        assert_eq!(
            compute("2026-08-21T02:00:00Z", "America/New_York", "00:00"),
            "2026-08-20"
        );
        assert_eq!(compute("2026-08-21T02:00:00Z", "Asia/Kolkata", "00:00"), "2026-08-21");
    }

    /// FALSIFICATION target for the structural fix: an unresolvable
    /// timezone must be REJECTED, never silently substituted. This is the
    /// replacement for the old `an_unresolvable_timezone_falls_back_to_utc`
    /// test — that test asserted the exact defect this rewrite removes, so
    /// its assertion is now inverted rather than merely deleted.
    #[test]
    fn an_unresolvable_timezone_is_rejected_not_silently_substituted() {
        let err = OutletTimezone::parse("Not/AZone");
        assert!(
            matches!(err, Err(DbError::InvalidInput(_))),
            "an invalid IANA identifier must be a typed, propagated error: {err:?}"
        );
    }

    #[test]
    fn a_malformed_day_start_time_is_rejected_not_silently_substituted() {
        let err = DayStartTime::parse("garbage");
        assert!(
            matches!(err, Err(DbError::InvalidInput(_))),
            "a malformed HH:MM value must be a typed, propagated error: {err:?}"
        );
    }

    #[test]
    fn a_valid_timezone_and_day_start_time_construct_successfully() {
        assert!(OutletTimezone::parse("Asia/Kolkata").is_ok());
        assert!(OutletTimezone::parse("America/New_York").is_ok());
        assert!(DayStartTime::parse("00:00").is_ok());
        assert!(DayStartTime::parse("23:59").is_ok());
    }
}
