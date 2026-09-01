//! Wastage recording (Milestone 4, track T3, ADR-018 §11). Gated on
//! `inventory.manage` — enforced by the caller, see `crate::stock`'s module
//! doc comment for why that check does not live in this crate.
//!
//! **There is no `wastage` table.** A wastage event is one more
//! `stock_ledger_entry` row (`entry_type='WASTAGE'`, `origin='WASTAGE'`) —
//! 0016's own `entry_type` list names it alongside `CONSUMPTION` and
//! `ADJUSTMENT`. **`wastage.approve` and any approval workflow are
//! deliberately absent** (ADR-018 §11, M5): recording is an append-only fact
//! ("a cook dropped a tray, the stock is gone"), and a mutable approval flag
//! on an append-only row would be a contradiction.

use rusqlite::Transaction;

use crate::error::{DbError, DbResult};
use crate::model::{NewStockLedgerEntry, NewWastageEntry, StockLedgerEntry};
use crate::repo;

use crate::deduction::business_date::compute_business_date;

/// Records one wastage event as a negative `stock_ledger_entry`
/// (`entry_type='WASTAGE'`, `origin='WASTAGE'`). Rejects BEFORE any write if
/// `quantity_micro` is not `> 0`
/// ([`DbError::WastageQuantityNotPositive`]) or `reason_code` is blank
/// ([`DbError::WastageReasonRequired`]) — the same "checked before write,
/// never a silent shortfall with no cause" discipline
/// `close_cash_shift`/`record_paid_in_out` already apply to cash.
///
/// **Stock never blocks this write.** There is no balance check — negative
/// stock after a wastage entry is a variance signal, not an error (ADR-018
/// Rule 1), and this function never queries current stock at all.
pub(crate) fn record_wastage(tx: &Transaction, req: NewWastageEntry) -> DbResult<StockLedgerEntry> {
    if req.quantity_micro <= 0 {
        return Err(DbError::WastageQuantityNotPositive {
            quantity_micro: req.quantity_micro,
        });
    }
    if req.reason_code.trim().is_empty() {
        return Err(DbError::WastageReasonRequired);
    }
    let Some((name, dimension)) = repo::get_inventory_item_snapshot(tx, &req.inventory_item_id)?
    else {
        return Err(DbError::NotFound("inventory_item"));
    };

    let occurred_at = crate::tax::parse_utc(&req.occurred_at)?;
    let (timezone, day_start_time) = repo::get_outlet_business_date_config(tx, &req.outlet_id)?;
    let business_date = compute_business_date(occurred_at, &timezone, &day_start_time);

    let entry = NewStockLedgerEntry {
        outlet_id: req.outlet_id.clone(),
        inventory_item_id: req.inventory_item_id.clone(),
        inventory_item_name: name,
        dimension,
        entry_type: "WASTAGE".to_string(),
        origin: "WASTAGE".to_string(),
        // A wastage event always REDUCES stock; the caller supplies the
        // magnitude lost, this function applies the sign — the same
        // convention `deduction::ledger` uses for recipe consumption.
        quantity_applied_micro: -req.quantity_micro,
        recipe_id: None,
        recipe_version: None,
        recipe_name: None,
        source_order_id: None,
        source_order_item_id: None,
        reason_code: Some(req.reason_code),
        note: req.note,
        occurred_at: req.occurred_at.clone(),
        business_date,
        created_by_user_id: req.created_by_user_id,
        modifier_delta_id: None,
        modifier_name: None,
        modifier_delta_version: None,
        // Wastage is not count-driven — contracts 0.5.5's new column is
        // for COUNT_ADJUSTMENT provenance only (see
        // `crate::stock::count::complete_stock_count`).
        unit_cost_paise: None,
        // No invoiced total: this origin is valued AT the average, not by an
        // invoice, so writing a rounded quantity x rate product here would
        // fabricate precision and feed it back into the average (0.6.3).
        line_total_paise: None,
        source_stock_count_id: None,
        source_grn_id: None,
        source_purchase_return_id: None,
        source_stock_transfer_out_id: None,
    };

    let entry_seq = repo::next_stock_ledger_sequence_value(tx, &req.outlet_id, &req.occurred_at)?;
    let id = uuid::Uuid::now_v7().to_string();
    crate::deduction::ledger::insert_stock_ledger_entry(tx, &id, entry_seq, &entry)?;

    repo::get_stock_ledger_entry_by_seq(tx, &req.outlet_id, entry_seq)?
        .ok_or(DbError::NotFound("stock_ledger_entry"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{InventoryItem, Outlet};
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

    fn sample_request() -> NewWastageEntry {
        NewWastageEntry {
            outlet_id: "outlet-1".to_string(),
            inventory_item_id: "item-1".to_string(),
            quantity_micro: crate::inventory::grams(300),
            reason_code: "SPOILAGE".to_string(),
            note: Some("left out overnight".to_string()),
            occurred_at: "2026-08-20T13:45:12.000Z".to_string(),
            created_by_user_id: Some("user-1".to_string()),
        }
    }

    #[test]
    fn wastage_writes_a_negative_consumption_entry() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet_and_item(db.connection());

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        let stored = record_wastage(&tx, sample_request()).expect("record wastage");
        tx.commit().expect("commit");

        assert_eq!(stored.entry_type, "WASTAGE");
        assert_eq!(stored.origin, "WASTAGE");
        assert_eq!(stored.quantity_applied_micro, -crate::inventory::grams(300));
        assert_eq!(stored.reason_code.as_deref(), Some("SPOILAGE"));
        assert_eq!(stored.inventory_item_name, "Paneer");
        assert_eq!(stored.dimension, "MASS");

        // Assert against the store directly too, not just the returned
        // struct — a bug that mis-serialised the return value while
        // writing something else would otherwise pass silently.
        let current =
            repo::get_current_stock(db.connection(), "outlet-1", "item-1").expect("read stock");
        assert_eq!(current, -crate::inventory::grams(300));
    }

    #[test]
    fn a_non_positive_quantity_is_rejected_before_any_write() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet_and_item(db.connection());

        let mut req = sample_request();
        req.quantity_micro = 0;

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        let err = record_wastage(&tx, req).expect_err("zero quantity must be rejected");
        assert!(matches!(err, DbError::WastageQuantityNotPositive { .. }));
        tx.commit().expect("commit");

        let count: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM stock_ledger_entry WHERE outlet_id = 'outlet-1'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 0, "a rejected wastage request must write nothing");
    }

    #[test]
    fn a_blank_reason_code_is_rejected_before_any_write() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet_and_item(db.connection());

        let mut req = sample_request();
        req.reason_code = "   ".to_string();

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        let err = record_wastage(&tx, req).expect_err("blank reason must be rejected");
        assert!(matches!(err, DbError::WastageReasonRequired));
        tx.commit().expect("commit");

        let count: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM stock_ledger_entry WHERE outlet_id = 'outlet-1'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 0, "a rejected wastage request must write nothing");
    }

    #[test]
    fn a_dangling_inventory_item_is_rejected() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet_and_item(db.connection());

        let mut req = sample_request();
        req.inventory_item_id = "does-not-exist".to_string();

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        let err = record_wastage(&tx, req).expect_err("dangling item must be rejected");
        assert!(matches!(err, DbError::NotFound("inventory_item")));
        tx.commit().expect("commit");
    }
}
