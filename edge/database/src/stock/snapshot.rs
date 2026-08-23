//! `stock_balance_snapshot` sealing and the bounded current-stock read
//! (Milestone 4, track T3, ADR-018 §9/§9.1).
//!
//! ============================================================================
//! SEALING NEVER DEPENDS ON AN OPERATOR
//! ============================================================================
//!
//! [`seal_unsealed_business_days`] is called from [`crate::Db::open`] and
//! [`crate::Db::open_in_memory_for_tests`] — on every open, unconditionally,
//! **before the `Db` handle is returned to any caller that could serve a
//! stock read**. Day-end close may ALSO call it (harmless — see below), but
//! nothing in this crate makes sealing conditional on that ever having
//! happened. An outlet that never triggers day-end close, or a POS that
//! dies at 11pm every night for a month, still gets every prior day sealed
//! the next time the app opens.
//!
//! **Idempotent.** [`seal_one_business_day`] seals via `repo::seal_snapshot`,
//! an `INSERT OR IGNORE` against the table's own primary key
//! `(outlet_id, inventory_item_id, business_date)` — a collision is a
//! no-op, never an `UPDATE` (the 0017 triggers would abort an `UPDATE`
//! outright; this function is never expected to hit them, and if it ever
//! does, that is this code's bug, not the constraint's, per the task
//! brief). Calling this twice for the same day, from two separate `Db::open`
//! calls, writes the same rows once.
//!
//! **Lazily caught up.** [`repo::find_unsealed_item_days_before`] discovers
//! every `(item, business_date)` pair with ledger activity strictly before
//! "today" that has no sealed row yet — not "yesterday", not "since the
//! last seal": EVERY prior unsealed day, however many there are. Days are
//! sealed in ascending order so a later day's closing balance is always
//! computed against an already-consistent earlier one (though
//! [`repo::compute_seal_for_item_day`] recomputes a full sum rather than
//! chaining off the prior seal, so this ordering is a belt-and-braces
//! property, not a correctness dependency).
//!
//! ============================================================================
//! THE BOUNDED READ: entry_seq, NEVER business_date
//! ============================================================================
//!
//! [`get_current_stock`] is `closing_quantity_micro + SUM(quantity_applied_micro)
//! WHERE entry_seq > through_entry_seq` — ADR-018 §9's formula, verbatim, and
//! never `business_date > business_date`. The reason is the whole point of
//! this file: an entry that ARRIVES after its day is sealed while CARRYING
//! that day's `business_date` (a count started 23:40, completed 00:15 — see
//! `crate::stock::count`) would be absent from the seal (it did not exist at
//! seal time) and excluded by a date predicate (too old), vanishing
//! permanently and silently, since a seal is never re-issued. Selecting by
//! the mark makes that late arrival self-heal into the very next read
//! instead.
use chrono::{DateTime, Utc};
use rusqlite::Transaction;

use crate::error::DbResult;
use crate::repo;

/// The bounded stock read every write in this module (and T5's low-stock
/// surfacing) reads through. `&Transaction`, not `&Connection`, because
/// every caller in this crate needs it inside the same transaction as the
/// write it informs (e.g. a count line's `expected_quantity_micro`) —
/// `repo::get_current_stock` itself only needs `&Connection`, and
/// `rusqlite::Transaction` derefs to it.
pub(crate) fn get_current_stock_in_tx(
    tx: &Transaction,
    outlet_id: &str,
    inventory_item_id: &str,
) -> DbResult<i64> {
    repo::get_current_stock(tx, outlet_id, inventory_item_id)
}

/// Seals one `(outlet, item, business_date)` — computing the item's closing
/// balance and high-water mark up to and including that day, then writing
/// it via `repo::seal_snapshot`'s idempotent `INSERT OR IGNORE`. `Ok(())`
/// silently on a day/item combination that turns out to have no ledger
/// activity at all (defensive: `repo::find_unsealed_item_days_before` should
/// never hand this function a pair with nothing to seal, but this function
/// does not trust that invariant blindly — it re-derives from the data
/// rather than assuming its caller's query was exhaustive).
fn seal_one_business_day(
    tx: &Transaction,
    outlet_id: &str,
    inventory_item_id: &str,
    business_date: &str,
    sealed_at: &str,
) -> DbResult<()> {
    let Some((dimension, mark, closing)) =
        repo::compute_seal_for_item_day(tx, outlet_id, inventory_item_id, business_date)?
    else {
        return Ok(());
    };
    repo::seal_snapshot(
        tx,
        outlet_id,
        inventory_item_id,
        business_date,
        closing,
        &dimension,
        mark,
        sealed_at,
    )
}

/// The catch-up entry point (ADR-018 §9.1). For every outlet, computes
/// "today" from that outlet's own `timezone`/`day_start_time`, finds every
/// `(item, business_date)` pair with unsealed activity strictly before that
/// day, and seals each — in ascending date order, so a reader tracing this
/// crate's output sees the same order the invariant test asserts
/// ("skip three business days, reopen, assert three snapshots exist").
/// `now` is the caller's own clock reading (`Db::open` passes
/// `chrono::Utc::now()`; tests pass a fixed instant) — never read from the
/// system clock inside this function, so the whole operation is
/// deterministic given its inputs.
pub(crate) fn seal_unsealed_business_days(tx: &Transaction, now: DateTime<Utc>) -> DbResult<()> {
    let sealed_at = now.to_rfc3339();
    for outlet_id in repo::list_all_outlet_ids(tx)? {
        let (timezone, day_start_time) = repo::get_outlet_business_date_config(tx, &outlet_id)?;
        let today =
            crate::deduction::business_date::compute_business_date(now, &timezone, &day_start_time);
        for (inventory_item_id, business_date) in
            repo::find_unsealed_item_days_before(tx, &outlet_id, &today)?
        {
            seal_one_business_day(
                tx,
                &outlet_id,
                &inventory_item_id,
                &business_date,
                &sealed_at,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{grams, Dimension};
    use crate::model::{NewStockLedgerEntry, Outlet};
    use crate::Db;

    fn seed_outlet(conn: &rusqlite::Connection, id: &str) {
        repo::upsert_outlet(
            conn,
            &Outlet {
                id: id.to_string(),
                brand_id: "brand-1".to_string(),
                name: "Test Outlet".to_string(),
                timezone: "Asia/Kolkata".to_string(),
                config_version: 1,
                created_at: "2020-01-01T00:00:00Z".to_string(),
                updated_at: "2020-01-01T00:00:00Z".to_string(),
            },
        )
        .expect("seed outlet");
    }

    fn insert_ledger_entry(
        db: &mut Db,
        outlet_id: &str,
        item_id: &str,
        quantity_applied_micro: i64,
        occurred_at: &str,
        business_date: &str,
    ) {
        let entry = NewStockLedgerEntry {
            outlet_id: outlet_id.to_string(),
            inventory_item_id: item_id.to_string(),
            inventory_item_name: "Paneer".to_string(),
            dimension: Dimension::Mass.as_str().to_string(),
            entry_type: "ADJUSTMENT".to_string(),
            origin: "MANUAL".to_string(),
            quantity_applied_micro,
            recipe_id: None,
            recipe_version: None,
            recipe_name: None,
            source_order_id: None,
            source_order_item_id: None,
            reason_code: None,
            note: None,
            occurred_at: occurred_at.to_string(),
            business_date: business_date.to_string(),
            created_by_user_id: None,
            modifier_delta_id: None,
            modifier_name: None,
            modifier_delta_version: None,
            source_stock_count_id: None,
        };
        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin tx");
        let seq = repo::next_stock_ledger_sequence_value(&tx, outlet_id, occurred_at)
            .expect("mint entry_seq");
        let id = uuid::Uuid::now_v7().to_string();
        crate::deduction::ledger::insert_stock_ledger_entry(&tx, &id, seq, &entry)
            .expect("insert ledger entry");
        tx.commit().expect("commit");
    }

    /// The T6 sealing invariant, §66-falsified here rather than merely
    /// asserted: skip three business days of activity, reopen (simulated by
    /// calling `seal_unsealed_business_days` directly, since a real
    /// `Db::open` round-trip would re-encrypt to disk), assert three
    /// snapshots exist and the resulting balance equals a full-ledger sum.
    #[test]
    fn skipping_three_business_days_then_catching_up_seals_all_three() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");
        repo::upsert_inventory_item(
            db.connection(),
            &crate::model::InventoryItem {
                id: "item-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                sku: "PANEER-1KG".to_string(),
                name: "Paneer".to_string(),
                category: None,
                dimension: "MASS".to_string(),
                reorder_level_micro: None,
                par_level_micro: None,
                storage_location: None,
                is_active: true,
                yield_factor_ppm: 1_000_000,
                config_version: 1,
            },
        )
        .expect("seed item");

        // Three distinct business days, all in the past relative to any
        // real wall-clock "now" this test could run at.
        insert_ledger_entry(
            &mut db,
            "outlet-1",
            "item-1",
            grams(5_000),
            "2020-01-01T10:00:00Z",
            "2020-01-01",
        );
        insert_ledger_entry(
            &mut db,
            "outlet-1",
            "item-1",
            -grams(1_200),
            "2020-01-02T10:00:00Z",
            "2020-01-02",
        );
        insert_ledger_entry(
            &mut db,
            "outlet-1",
            "item-1",
            -grams(800),
            "2020-01-03T10:00:00Z",
            "2020-01-03",
        );

        // Assert the fixtures actually landed before asserting anything
        // about them — a failed INSERT would make every later assertion
        // trivially pass on zero rows.
        let full_sum_before: i64 = db
            .connection()
            .query_row(
                "SELECT SUM(quantity_applied_micro) FROM stock_ledger_entry \
                 WHERE outlet_id = 'outlet-1' AND inventory_item_id = 'item-1'",
                [],
                |row| row.get(0),
            )
            .expect("sum ledger");
        assert_eq!(
            full_sum_before,
            grams(5_000) - grams(1_200) - grams(800),
            "fixture rows must actually exist before this test asserts sealing behaviour"
        );

        let now = DateTime::parse_from_rfc3339("2020-06-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        {
            let conn = db.connection_mut();
            let tx = conn.transaction().expect("begin");
            seal_unsealed_business_days(&tx, now).expect("catch up sealing");
            tx.commit().expect("commit");
        }

        let snapshot_count: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM stock_balance_snapshot \
                 WHERE outlet_id = 'outlet-1' AND inventory_item_id = 'item-1'",
                [],
                |row| row.get(0),
            )
            .expect("count snapshots");
        assert_eq!(snapshot_count, 3, "one snapshot per skipped business day");

        let current =
            repo::get_current_stock(db.connection(), "outlet-1", "item-1").expect("bounded read");
        assert_eq!(
            current, full_sum_before,
            "the bounded read must equal a full-ledger sum after catch-up sealing"
        );
    }

    /// Falsifies the reason `entry_seq`, not `business_date`, drives the
    /// read: seal a day, then insert an entry carrying that ALREADY-SEALED
    /// day's business_date (the exact "count started 23:40, completed
    /// 00:15" shape) — and assert the bounded read still includes it.
    #[test]
    fn a_late_arrival_carrying_an_already_sealed_business_date_is_still_read() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");
        repo::upsert_inventory_item(
            db.connection(),
            &crate::model::InventoryItem {
                id: "item-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                sku: "PANEER-1KG".to_string(),
                name: "Paneer".to_string(),
                category: None,
                dimension: "MASS".to_string(),
                reorder_level_micro: None,
                par_level_micro: None,
                storage_location: None,
                is_active: true,
                yield_factor_ppm: 1_000_000,
                config_version: 1,
            },
        )
        .expect("seed item");

        insert_ledger_entry(
            &mut db,
            "outlet-1",
            "item-1",
            grams(5_000),
            "2020-01-01T10:00:00Z",
            "2020-01-01",
        );

        let now = DateTime::parse_from_rfc3339("2020-01-02T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        {
            let conn = db.connection_mut();
            let tx = conn.transaction().expect("begin");
            seal_unsealed_business_days(&tx, now).expect("seal day 1");
            tx.commit().expect("commit");
        }
        let sealed_count: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM stock_balance_snapshot \
                 WHERE outlet_id='outlet-1' AND inventory_item_id='item-1' AND business_date='2020-01-01'",
                [],
                |row| row.get(0),
            )
            .expect("count seal");
        assert_eq!(
            sealed_count, 1,
            "day 1 must actually be sealed before the late arrival is written"
        );

        // A late COUNT_ADJUSTMENT-shaped entry, dated to the ALREADY-SEALED
        // day, arriving after the seal — entry_seq will be higher than the
        // seal's mark, but business_date is the old, sealed day.
        insert_ledger_entry(
            &mut db,
            "outlet-1",
            "item-1",
            -grams(300),
            "2020-01-02T00:15:00Z",
            "2020-01-01",
        );

        let current =
            repo::get_current_stock(db.connection(), "outlet-1", "item-1").expect("bounded read");
        assert_eq!(
            current,
            grams(5_000) - grams(300),
            "a late arrival carrying an already-sealed business_date must still be included \
             by the entry_seq-based read — a business_date predicate would silently drop it"
        );
    }

    #[test]
    fn sealing_twice_is_idempotent_and_never_updates_a_seal() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");
        repo::upsert_inventory_item(
            db.connection(),
            &crate::model::InventoryItem {
                id: "item-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                sku: "PANEER-1KG".to_string(),
                name: "Paneer".to_string(),
                category: None,
                dimension: "MASS".to_string(),
                reorder_level_micro: None,
                par_level_micro: None,
                storage_location: None,
                is_active: true,
                yield_factor_ppm: 1_000_000,
                config_version: 1,
            },
        )
        .expect("seed item");
        insert_ledger_entry(
            &mut db,
            "outlet-1",
            "item-1",
            grams(1_000),
            "2020-01-01T10:00:00Z",
            "2020-01-01",
        );

        let now = DateTime::parse_from_rfc3339("2020-01-02T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        for _ in 0..2 {
            let conn = db.connection_mut();
            let tx = conn.transaction().expect("begin");
            seal_unsealed_business_days(&tx, now).expect("seal (idempotent)");
            tx.commit().expect("commit");
        }

        let snapshot_count: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM stock_balance_snapshot \
                 WHERE outlet_id='outlet-1' AND inventory_item_id='item-1'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(
            snapshot_count, 1,
            "sealing an already-sealed day twice must not create a second row"
        );
    }

    // ------------------------- criterion 7: measured, not asserted --------

    /// Builds one outlet whose item has `sealed_days` business days of
    /// history, every one of them sealed, followed by exactly
    /// `unsealed_entries` entries on an unsealed day. Returns the VM steps
    /// the SHIPPED current-stock read takes over that data.
    ///
    /// One entry per sealed day (rather than many) keeps the two scenarios
    /// differing in exactly one variable: the volume of history behind the
    /// seal.
    fn vm_steps_for_history(sealed_days: usize, unsealed_entries: usize) -> i64 {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");
        repo::upsert_inventory_item(
            db.connection(),
            &crate::model::InventoryItem {
                id: "item-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                sku: "PANEER-1KG".to_string(),
                name: "Paneer".to_string(),
                category: None,
                dimension: "MASS".to_string(),
                reorder_level_micro: None,
                par_level_micro: None,
                storage_location: None,
                is_active: true,
                yield_factor_ppm: 1_000_000,
                config_version: 1,
            },
        )
        .expect("seed item");

        // Sealed history: one entry per day across consecutive days in 2020.
        let start = DateTime::parse_from_rfc3339("2020-01-01T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        for d in 0..sealed_days {
            let day = start + chrono::Duration::days(d as i64);
            insert_ledger_entry(
                &mut db,
                "outlet-1",
                "item-1",
                grams(1),
                &day.to_rfc3339(),
                &day.format("%Y-%m-%d").to_string(),
            );
        }

        // Seal everything above, from a "now" past all of it. Must clear the
        // LONGEST history this helper is called with: 400 days from
        // 2020-01-01 reaches 2021-02-04, and an earlier `now` silently seals
        // only part of it — which the assertion below caught when it did.
        let now = DateTime::parse_from_rfc3339("2021-07-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        {
            let conn = db.connection_mut();
            let tx = conn.transaction().expect("begin");
            seal_unsealed_business_days(&tx, now).expect("seal history");
            tx.commit().expect("commit");
        }

        let sealed_count: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM stock_balance_snapshot WHERE outlet_id = 'outlet-1'",
                [],
                |row| row.get(0),
            )
            .expect("count snapshots");
        assert_eq!(
            sealed_count, sealed_days as i64,
            "every day of history must actually be sealed, or this measures the wrong thing"
        );

        // The unsealed tail, after the seal, on a later day.
        for i in 0..unsealed_entries {
            let at = DateTime::parse_from_rfc3339("2022-01-01T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
                + chrono::Duration::minutes(i as i64);
            insert_ledger_entry(
                &mut db,
                "outlet-1",
                "item-1",
                -grams(1),
                &at.to_rfc3339(),
                "2022-01-01",
            );
        }

        repo::measure_list_current_stock_vm_steps(db.connection(), "outlet-1")
            .expect("measure the shipped read")
    }

    /// **M4 acceptance criterion 7, measured rather than asserted.** The
    /// engine counts the work; nothing here times a clock, so the result is
    /// identical on a fast machine and a 4GB spinning-disk till, and a
    /// regression cannot hide behind a generous timing margin.
    ///
    /// The claim under test is ADR-018 §9's whole reason for the sealed
    /// snapshot: a stock read costs what the UNSEALED tail costs, and is
    /// independent of how much sealed history sits behind it. If the read
    /// ever regressed to scanning history — a dropped `entry_seq >` term, a
    /// date predicate substituted for the mark — this number would climb
    /// with `sealed_days` and the test would fail with both figures named.
    #[test]
    fn stock_reads_stay_bounded_after_a_sealed_snapshot() {
        // Same unsealed tail, two very different volumes of sealed history.
        let short_history = vm_steps_for_history(5, 3);
        let long_history = vm_steps_for_history(400, 3);

        assert!(
            short_history > 0,
            "the measurement itself must do work, or it is measuring nothing"
        );
        assert_eq!(
            long_history, short_history,
            "the bounded read must cost the same with 400 sealed days behind it as with 5. \
             Measured VM steps: {short_history} at 5 sealed days, {long_history} at 400. \
             A number that grows with sealed history means the read is scanning the ledger \
             again rather than reading from the seal (ADR-018 §9)."
        );
    }

    /// The companion that stops the test above from passing vacuously. If
    /// the measurement were insensitive to everything, "cost does not grow
    /// with sealed history" would be true and meaningless. Cost MUST grow
    /// with the unsealed tail, because that is the work the read genuinely
    /// has to do.
    #[test]
    fn the_measurement_does_respond_to_the_unsealed_tail() {
        let small_tail = vm_steps_for_history(5, 3);
        let large_tail = vm_steps_for_history(5, 60);

        assert!(
            large_tail > small_tail,
            "a longer unsealed tail must cost more ({small_tail} steps at 3 entries vs \
             {large_tail} at 60). If these were equal the measurement would be blind, and \
             `stock_reads_stay_bounded_after_a_sealed_snapshot` would prove nothing."
        );
    }
}
