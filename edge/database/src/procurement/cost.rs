//! Weighted average cost, DERIVED from the stock ledger (ADR-018 §8,
//! ADR-019, docs/m5-planning.md §2.2 rule 5).
//!
//! ============================================================================
//! COST LIVES ON THE LEDGER ENTRY, NEVER ON `inventory_item`
//! ============================================================================
//!
//! A cost column on a cloud-owned config row is the half-config,
//! half-transaction row ADR-011 forbids: the edge would be writing a number
//! onto an aggregate the cloud owns. So there is no stored average anywhere.
//! This module re-derives it on every call from the entries that carry a
//! cost, exactly as `crate::stock::snapshot` re-derives a balance.
//!
//! ============================================================================
//! WHAT IS AVERAGED, AND WHAT IS NOT
//! ============================================================================
//!
//! Only INBOUND entries with a recorded cost: `quantity_applied_micro > 0`
//! AND `unit_cost_paise IS NOT NULL`. That is receipts (`PURCHASE`) and any
//! future inbound movement that records what it was worth.
//!
//! Consumption, wastage and count adjustments are deliberately excluded even
//! when they carry a cost, because they consume at the average rather than
//! setting it — including an outbound row would let the act of issuing stock
//! move the purchase price, which is not what a purchase-weighted average
//! means. A count adjustment carries no cost at all today, which is why the
//! `unit_cost_paise IS NOT NULL` half is a real filter and not decoration.
//!
//! ============================================================================
//! NO FLOAT
//! ============================================================================
//!
//! `sum(quantity x cost) / sum(quantity)` accumulates in `i128` and rounds
//! HALF AWAY FROM ZERO exactly once, at the end — the ADR-018 §5 rule the
//! quantity path already follows, applied to money. The two sums are taken
//! in SQLite as `INTEGER` totals, then divided here rather than in SQL,
//! because SQLite's `/` on two integers truncates toward zero and would
//! quietly bias every average downward.

use rusqlite::{params, Connection};

use crate::error::DbResult;
use crate::inventory::round_ratio_half_away_from_zero;
use crate::procurement::convert::MICRO;

/// The weighted average cost of one item at one outlet, in integer paise
/// per BASE unit, or `None` when this outlet has never recorded a costed
/// receipt for it.
///
/// **`None` is not zero.** A caller that needs a number for a row it is
/// about to write must decide what an unpriced item is worth; silently
/// valuing it at nothing would put a free issue on the books and make the
/// figure look computed. `crate::procurement::movement` treats `None` as
/// zero ONLY after the caller has declined to supply a price, and says so.
pub(crate) fn weighted_average_cost_paise(
    conn: &Connection,
    outlet_id: &str,
    inventory_item_id: &str,
) -> DbResult<Option<i64>> {
    let (total_quantity_micro, total_value): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(quantity_applied_micro), 0),
                COALESCE(SUM(line_total_paise), 0)
         FROM stock_ledger_entry
         WHERE outlet_id = ?1
           AND inventory_item_id = ?2
           AND line_total_paise IS NOT NULL
           AND quantity_applied_micro > 0",
        params![outlet_id, inventory_item_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    if total_quantity_micro <= 0 {
        return Ok(None);
    }

    // SCALE. `line_total_paise` is money; `quantity_applied_micro` is
    // MICRO-units. The old `SUM(quantity x rate)` form cancelled the 10^6
    // implicitly because the rate was already per BASE unit -- summing money
    // directly does not, so the numerator carries the factor explicitly or
    // every average comes back a millionth of its true size. The i128
    // accumulator is what makes that multiplication safe.
    let rounded = round_ratio_half_away_from_zero(
        i128::from(total_value).saturating_mul(i128::from(MICRO)),
        i128::from(total_quantity_micro),
    );
    Ok(i64::try_from(rounded).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{grams, kilograms};
    use crate::model::NewStockLedgerEntry;
    use crate::procurement::testsupport::{seed_inventory_item, seed_outlet};
    use crate::Db;

    const OUTLET: &str = "outlet-1";
    const RICE: &str = "item-rice";

    /// Posts with the total DERIVED from the rate. Only sound for fixtures
    /// whose numbers divide evenly -- which is exactly why it cannot be used
    /// for the precision test below.
    fn post(db: &mut Db, quantity_micro: i64, unit_cost_paise: Option<i64>, entry_type: &str) {
        let derived = unit_cost_paise
            .filter(|_| quantity_micro > 0)
            .map(|c| (i128::from(quantity_micro) * i128::from(c) / 1_000_000) as i64);
        post_with_total(db, quantity_micro, unit_cost_paise, derived, entry_type)
    }

    /// Posts a row stating BOTH figures independently, the way a receipt does:
    /// the invoiced total is the fact, the rate is derived from it.
    fn post_with_total(
        db: &mut Db,
        quantity_micro: i64,
        unit_cost_paise: Option<i64>,
        line_total_paise: Option<i64>,
        entry_type: &str,
    ) {
        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        let entry = NewStockLedgerEntry {
            outlet_id: OUTLET.to_string(),
            inventory_item_id: RICE.to_string(),
            inventory_item_name: "Rice".to_string(),
            dimension: "MASS".to_string(),
            entry_type: entry_type.to_string(),
            // The origin a row of this entry_type really carries in
            // production (contracts 0.6.2). A PURCHASE row posted by the
            // receipt path says GOODS_RECEIPT, not MANUAL; a fixture that
            // said MANUAL for every row would be averaging over data no
            // shipping path produces.
            origin: match entry_type {
                "PURCHASE" => "GOODS_RECEIPT",
                _ => "MANUAL",
            }
            .to_string(),
            quantity_applied_micro: quantity_micro,
            recipe_id: None,
            recipe_version: None,
            recipe_name: None,
            source_order_id: None,
            source_order_item_id: None,
            reason_code: None,
            note: None,
            occurred_at: "2026-08-29T10:00:00Z".to_string(),
            business_date: "2026-08-29".to_string(),
            created_by_user_id: None,
            modifier_delta_id: None,
            modifier_name: None,
            modifier_delta_version: None,
            unit_cost_paise,
            line_total_paise,
            source_stock_count_id: None,
            source_grn_id: None,
            source_purchase_return_id: None,
            source_stock_transfer_out_id: None,
        };
        crate::deduction::ledger::insert_stock_ledger_entry_with_next_seq(
            &tx,
            OUTLET,
            "2026-08-29T10:00:00Z",
            &entry,
        )
        .expect("post ledger entry");
        tx.commit().expect("commit");
    }

    fn seeded() -> Db {
        let db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), OUTLET);
        seed_inventory_item(db.connection(), RICE, OUTLET, "Rice", "MASS", 1_000_000);
        db
    }

    /// M5 ACCEPTANCE CRITERION 7, and the figure on the right-hand side is
    /// computed BY HAND here, not by calling any function under test:
    ///
    ///   100 kg = 100_000 g at 4 paise/g  ->    400_000 paise
    ///    50 kg =  50_000 g at 5 paise/g  ->    250_000 paise
    ///   ------------------------------------------------------
    ///   650_000 paise over 150_000 g     =  4.333... -> 4 paise/g
    ///
    /// Nothing below reuses the implementation's own arithmetic to state the
    /// expectation.
    #[test]
    fn two_receipts_at_different_prices_match_an_independently_computed_average() {
        let mut db = seeded();
        post(&mut db, kilograms(100), Some(4), "PURCHASE");
        post(&mut db, kilograms(50), Some(5), "PURCHASE");

        // Fixtures must be present before anything is asserted about them.
        let rows: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM stock_ledger_entry WHERE outlet_id = ?1",
                params![OUTLET],
                |r| r.get(0),
            )
            .expect("count entries");
        assert_eq!(rows, 2, "both receipts must have inserted");

        let independent_numerator: i128 = 100_000 * 4 + 50_000 * 5;
        let independent_denominator: i128 = 100_000 + 50_000;
        let independent = independent_numerator / independent_denominator; // 4
        assert_eq!(independent, 4);

        let actual = weighted_average_cost_paise(db.connection(), OUTLET, RICE)
            .expect("read wac")
            .expect("two costed receipts must produce an average");
        assert_eq!(actual as i128, independent);
    }

    /// A second, differently-shaped pair whose exact average is NOT an
    /// integer in the other direction, so the rounding rule itself is
    /// exercised rather than a case that divides evenly.
    #[test]
    fn the_average_rounds_half_away_from_zero_exactly_once() {
        let mut db = seeded();
        // 1 g at 1 paise + 1 g at 2 paise = 1.5 -> 2 (half away from zero).
        post(&mut db, grams(1), Some(1), "PURCHASE");
        post(&mut db, grams(1), Some(2), "PURCHASE");

        let actual = weighted_average_cost_paise(db.connection(), OUTLET, RICE)
            .expect("read wac")
            .expect("average");
        assert_eq!(actual, 2, "1.5 rounds away from zero, never truncates to 1");
    }

    /// SQLite integer division truncates. If the two sums were divided in
    /// SQL, the case above would silently return 1. Falsified here by
    /// computing the truncating answer explicitly and asserting the module
    /// does not produce it.
    #[test]
    fn sql_side_integer_division_would_have_biased_the_average_downward() {
        let mut db = seeded();
        post(&mut db, grams(1), Some(1), "PURCHASE");
        post(&mut db, grams(1), Some(2), "PURCHASE");

        let truncating: i64 = db
            .connection()
            .query_row(
                "SELECT SUM(quantity_applied_micro * unit_cost_paise) \
                 / SUM(quantity_applied_micro) \
                 FROM stock_ledger_entry WHERE outlet_id = ?1 AND inventory_item_id = ?2 \
                   AND unit_cost_paise IS NOT NULL AND quantity_applied_micro > 0",
                params![OUTLET, RICE],
                |r| r.get(0),
            )
            .expect("truncating average");
        assert_eq!(truncating, 1, "this is the wrong answer SQL would give");
        assert_eq!(
            weighted_average_cost_paise(db.connection(), OUTLET, RICE)
                .expect("read")
                .expect("average"),
            2
        );
    }

    /// Outbound movements consume at the average, they do not set it.
    #[test]
    fn consumption_and_uncosted_entries_do_not_move_the_average() {
        let mut db = seeded();
        post(&mut db, kilograms(100), Some(4), "PURCHASE");
        post(&mut db, -kilograms(10), Some(999), "CONSUMPTION");
        post(&mut db, kilograms(10), None, "ADJUSTMENT");

        assert_eq!(
            weighted_average_cost_paise(db.connection(), OUTLET, RICE)
                .expect("read")
                .expect("average"),
            4,
            "only costed inbound entries set the purchase-weighted average"
        );
    }

    /// An item this outlet has never bought has NO average — not zero.
    #[test]
    fn an_item_with_no_costed_receipt_has_no_average_rather_than_zero() {
        let db = seeded();
        assert_eq!(
            weighted_average_cost_paise(db.connection(), OUTLET, RICE).expect("read"),
            None
        );
    }

    /// One outlet's purchases never price another outlet's stock.
    #[test]
    fn the_average_is_scoped_to_one_outlet() {
        let mut db = seeded();
        seed_outlet(db.connection(), "outlet-2");
        post(&mut db, kilograms(100), Some(4), "PURCHASE");
        assert_eq!(
            weighted_average_cost_paise(db.connection(), "outlet-2", RICE).expect("read"),
            None
        );
    }
}
