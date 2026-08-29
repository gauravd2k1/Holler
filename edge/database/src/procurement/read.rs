//! The procurement READ surface (Milestone 5, track T3): suppliers, supplier
//! pricing and open purchase orders, for the receiving screen's pickers.
//!
//! ============================================================================
//! WHY THIS FILE EXISTS
//! ============================================================================
//!
//! `edge/database` exposed no read for `supplier`, `supplier_item` or
//! `purchase_order`, and the POS crate has no `rusqlite` dependency by design,
//! so a raw SELECT from the UI layer is structurally impossible. The receiving
//! screen therefore had typed UUID reference fields instead of pickers — a
//! screen that exists and nobody can use.
//!
//! That is the M4 missing-variant defect exactly: the till hardcoded
//! `variantId: null`, so no sale the POS ever took wrote a ledger row, while
//! the harness that "proved" the criterion selected a variant directly.
//! **A receiving clerk cannot type a UUID.**
//!
//! ============================================================================
//! READS ONLY
//! ============================================================================
//!
//! Every table here is CLOUD-OWNED CONFIG (ADR-019, §50.1). Nothing in this
//! file writes, and nothing in it decides anything: no conversion, no
//! defaulting, no dimension inference. In particular `quantity_dimension` is
//! returned exactly as stored — **the unit the author chose, never derived
//! from the referent** (contracts 0.5.2, ADR-019 §6). A read that "helpfully"
//! substituted `inventory_item.dimension` here would make the write path's
//! comparison `x == x` and silently disarm the `DIMENSION_MISMATCH` gap.

use rusqlite::{params, Connection};

use crate::error::DbResult;
use crate::model::{PurchaseOrderLineRow, PurchaseOrderSummary, Supplier, SupplierItem};

/// Hard bound on every list below, matching the posture of
/// [`super::GRN_GAP_REPORT_LIMIT`] and `repo::STOCK_DEDUCTION_GAP_REPORT_LIMIT`:
/// a picker is a fixed-cost read on a 4GB spinning disk, never a scan whose
/// cost grows with the config table behind it.
pub(crate) const PROCUREMENT_PICKER_LIMIT: i64 = 500;

/// The purchase-order statuses a delivery can arrive against.
///
/// `DRAFT` and `PENDING_APPROVAL` have not been placed with anyone, and
/// `CANCELLED`/`CLOSED` are finished — showing any of them in a receiving
/// picker invites a receipt against an order that was never sent. **A PO
/// missing from this list is not a refusal to receive**: the GRN path accepts
/// a receipt with no PO at all and records a `NO_PURCHASE_ORDER` gap
/// (ADR-019 §1).
const OPEN_PO_STATUSES: [&str; 2] = ["APPROVED", "SENT"];

/// Active suppliers for one outlet, name-ordered, for the picker.
///
/// Inactive suppliers are excluded from the PICKER, which is not the same as
/// being unknown: a receipt naming a supplier this edge has no row for is
/// still accepted, with a `SUPPLIER_NOT_FOUND` gap.
pub(crate) fn list_suppliers(conn: &Connection, outlet_id: &str) -> DbResult<Vec<Supplier>> {
    let mut stmt = conn.prepare(
        "SELECT id, outlet_id, code, name, gstin, phone, email, address, \
                payment_terms_days, is_active \
         FROM supplier WHERE outlet_id = ?1 AND is_active = 1 \
         ORDER BY name ASC, code ASC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![outlet_id, PROCUREMENT_PICKER_LIMIT], |row| {
            Ok(Supplier {
                id: row.get(0)?,
                outlet_id: row.get(1)?,
                code: row.get(2)?,
                name: row.get(3)?,
                gstin: row.get(4)?,
                phone: row.get(5)?,
                email: row.get(6)?,
                address: row.get(7)?,
                payment_terms_days: row.get(8)?,
                is_active: row.get::<_, i64>(9)? != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// What one supplier sells: the rows that resolve a purchase unit and pack
/// size for a receiving line.
///
/// `inventory_item_id` narrows to one item when the caller has already chosen
/// one. Several rows for the same item are normal and expected — `supplier_item`
/// is unique on `(supplier_id, inventory_item_id, purchase_unit)`, so a
/// supplier selling both SACK and KG is two rows, and the operator picks the
/// unit that is written on the delivery note. Preferred rows lead, then unit
/// name, so the order is stable rather than incidental.
pub(crate) fn list_supplier_items(
    conn: &Connection,
    supplier_id: &str,
    inventory_item_id: Option<&str>,
) -> DbResult<Vec<SupplierItem>> {
    let mut stmt = conn.prepare(
        "SELECT si.id, si.supplier_id, si.inventory_item_id, ii.name, \
                si.purchase_unit, si.pack_size_micro, si.quantity_dimension, \
                si.last_price_paise, si.is_preferred \
         FROM supplier_item si \
         JOIN inventory_item ii ON ii.id = si.inventory_item_id \
         WHERE si.supplier_id = ?1 \
           AND (?2 IS NULL OR si.inventory_item_id = ?2) \
         ORDER BY si.is_preferred DESC, ii.name ASC, si.purchase_unit ASC \
         LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(
            params![supplier_id, inventory_item_id, PROCUREMENT_PICKER_LIMIT],
            |row| {
                Ok(SupplierItem {
                    id: row.get(0)?,
                    supplier_id: row.get(1)?,
                    inventory_item_id: row.get(2)?,
                    inventory_item_name: row.get(3)?,
                    purchase_unit: row.get(4)?,
                    pack_size_micro: row.get(5)?,
                    // As stored. Never `inventory_item.dimension` — see the
                    // module doc comment.
                    quantity_dimension: row.get(6)?,
                    last_price_paise: row.get(7)?,
                    is_preferred: row.get::<_, i64>(8)? != 0,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Open purchase orders for one outlet, newest first, each with its lines, so
/// the receiving screen can pick one and prefill.
///
/// **No receipt state is returned or implied.** How much has already arrived
/// is a separate, derived read
/// ([`crate::Db::purchase_order_receipt_progress`]) whose figure legitimately
/// differs from the cloud's for the same PO (ADR-019 §4).
pub(crate) fn list_open_purchase_orders(
    conn: &Connection,
    outlet_id: &str,
) -> DbResult<Vec<PurchaseOrderSummary>> {
    let mut stmt = conn.prepare(
        "SELECT po.id, po.outlet_id, po.supplier_id, s.name, po.po_number, po.status, \
                po.expected_date, po.notes, po.total_paise, po.created_at \
         FROM purchase_order po \
         JOIN supplier s ON s.id = po.supplier_id \
         WHERE po.outlet_id = ?1 AND po.status IN (?2, ?3) \
         ORDER BY po.created_at DESC, po.po_number DESC LIMIT ?4",
    )?;
    let mut orders = stmt
        .query_map(
            params![
                outlet_id,
                OPEN_PO_STATUSES[0],
                OPEN_PO_STATUSES[1],
                PROCUREMENT_PICKER_LIMIT
            ],
            |row| {
                Ok(PurchaseOrderSummary {
                    id: row.get(0)?,
                    outlet_id: row.get(1)?,
                    supplier_id: row.get(2)?,
                    supplier_name: row.get(3)?,
                    po_number: row.get(4)?,
                    status: row.get(5)?,
                    expected_date: row.get(6)?,
                    notes: row.get(7)?,
                    total_paise: row.get(8)?,
                    created_at: row.get(9)?,
                    lines: Vec::new(),
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;

    for order in orders.iter_mut() {
        order.lines = list_purchase_order_lines(conn, &order.id)?;
    }
    Ok(orders)
}

/// One purchase order's lines, in line order.
pub(crate) fn list_purchase_order_lines(
    conn: &Connection,
    purchase_order_id: &str,
) -> DbResult<Vec<PurchaseOrderLineRow>> {
    let mut stmt = conn.prepare(
        "SELECT pol.id, pol.purchase_order_id, pol.inventory_item_id, ii.name, \
                pol.line_number, pol.purchase_unit, pol.ordered_quantity_micro, \
                pol.quantity_dimension, pol.unit_price_paise, pol.line_total_paise \
         FROM purchase_order_line pol \
         JOIN inventory_item ii ON ii.id = pol.inventory_item_id \
         WHERE pol.purchase_order_id = ?1 \
         ORDER BY pol.line_number ASC",
    )?;
    let rows = stmt
        .query_map(params![purchase_order_id], |row| {
            Ok(PurchaseOrderLineRow {
                id: row.get(0)?,
                purchase_order_id: row.get(1)?,
                inventory_item_id: row.get(2)?,
                inventory_item_name: row.get(3)?,
                line_number: row.get(4)?,
                purchase_unit: row.get(5)?,
                ordered_quantity_micro: row.get(6)?,
                quantity_dimension: row.get(7)?,
                unit_price_paise: row.get(8)?,
                line_total_paise: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::super::testsupport;
    use crate::Db;

    fn db() -> Db {
        Db::open_in_memory_for_tests().expect("open test db")
    }

    #[test]
    fn supplier_picker_lists_active_suppliers_and_hides_inactive_ones() {
        let db = db();
        testsupport::seed_outlet(db.connection(), "outlet-1");
        testsupport::seed_supplier(db.connection(), "sup-a", "outlet-1");
        testsupport::seed_supplier(db.connection(), "sup-b", "outlet-1");
        db.connection()
            .execute(
                "UPDATE supplier SET is_active = 0, name = 'Zed' WHERE id = 'sup-b'",
                [],
            )
            .expect("deactivate");

        // FIXTURES INSERTED? Assert the rows exist before asserting anything
        // about them: a rejected INSERT leaves zero rows and every later
        // assertion trivially "passes" on absent data.
        let seeded: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM supplier", [], |r| r.get(0))
            .expect("count suppliers");
        assert_eq!(seeded, 2, "both supplier fixtures must have inserted");

        let listed = db.list_suppliers("outlet-1").expect("list suppliers");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "sup-a");
        assert!(listed[0].is_active);
    }

    #[test]
    fn supplier_items_return_the_stored_dimension_not_the_items_own() {
        let db = db();
        testsupport::seed_outlet(db.connection(), "outlet-1");
        // The item is MASS; the supplier_item row deliberately says COUNT.
        // A read that "corrected" this to the item's dimension would make the
        // write path's comparison x == x and disarm DIMENSION_MISMATCH.
        testsupport::seed_inventory_item(
            db.connection(),
            "item-1",
            "outlet-1",
            "Onion",
            "MASS",
            1_000_000,
        );
        testsupport::seed_supplier_item(
            db.connection(),
            "outlet-1",
            "sup-a",
            "item-1",
            "SACK",
            50_000_000_000,
            "COUNT",
        );

        let rows = db
            .list_supplier_items("sup-a", Some("item-1"))
            .expect("list supplier items");
        assert_eq!(rows.len(), 1, "the supplier_item fixture must have inserted");
        assert_eq!(rows[0].quantity_dimension, "COUNT");
        assert_eq!(rows[0].pack_size_micro, 50_000_000_000);
        assert_eq!(rows[0].inventory_item_name, "Onion");
    }

    #[test]
    fn open_purchase_orders_carry_their_lines_and_exclude_finished_orders() {
        let db = db();
        testsupport::seed_outlet(db.connection(), "outlet-1");
        testsupport::seed_user(db.connection(), "user-1", "outlet-1");
        testsupport::seed_inventory_item(
            db.connection(),
            "item-1",
            "outlet-1",
            "Onion",
            "MASS",
            1_000_000,
        );
        testsupport::seed_supplier(db.connection(), "sup-a", "outlet-1");
        testsupport::seed_purchase_order_with_line(
            db.connection(),
            "po-open",
            "pol-1",
            "outlet-1",
            "sup-a",
            "user-1",
            "item-1",
            "SACK",
            2_000_000,
            "MASS",
        );
        testsupport::seed_purchase_order_with_line(
            db.connection(),
            "po-closed",
            "pol-2",
            "outlet-1",
            "sup-a",
            "user-1",
            "item-1",
            "SACK",
            2_000_000,
            "MASS",
        );
        db.connection()
            .execute(
                "UPDATE purchase_order SET status = 'CLOSED' WHERE id = 'po-closed'",
                [],
            )
            .expect("close po");

        let seeded: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM purchase_order_line", [], |r| r.get(0))
            .expect("count po lines");
        assert_eq!(seeded, 2, "both purchase-order fixtures must have inserted");

        let listed = db
            .list_open_purchase_orders("outlet-1")
            .expect("list purchase orders");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "po-open");
        assert_eq!(listed[0].supplier_name, "Test Supplier");
        assert_eq!(listed[0].lines.len(), 1);
        assert_eq!(listed[0].lines[0].id, "pol-1");
        assert_eq!(listed[0].lines[0].inventory_item_name, "Onion");
        assert_eq!(listed[0].lines[0].quantity_dimension, "MASS");
    }

    #[test]
    fn suppliers_from_another_outlet_never_appear() {
        let db = db();
        testsupport::seed_outlet(db.connection(), "outlet-1");
        testsupport::seed_outlet(db.connection(), "outlet-2");
        testsupport::seed_supplier(db.connection(), "sup-other", "outlet-2");

        let listed = db.list_suppliers("outlet-1").expect("list suppliers");
        assert!(listed.is_empty());
        let other = db.list_suppliers("outlet-2").expect("list suppliers");
        assert_eq!(other.len(), 1, "the fixture must have inserted somewhere");
    }
}
