//! Physical stock counts (Milestone 4, track T3, ADR-018). Open a count,
//! enter lines while it is OPEN, complete it — a completed count posts
//! `COUNT_ADJUSTMENT` ledger entries so the ledger stays the single source
//! of stock. Gated on `inventory.count` — enforced by the caller, see
//! `crate::stock`'s module doc comment.
//!
//! **`expected_quantity_micro` is snapshotted at the moment of counting,
//! never recomputed.** [`add_or_update_count_line`] reads
//! [`crate::stock::snapshot::get_current_stock_in_tx`] fresh on every call
//! and stores the result; nothing in this file, or anywhere else, ever
//! re-derives it from the stored count later. Recomputing it on read would
//! compare today's theory against yesterday's shelf.
//!
//! **A count is mutable while OPEN, immutable once COMPLETED** — enforced by
//! this module's own `status == 'OPEN'` check, backed by the 0016 triggers
//! on both `stock_count` and `stock_count_line` for the cases the triggers
//! actually reach. **Falsified, not assumed**: those two triggers are both
//! `BEFORE UPDATE`, so they catch a CORRECTION to an existing line on a
//! completed count, but NOT a brand-new item counted for the first time
//! after completion — that path is a plain `INSERT`, which no `BEFORE
//! UPDATE` trigger ever sees. Removing this module's own check and running
//! [`tests::a_completed_count_rejects_a_further_line_write`] confirmed a
//! fresh line insert sails through with no error at all when only the
//! trigger is relied on. This module's check is therefore not redundant
//! belt-and-braces — it is the ONLY guard for that case, and the trigger is
//! the backstop for corrections to a line already on the count.
//!
//! **The count's own `business_date` — not the completion instant's — is
//! what every `COUNT_ADJUSTMENT` entry it posts carries** (0016's own
//! migration header: "A count started 23:40 and completed 00:15 posts
//! COUNT_ADJUSTMENT entries dated to the earlier business day"). This is
//! deliberately the exact late-arrival shape `crate::stock::snapshot`'s
//! entry_seq-based read exists to survive.

use rusqlite::Transaction;

use crate::error::{DbError, DbResult};
use crate::model::{NewStockCount, NewStockCountLine, NewStockLedgerEntry, StockCount, StockCountLine};
use crate::repo;

use super::snapshot::get_current_stock_in_tx;

/// Opens a new count. `business_date` is computed once, here, from
/// `req.started_at` and the outlet's own `timezone`/`day_start_time`
/// (ADR-018 §9.2) — never accepted from a caller, and it is this exact
/// value every `COUNT_ADJUSTMENT` entry the count eventually posts will
/// carry, however late `complete_stock_count` is actually called.
pub(crate) fn open_stock_count(tx: &Transaction, req: NewStockCount) -> DbResult<StockCount> {
    let started_at = crate::tax::parse_utc(&req.started_at)?;
    let (timezone, day_start_time) =
        repo::get_outlet_business_date_config(tx, &req.outlet_id)?;
    let business_date =
        crate::deduction::business_date::compute_business_date(started_at, &timezone, &day_start_time);

    repo::insert_stock_count(
        tx,
        &req.id,
        &req.outlet_id,
        &business_date,
        &req.started_at,
        req.counted_by_user_id.as_deref(),
        req.note.as_deref(),
    )?;
    repo::get_stock_count(tx, &req.id)?.ok_or(DbError::NotFound("stock_count"))
}

/// Adds a new counted line, or corrects an existing one for the same item
/// (the table's own `UNIQUE(stock_count_id, inventory_item_id)`) — "mutable
/// while OPEN". Rejects with [`DbError::StockCountNotOpen`] if the count is
/// not currently `OPEN`, checked here first rather than left to the 0016
/// trigger. `expected_quantity_micro` is read fresh, right now, via the
/// bounded stock read — see the module doc comment.
pub(crate) fn add_or_update_count_line(
    tx: &Transaction,
    stock_count_id: &str,
    outlet_id: &str,
    req: NewStockCountLine,
) -> DbResult<StockCountLine> {
    let count = repo::get_stock_count(tx, stock_count_id)?
        .ok_or(DbError::NotFound("stock_count"))?;
    if count.status != "OPEN" {
        return Err(DbError::StockCountNotOpen {
            stock_count_id: stock_count_id.to_string(),
            status: count.status,
        });
    }

    let Some((name, dimension)) =
        repo::get_inventory_item_snapshot(tx, &req.inventory_item_id)?
    else {
        return Err(DbError::NotFound("inventory_item"));
    };

    let expected_quantity_micro =
        get_current_stock_in_tx(tx, outlet_id, &req.inventory_item_id)?;

    let new_id = uuid::Uuid::now_v7().to_string();
    repo::upsert_stock_count_line(
        tx,
        &new_id,
        stock_count_id,
        &req.inventory_item_id,
        &name,
        &dimension,
        req.counted_quantity_micro,
        expected_quantity_micro,
        req.note.as_deref(),
    )
}

/// Completes an OPEN count: marks it `COMPLETED`, then posts one
/// `COUNT_ADJUSTMENT` `stock_ledger_entry` per line whose variance
/// (`counted - expected`) is non-zero — a zero-variance line needs no
/// correction and writing one would only be noise in an append-only table.
/// Every posted entry carries the COUNT's own `business_date` (see the
/// module doc comment), and `completed_at` as its `occurred_at`.
///
/// **This never fails over a business reason once the OPEN check passes.**
/// There is no balance check anywhere here — a count that finds stock deep
/// in the negative posts the adjustment that makes the ledger agree with
/// reality; that negative balance is the variance signal, not a rejection
/// (ADR-018 Rule 1).
pub(crate) fn complete_stock_count(
    tx: &Transaction,
    stock_count_id: &str,
    outlet_id: &str,
    completed_at: &str,
) -> DbResult<StockCount> {
    let count = repo::get_stock_count(tx, stock_count_id)?
        .ok_or(DbError::NotFound("stock_count"))?;
    if count.status != "OPEN" {
        return Err(DbError::StockCountNotOpen {
            stock_count_id: stock_count_id.to_string(),
            status: count.status,
        });
    }

    let affected = repo::mark_stock_count_completed(tx, stock_count_id, completed_at)?;
    if affected != 1 {
        // The `status == OPEN` check above ran inside this same
        // transaction, so this can only be a logic error here, not a
        // legitimate race (the edge is a single SQLite writer, ADR-018
        // Rule 3) — the same posture `close_cash_shift` takes on its own
        // affected-row check.
        return Err(DbError::StockCountNotOpen {
            stock_count_id: stock_count_id.to_string(),
            status: "OPEN".to_string(),
        });
    }

    let lines = repo::list_stock_count_lines(tx, stock_count_id)?;
    for line in &lines {
        let variance = line.counted_quantity_micro - line.expected_quantity_micro;
        if variance == 0 {
            continue;
        }
        let entry = NewStockLedgerEntry {
            outlet_id: outlet_id.to_string(),
            inventory_item_id: line.inventory_item_id.clone(),
            inventory_item_name: line.inventory_item_name.clone(),
            dimension: line.dimension.clone(),
            entry_type: "ADJUSTMENT".to_string(),
            origin: "COUNT_ADJUSTMENT".to_string(),
            quantity_applied_micro: variance,
            recipe_id: None,
            recipe_version: None,
            recipe_name: None,
            source_order_id: None,
            source_order_item_id: None,
            reason_code: None,
            // 0016 carries no `source_stock_count_id` column (no FK, by the
            // table's own no-FK provenance design) — the count's id is
            // recorded here, in `note`, as the best available provenance
            // link. Flagged as an open item: a dedicated column would be a
            // cleaner audit trail, but adding one is a contract change this
            // task does not have the authority to make.
            note: Some(format!("stock_count:{stock_count_id}")),
            occurred_at: completed_at.to_string(),
            business_date: count.business_date.clone(),
            created_by_user_id: count.counted_by_user_id.clone(),
            modifier_delta_id: None,
            modifier_name: None,
            modifier_delta_version: None,
        };
        let entry_seq = repo::next_stock_ledger_sequence_value(tx, outlet_id, completed_at)?;
        let id = uuid::Uuid::now_v7().to_string();
        crate::deduction::ledger::insert_stock_ledger_entry(tx, &id, entry_seq, &entry)?;
    }

    repo::get_stock_count(tx, stock_count_id)?.ok_or(DbError::NotFound("stock_count"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::grams;
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

    fn new_count_req(id: &str, started_at: &str) -> NewStockCount {
        NewStockCount {
            id: id.to_string(),
            outlet_id: "outlet-1".to_string(),
            started_at: started_at.to_string(),
            counted_by_user_id: Some("user-1".to_string()),
            note: None,
        }
    }

    #[test]
    fn open_add_line_and_complete_posts_a_count_adjustment_entry() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet_and_item(db.connection());

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        let count =
            open_stock_count(&tx, new_count_req("count-1", "2026-08-20T22:10:00Z")).expect("open");
        assert_eq!(count.status, "OPEN");
        assert_eq!(count.business_date, "2026-08-21"); // IST local date

        let line = add_or_update_count_line(
            &tx,
            "count-1",
            "outlet-1",
            NewStockCountLine {
                inventory_item_id: "item-1".to_string(),
                counted_quantity_micro: grams(4_750),
                note: Some("370g short".to_string()),
            },
        )
        .expect("add line");
        assert_eq!(line.expected_quantity_micro, 0, "no ledger activity yet");
        assert_eq!(line.counted_quantity_micro, grams(4_750));

        let completed =
            complete_stock_count(&tx, "count-1", "outlet-1", "2026-08-20T22:41:00Z").expect("complete");
        assert_eq!(completed.status, "COMPLETED");
        tx.commit().expect("commit");

        // Assert the fixture rows landed before asserting anything about
        // them.
        let ledger_count: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM stock_ledger_entry WHERE outlet_id = 'outlet-1'",
                [],
                |row| row.get(0),
            )
            .expect("count ledger rows");
        assert_eq!(ledger_count, 1, "a non-zero variance must post exactly one COUNT_ADJUSTMENT entry");

        let stored: (String, String, i64, String) = db
            .connection()
            .query_row(
                "SELECT entry_type, origin, quantity_applied_micro, business_date \
                 FROM stock_ledger_entry WHERE outlet_id = 'outlet-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("read entry");
        assert_eq!(stored.0, "ADJUSTMENT");
        assert_eq!(stored.1, "COUNT_ADJUSTMENT");
        assert_eq!(stored.2, grams(4_750), "counted - expected(0) = counted");
        assert_eq!(
            stored.3, "2026-08-21",
            "the ADJUSTMENT entry must carry the COUNT's own business_date, not completion time's"
        );
    }

    #[test]
    fn a_zero_variance_line_posts_no_adjustment_entry() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet_and_item(db.connection());

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        open_stock_count(&tx, new_count_req("count-1", "2026-08-20T10:00:00Z")).expect("open");
        add_or_update_count_line(
            &tx,
            "count-1",
            "outlet-1",
            NewStockCountLine {
                inventory_item_id: "item-1".to_string(),
                counted_quantity_micro: 0,
                note: None,
            },
        )
        .expect("add line matching zero expected stock");
        complete_stock_count(&tx, "count-1", "outlet-1", "2026-08-20T10:05:00Z").expect("complete");
        tx.commit().expect("commit");

        let ledger_count: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM stock_ledger_entry WHERE outlet_id = 'outlet-1'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(ledger_count, 0, "a zero-variance line must post nothing");
    }

    #[test]
    fn a_completed_count_rejects_a_further_line_write() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet_and_item(db.connection());

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        open_stock_count(&tx, new_count_req("count-1", "2026-08-20T10:00:00Z")).expect("open");
        complete_stock_count(&tx, "count-1", "outlet-1", "2026-08-20T10:05:00Z").expect("complete");

        let err = add_or_update_count_line(
            &tx,
            "count-1",
            "outlet-1",
            NewStockCountLine {
                inventory_item_id: "item-1".to_string(),
                counted_quantity_micro: grams(1_000),
                note: None,
            },
        )
        .expect_err("a completed count must reject a further line write");
        assert!(matches!(err, DbError::StockCountNotOpen { .. }));
    }

    #[test]
    fn completing_an_already_completed_count_is_rejected_not_a_silent_no_op() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet_and_item(db.connection());

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        open_stock_count(&tx, new_count_req("count-1", "2026-08-20T10:00:00Z")).expect("open");
        complete_stock_count(&tx, "count-1", "outlet-1", "2026-08-20T10:05:00Z").expect("first complete");

        let err = complete_stock_count(&tx, "count-1", "outlet-1", "2026-08-20T10:06:00Z")
            .expect_err("completing twice must be rejected");
        assert!(matches!(err, DbError::StockCountNotOpen { .. }));
    }
}
