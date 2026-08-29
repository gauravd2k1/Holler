//! GRN numbering from the edge-local `grn_sequence` counter (contracts
//! 0.6.0, `packages/contracts/sqlite/0028_grn_sequence.sql`).
//!
//! ============================================================================
//! THE COUNTER NEVER LEAVES THE OUTLET
//! ============================================================================
//!
//! The `invoice_sequence` precedent (ADR-016), restated by ADR-019: the
//! ISSUED NUMBER travels on the `goods_receipt_note`; the COUNTER that
//! produced it is SQLite-only, has no `AggregateType` and no sync direction,
//! ever. Mirroring it would make the cloud a second minter of a number the
//! outlet issues, which the single-authority rule forbids.
//!
//! ============================================================================
//! THE NUMBER MUST CARRY THE DATE THE COUNTER IS KEYED BY
//! ============================================================================
//!
//! `grn_sequence` is keyed `(outlet_id, business_date)`, so the counter
//! restarts at 1 every outlet-local business day, while
//! `goods_receipt_note` is `UNIQUE (outlet_id, grn_number)` FOREVER. A
//! format without a date token therefore issues `GRN-0001` again tomorrow
//! and collides.
//!
//! **That is not hypothetical — it is a live M3 defect being avoided here
//! rather than repeated.** `invoice_series.reset_policy` with a prefix that
//! lacks a matching date token yields duplicate invoice numbers, caught only
//! by the UNIQUE index (CLAUDE.md, M3 defects filed to M6). The format below
//! embeds the business date the counter is keyed by, so the two cannot
//! disagree, and `the_number_embeds_the_business_date_the_counter_resets_on`
//! falsifies it.
//!
//! **1-BASED, never 0-based** (the 0.5.8 lesson): `next_value` starts at 1
//! and the migration's own CHECK enforces it. A 0-based counter skips every
//! outlet's first document, permanently and silently.
//!
//! ============================================================================
//! WHAT THIS MODULE DELIBERATELY DOES NOT MINT
//! ============================================================================
//!
//! `purchase_return.return_number` and `stock_transfer_out.transfer_number`.
//! Contracts 0.6.0 ships a counter for the GRN and none for either of those,
//! so this crate takes their numbers from the caller rather than inventing a
//! counter the contract does not model — a `MAX(number) + 1` derivation is
//! the defect `stock_ledger_sequence` was created to remove (a derived
//! counter restarts once rows are removed). Reported as a contract gap.

use rusqlite::{params, Transaction};

use crate::error::DbResult;

/// The document prefix. Distinct from the invoice prefix on purpose: a
/// receipt and a bill are minted by different counters, and a shared prefix
/// would make two unrelated series look like one to a human reading them.
const GRN_PREFIX: &str = "GRN";

/// Zero-padding width for the within-day ordinal. Four digits carries 9999
/// receipts in one business day at one outlet; past that the format GROWS
/// rather than truncating, so a busy central kitchen produces
/// `GRN/20260829/10000` and never a duplicate.
const ORDINAL_WIDTH: usize = 4;

/// Atomically advances this outlet's `grn_sequence` for `business_date` and
/// returns the NEW value, inside the SAME transaction as the receipt that
/// will use it.
///
/// The atomicity argument is `next_invoice_sequence_value`'s, unchanged: a
/// crash before this statement leaves the counter untouched; a crash after
/// it but before `COMMIT` reverts it along with the receipt that would have
/// used it; a crash after `COMMIT` makes both durable together. There is no
/// window in which a number is consumed without the receipt it belongs to
/// also landing.
pub(crate) fn next_grn_sequence_value(
    tx: &Transaction,
    outlet_id: &str,
    business_date: &str,
) -> DbResult<i64> {
    tx.query_row(
        "INSERT INTO grn_sequence (outlet_id, business_date, next_value)
         VALUES (?1, ?2, 1)
         ON CONFLICT(outlet_id, business_date) DO UPDATE SET
            next_value = grn_sequence.next_value + 1
         RETURNING next_value",
        params![outlet_id, business_date],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

/// Renders the issued GRN number. Pure formatting, separated from the
/// counter advance above so the format can be falsified without a database.
pub(crate) fn format_grn_number(business_date: &str, ordinal: i64) -> String {
    let compact_date: String = business_date
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    format!(
        "{GRN_PREFIX}/{compact_date}/{ordinal:0width$}",
        width = ORDINAL_WIDTH
    )
}

/// Mints the next GRN number for one outlet-local business day.
pub(crate) fn next_grn_number(
    tx: &Transaction,
    outlet_id: &str,
    business_date: &str,
) -> DbResult<String> {
    let ordinal = next_grn_sequence_value(tx, outlet_id, business_date)?;
    Ok(format_grn_number(business_date, ordinal))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procurement::testsupport::seed_outlet;
    use crate::Db;

    #[test]
    fn the_counter_is_one_based_and_monotonic_within_a_business_day() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");
        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");

        let first = next_grn_sequence_value(&tx, "outlet-1", "2026-08-29").expect("first");
        let second = next_grn_sequence_value(&tx, "outlet-1", "2026-08-29").expect("second");
        assert_eq!(
            first, 1,
            "a 0-based counter skips the outlet's first receipt, permanently and silently"
        );
        assert_eq!(second, 2);
    }

    #[test]
    fn the_counter_restarts_per_outlet_and_per_business_day() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");
        seed_outlet(db.connection(), "outlet-2");
        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");

        assert_eq!(
            next_grn_sequence_value(&tx, "outlet-1", "2026-08-29").expect("a"),
            1
        );
        assert_eq!(
            next_grn_sequence_value(&tx, "outlet-1", "2026-08-29").expect("b"),
            2
        );
        assert_eq!(
            next_grn_sequence_value(&tx, "outlet-1", "2026-08-30").expect("next day"),
            1,
            "the counter is keyed by business_date and restarts with it"
        );
        assert_eq!(
            next_grn_sequence_value(&tx, "outlet-2", "2026-08-29").expect("other outlet"),
            1,
            "one outlet's receipts never consume another outlet's numbers"
        );
    }

    /// FALSIFICATION of the M3 duplicate-number defect this module exists
    /// not to repeat: because the counter resets per business day, a format
    /// WITHOUT the date token would issue the same string twice. Asserting
    /// the date is present is asserting the collision cannot happen.
    #[test]
    fn the_number_embeds_the_business_date_the_counter_resets_on() {
        let today = format_grn_number("2026-08-29", 1);
        let tomorrow = format_grn_number("2026-08-30", 1);
        assert_eq!(today, "GRN/20260829/0001");
        assert_eq!(tomorrow, "GRN/20260830/0001");
        assert_ne!(
            today, tomorrow,
            "the counter restarts daily, so a number without a date token collides"
        );
    }

    #[test]
    fn the_ordinal_grows_rather_than_truncating_past_the_pad_width() {
        assert_eq!(format_grn_number("2026-08-29", 9999), "GRN/20260829/9999");
        assert_eq!(
            format_grn_number("2026-08-29", 10_000),
            "GRN/20260829/10000"
        );
    }

    /// A number consumed by a transaction that never commits is returned to
    /// the counter. That is the property that makes the series meaningful:
    /// a gap can only mean "a receipt was rolled back", never "two receipts
    /// share a number".
    #[test]
    fn an_uncommitted_mint_does_not_consume_a_number() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");
        {
            let conn = db.connection_mut();
            let tx = conn.transaction().expect("begin");
            assert_eq!(
                next_grn_sequence_value(&tx, "outlet-1", "2026-08-29").expect("mint"),
                1
            );
            // Dropped without commit: rusqlite rolls back, exactly as a
            // process death before COMMIT does.
        }
        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin again");
        assert_eq!(
            next_grn_sequence_value(&tx, "outlet-1", "2026-08-29").expect("re-mint"),
            1,
            "an uncommitted mint must not consume a number"
        );
    }
}
