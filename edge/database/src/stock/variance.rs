//! Variance reporting for a stock count (Milestone 4, track T3, ADR-018).
//! Actual (counted) vs Theoretical (expected), as quantity and a
//! basis-point percentage — DERIVED, never stored as authoritative: the
//! ledger is the only source of stock (0016's header, restated at the top
//! of every M4 file that touches it).
//!
//! **`stock_deduction_gap` appears as a NAMED TERM, never folded into
//! shrinkage** (ADR-018 §10.1). A gap carries no `inventory_item_id` — that
//! is the entire reason it exists, nothing was resolved to an ingredient —
//! so it cannot be attributed to any one variance line. It is reported
//! standalone as `sales_unaccounted`, cumulative through the count's own
//! `business_date` (matching the cumulative-since-forever posture of
//! `expected_quantity_micro` itself), so a reader can distinguish "N sales
//! this could not explain" from "the rest is unexplained shrinkage".

use rusqlite::Transaction;

use crate::error::{DbError, DbResult};
use crate::inventory::round_ratio_half_away_from_zero;
use crate::model::{StockCountVarianceLine, StockCountVarianceReport};
use crate::repo;

/// One basis point per `1/10000`, matching the `rate_bps` convention
/// `packages/contracts/sqlite/0006_m3_billing.sql` already establishes for
/// "integer basis points, never a float and never a percentage string".
const BPS_SCALE: i128 = 10_000;

/// Builds the variance report for a COMPLETED count. Rejects with
/// [`DbError::StockCountNotOpen`] — same variant, inverted sense: a report
/// over an OPEN count would describe lines whose `counted_quantity_micro`
/// may still change, which is not a report, it is a preview of one. Callers
/// wanting to preview an in-progress count should read the count's lines
/// directly (`crate::stock::count::list_count_lines`), not this function.
pub(crate) fn build_variance_report(
    tx: &Transaction,
    stock_count_id: &str,
    outlet_id: &str,
) -> DbResult<StockCountVarianceReport> {
    let count =
        repo::get_stock_count(tx, stock_count_id)?.ok_or(DbError::NotFound("stock_count"))?;
    if count.status != "COMPLETED" {
        return Err(DbError::StockCountNotOpen {
            stock_count_id: stock_count_id.to_string(),
            status: count.status,
        });
    }

    let lines = repo::list_stock_count_lines(tx, stock_count_id)?
        .into_iter()
        .map(|line| {
            let variance_quantity_micro =
                line.counted_quantity_micro - line.expected_quantity_micro;
            let variance_percentage_bps = if line.expected_quantity_micro == 0 {
                None
            } else {
                Some(round_ratio_half_away_from_zero(
                    i128::from(variance_quantity_micro) * BPS_SCALE,
                    i128::from(line.expected_quantity_micro.abs()),
                ) as i64)
            };
            StockCountVarianceLine {
                inventory_item_id: line.inventory_item_id,
                inventory_item_name: line.inventory_item_name,
                dimension: line.dimension,
                counted_quantity_micro: line.counted_quantity_micro,
                expected_quantity_micro: line.expected_quantity_micro,
                variance_quantity_micro,
                variance_percentage_bps,
            }
        })
        .collect();

    let sales_unaccounted =
        repo::sum_unaccounted_sales_through_business_date(tx, outlet_id, &count.business_date)?;

    Ok(StockCountVarianceReport {
        stock_count_id: stock_count_id.to_string(),
        business_date: count.business_date,
        lines,
        sales_unaccounted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::grams;
    use crate::model::{InventoryItem, NewStockCount, NewStockCountLine, Outlet};
    use crate::stock::count::{add_or_update_count_line, complete_stock_count, open_stock_count};
    use crate::Db;

    fn seed_outlet_and_item(conn: &rusqlite::Connection) {
        repo::upsert_outlet(
            conn,
            &Outlet {
                id: "outlet-1".to_string(),
                brand_id: "brand-1".to_string(),
                name: "Test Outlet".to_string(),
                timezone: "Asia/Kolkata".to_string(),
                config_version: 1,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )
        .expect("seed outlet");
        repo::upsert_inventory_item(
            conn,
            &InventoryItem {
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
    }

    /// Variance checked against an INDEPENDENTLY computed figure, not the
    /// same expression that produced it (the M3 discount/tax precedent):
    /// expected = 5,000g theoretical (posted as a manual ledger entry, the
    /// same way `deduction::ledger`'s own theoretical consumption would
    /// arrive); counted = 4,750g. Variance quantity = -250g by hand
    /// arithmetic; percentage by hand = -250/5000 = -0.05 = -500 bps —
    /// computed here with a calculator, not by re-running this module's own
    /// formula.
    #[test]
    fn variance_matches_an_independently_computed_figure() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet_and_item(db.connection());

        // Seed 5000g of theoretical stock via a manual ledger entry BEFORE
        // the count opens, so `expected_quantity_micro` snapshots exactly
        // that.
        {
            let conn = db.connection_mut();
            let tx = conn.transaction().expect("begin");
            let entry = crate::model::NewStockLedgerEntry {
                outlet_id: "outlet-1".to_string(),
                inventory_item_id: "item-1".to_string(),
                inventory_item_name: "Paneer".to_string(),
                dimension: "MASS".to_string(),
                entry_type: "PURCHASE".to_string(),
                origin: "MANUAL".to_string(),
                quantity_applied_micro: grams(5_000),
                recipe_id: None,
                recipe_version: None,
                recipe_name: None,
                source_order_id: None,
                source_order_item_id: None,
                reason_code: None,
                note: None,
                occurred_at: "2026-08-19T09:00:00Z".to_string(),
                business_date: "2026-08-19".to_string(),
                created_by_user_id: None,
                modifier_delta_id: None,
                modifier_name: None,
                modifier_delta_version: None,
                source_stock_count_id: None,
            };
            let seq =
                repo::next_stock_ledger_sequence_value(&tx, "outlet-1", "2026-08-19T09:00:00Z")
                    .expect("mint seq");
            let id = uuid::Uuid::now_v7().to_string();
            crate::deduction::ledger::insert_stock_ledger_entry(&tx, &id, seq, &entry)
                .expect("insert purchase");
            tx.commit().expect("commit");
        }
        // Assert the fixture landed before trusting anything downstream of it.
        let seeded = repo::get_current_stock(db.connection(), "outlet-1", "item-1").expect("read");
        assert_eq!(
            seeded,
            grams(5_000),
            "fixture purchase must actually be on the ledger"
        );

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        open_stock_count(
            &tx,
            NewStockCount {
                id: "count-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                started_at: "2026-08-20T22:10:00Z".to_string(),
                counted_by_user_id: Some("user-1".to_string()),
                note: None,
            },
        )
        .expect("open");
        let line = add_or_update_count_line(
            &tx,
            "count-1",
            "outlet-1",
            NewStockCountLine {
                inventory_item_id: "item-1".to_string(),
                counted_quantity_micro: grams(4_750),
                note: None,
            },
        )
        .expect("add line");
        assert_eq!(line.expected_quantity_micro, grams(5_000));
        complete_stock_count(&tx, "count-1", "outlet-1", "2026-08-20T22:41:00Z").expect("complete");

        let report = build_variance_report(&tx, "count-1", "outlet-1").expect("report");
        tx.commit().expect("commit");

        assert_eq!(report.lines.len(), 1);
        let line = &report.lines[0];
        assert_eq!(
            line.variance_quantity_micro,
            -grams(250),
            "hand-computed: 4750 - 5000 = -250 g"
        );
        assert_eq!(
            line.variance_percentage_bps,
            Some(-500),
            "hand-computed: -250 / 5000 = -0.05 = -500 bps"
        );
    }

    #[test]
    fn a_zero_expected_quantity_reports_no_percentage() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet_and_item(db.connection());

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        open_stock_count(
            &tx,
            NewStockCount {
                id: "count-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                started_at: "2026-08-20T10:00:00Z".to_string(),
                counted_by_user_id: None,
                note: None,
            },
        )
        .expect("open");
        add_or_update_count_line(
            &tx,
            "count-1",
            "outlet-1",
            NewStockCountLine {
                inventory_item_id: "item-1".to_string(),
                counted_quantity_micro: grams(200),
                note: None,
            },
        )
        .expect("add line against zero theoretical stock");
        complete_stock_count(&tx, "count-1", "outlet-1", "2026-08-20T10:05:00Z").expect("complete");

        let report = build_variance_report(&tx, "count-1", "outlet-1").expect("report");
        assert_eq!(
            report.lines[0].variance_percentage_bps, None,
            "a percentage of zero theoretical stock is undefined, not zero"
        );
        assert_eq!(report.lines[0].variance_quantity_micro, grams(200));
    }

    #[test]
    fn a_report_over_an_open_count_is_rejected() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet_and_item(db.connection());

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        open_stock_count(
            &tx,
            NewStockCount {
                id: "count-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                started_at: "2026-08-20T10:00:00Z".to_string(),
                counted_by_user_id: None,
                note: None,
            },
        )
        .expect("open");

        let err = build_variance_report(&tx, "count-1", "outlet-1")
            .expect_err("a report over a still-OPEN count must be rejected");
        assert!(matches!(err, DbError::StockCountNotOpen { .. }));
    }
}
