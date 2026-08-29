//! Edge-capable procurement (Milestone 5, track T2, ADR-019, contracts
//! 0.6.0): goods receipt, purchase return and the outbound half of
//! inter-outlet stock transfer, and the `stock_ledger_entry` rows each of
//! them posts.
//!
//! ============================================================================
//! THE RULE THIS WHOLE MODULE EXISTS TO ENFORCE
//! ============================================================================
//!
//! **A GRN NEVER BLOCKS ON A PO.** Goods arrive against an order that never
//! synced, an order amended after dispatch, and no order at all. Each records
//! a `grn_gap` and ACCEPTS the receipt. Refusing a delivery standing in the
//! kitchen doorway is the outage, not the protection — a refused receipt does
//! not keep the goods out of the walk-in, it only stops the system knowing
//! they went in. See `receipt`'s module doc comment for the full statement.
//!
//! Module layout:
//!   - `convert`   — purchase-unit conversion and `yield_factor_ppm`, the
//!     eight-value `GrnGapReason` closed set, and the `x == x` trap around
//!     `quantity_dimension`. Pure resolution: writes nothing.
//!   - `numbering` — the edge-local `grn_sequence` counter and the issued
//!     number's format. The counter never leaves the outlet.
//!   - `receipt`   — the GRN itself: header, lines, gaps and `PURCHASE`
//!     ledger entries, all in ONE transaction, plus the `entryIntentEcho`.
//!   - `cost`      — weighted average cost, derived from the ledger, never
//!     stored on `inventory_item`.
//!   - `movement`  — purchase returns (`RETURN_TO_VENDOR`) and outbound
//!     transfers (`TRANSFER_OUT`), posting the same way.
//!
//! **Permission gating is NOT enforced in this crate**, matching
//! `crate::stock`: `procurement.manage` is checked one layer up, in the Tauri
//! command handlers, and every entry point below names the permission its
//! caller must already hold.
//!
//! **Sync transport is NOT here either.** This module writes the rows and the
//! `local_outbox` events; cursors, retry budgets and HTTP push are `edge/sync`
//! (track T3). `grn_gap` rides the PLAIN outbox — no `entry_seq`, no counter,
//! no cursor, no contiguity check (ADR-019 §2).

pub(crate) mod convert;
pub(crate) mod cost;
pub(crate) mod movement;
pub(crate) mod numbering;
pub(crate) mod receipt;

pub use convert::GrnGapReason;

use rusqlite::{params, Connection};

use crate::error::DbResult;
use crate::model::{GrnGap, PurchaseOrderReceiptProgress};

/// The hard bound on the human-facing gap report, matching
/// `repo::STOCK_DEDUCTION_GAP_REPORT_LIMIT`'s posture: a fixed-cost read, not
/// a scan whose cost grows with an append-only signal table.
///
/// Lower than the deduction-gap bound on purpose. A `grn_gap` is a discrete
/// event a buyer acts on — a handful a week — so a screen showing 200 of them
/// is already showing a backlog nobody is working through.
pub(crate) const GRN_GAP_REPORT_LIMIT: i64 = 200;

/// The newest-first `grn_gap` read behind M5 acceptance criterion 3: **the
/// gap must be VISIBLE TO A HUMAN ON THE POS**, not merely present in a
/// table. `detail` is prose because a person reads it.
pub(crate) fn list_grn_gaps_for_outlet(
    conn: &Connection,
    outlet_id: &str,
) -> DbResult<Vec<GrnGap>> {
    let mut stmt = conn.prepare(
        "SELECT id, outlet_id, grn_id, grn_line_id, inventory_item_id, reason, detail, \
                occurred_at, business_date \
         FROM grn_gap WHERE outlet_id = ?1 \
         ORDER BY occurred_at DESC, id DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![outlet_id, GRN_GAP_REPORT_LIMIT], |row| {
            Ok(GrnGap {
                id: row.get(0)?,
                outlet_id: row.get(1)?,
                grn_id: row.get(2)?,
                grn_line_id: row.get(3)?,
                inventory_item_id: row.get(4)?,
                reason: row.get(5)?,
                detail: row.get(6)?,
                occurred_at: row.get(7)?,
                business_date: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// **THIS OUTLET's** view of how much of a purchase order has arrived,
/// derived on demand from local `grn_line` rows. Never stored, and never
/// written back to `purchase_order` — that would make the outlet a second
/// writer of a cloud-owned aggregate (ADR-019 §4, the §50.1 rule).
///
/// **The cloud's figure for the same PO will differ, and both are right.**
/// The cloud sums every outlet's receipts; this sums one outlet's. A shared
/// PO reads "40 of 100" here and "90 of 100" in the admin, simultaneously.
/// Show both, label which is which, NEVER reconcile them.
///
/// `ordered_base_quantity_micro` is reported in the PO line's own
/// `ordered_quantity_micro` units rather than converted, because a conversion
/// needs an item and a supplier context this read does not have; the caller
/// that wants a like-for-like comparison uses the receipt path's own
/// conversion. Stated rather than silently mixed.
pub(crate) fn purchase_order_receipt_progress(
    conn: &Connection,
    purchase_order_id: &str,
) -> DbResult<Vec<PurchaseOrderReceiptProgress>> {
    let mut stmt = conn.prepare(
        "SELECT pol.purchase_order_id, pol.id, pol.inventory_item_id, \
                pol.ordered_quantity_micro, \
                COALESCE((SELECT SUM(gl.base_quantity_micro) FROM grn_line gl \
                          WHERE gl.purchase_order_line_id = pol.id), 0) \
         FROM purchase_order_line pol \
         WHERE pol.purchase_order_id = ?1 \
         ORDER BY pol.line_number",
    )?;
    let rows = stmt
        .query_map(params![purchase_order_id], |row| {
            Ok(PurchaseOrderReceiptProgress {
                purchase_order_id: row.get(0)?,
                purchase_order_line_id: row.get(1)?,
                inventory_item_id: row.get(2)?,
                ordered_base_quantity_micro: row.get(3)?,
                received_base_quantity_micro_at_this_outlet: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Fixtures shared by this module's own tests. Config rows only — every
/// operational row a test asserts about must be written by the code under
/// test, never seeded, or the test proves nothing about the write path.
#[cfg(test)]
pub(crate) mod testsupport {
    use rusqlite::{params, Connection};

    use crate::model::Outlet;
    use crate::repo;

    pub(crate) fn seed_outlet(conn: &Connection, id: &str) {
        repo::upsert_outlet(
            conn,
            &Outlet {
                id: id.to_string(),
                brand_id: "brand-1".to_string(),
                name: format!("Outlet {id}"),
                timezone: "Asia/Kolkata".to_string(),
                config_version: 1,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )
        .expect("seed outlet");
    }

    pub(crate) fn seed_user(conn: &Connection, id: &str, outlet_id: &str) {
        conn.execute(
            "INSERT OR REPLACE INTO app_user
                (id, tenant_id, outlet_id, email, full_name, password_hash, pin_hash,
                 is_active, permissions_json, config_version, updated_at)
             VALUES (?1, 'tenant-1', ?2, ?3, 'Receiver', 'not-a-real-hash', NULL, 1,
                     '[]', 1, '2026-01-01T00:00:00Z')",
            params![id, outlet_id, format!("{id}@example.test")],
        )
        .expect("seed app_user");
    }

    pub(crate) fn seed_inventory_item(
        conn: &Connection,
        id: &str,
        outlet_id: &str,
        name: &str,
        dimension: &str,
        yield_factor_ppm: i64,
    ) {
        conn.execute(
            "INSERT OR REPLACE INTO inventory_item
                (id, outlet_id, sku, name, dimension, is_active, yield_factor_ppm,
                 config_version)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, 1)",
            params![
                id,
                outlet_id,
                format!("SKU-{id}"),
                name,
                dimension,
                yield_factor_ppm
            ],
        )
        .expect("seed inventory_item");
    }

    pub(crate) fn seed_supplier(conn: &Connection, id: &str, outlet_id: &str) {
        conn.execute(
            "INSERT OR REPLACE INTO supplier
                (id, outlet_id, code, name, payment_terms_days, is_active, config_version)
             VALUES (?1, ?2, ?3, 'Test Supplier', 0, 1, 1)",
            params![id, outlet_id, format!("SUP-{id}")],
        )
        .expect("seed supplier");
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn seed_supplier_item(
        conn: &Connection,
        outlet_id: &str,
        supplier_id: &str,
        inventory_item_id: &str,
        purchase_unit: &str,
        pack_size_micro: i64,
        quantity_dimension: &str,
    ) {
        seed_supplier(conn, supplier_id, outlet_id);
        conn.execute(
            "INSERT OR REPLACE INTO supplier_item
                (id, supplier_id, inventory_item_id, purchase_unit, pack_size_micro,
                 quantity_dimension, last_price_paise, is_preferred)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, 1)",
            params![
                format!("si-{supplier_id}-{inventory_item_id}-{purchase_unit}"),
                supplier_id,
                inventory_item_id,
                purchase_unit,
                pack_size_micro,
                quantity_dimension
            ],
        )
        .expect("seed supplier_item");
    }

    /// Seeds an APPROVED purchase order with one line. `approved_by_user_id`
    /// and `approved_at` are set together, because the table's own CHECK
    /// requires it and because a half-recorded approval is how "who
    /// authorised this spend" becomes unanswerable.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn seed_purchase_order_with_line(
        conn: &Connection,
        po_id: &str,
        po_line_id: &str,
        outlet_id: &str,
        supplier_id: &str,
        user_id: &str,
        inventory_item_id: &str,
        purchase_unit: &str,
        ordered_quantity_micro: i64,
        quantity_dimension: &str,
    ) {
        conn.execute(
            "INSERT OR REPLACE INTO purchase_order
                (id, outlet_id, supplier_id, po_number, status, total_paise,
                 approved_by_user_id, approved_at, created_at, config_version)
             VALUES (?1, ?2, ?3, ?4, 'SENT', 0, ?5, '2026-08-28T09:00:00Z',
                     '2026-08-28T09:00:00Z', 1)",
            params![
                po_id,
                outlet_id,
                supplier_id,
                format!("PO-{po_id}"),
                user_id
            ],
        )
        .expect("seed purchase_order");
        conn.execute(
            "INSERT OR REPLACE INTO purchase_order_line
                (id, purchase_order_id, inventory_item_id, line_number, purchase_unit,
                 ordered_quantity_micro, quantity_dimension, unit_price_paise,
                 line_total_paise)
             VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, 0, 0)",
            params![
                po_line_id,
                po_id,
                inventory_item_id,
                purchase_unit,
                ordered_quantity_micro,
                quantity_dimension
            ],
        )
        .expect("seed purchase_order_line");
    }
}
