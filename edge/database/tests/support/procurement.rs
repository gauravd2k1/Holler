//! Procurement fixtures for `tests/goods_receipt.rs`.
//!
//! `#![allow(dead_code)]` because cargo compiles a `tests/support/` module
//! into EVERY test binary in this directory, and each binary uses only the
//! helpers it needs. Every other binary therefore sees the rest as dead --
//! 14 warnings, and CI's clippy runs with `-D warnings`, so `edge-style`
//! failed to compile `invoice_numbering_stress` over helpers that are used,
//! just not by that target. The allow is scoped to this shared module; it
//! silences nothing in the code under test.
//!
//! **CONFIG ROWS ONLY.** Everything operational a test asserts about — the
//! receipt, its lines, its gaps, its ledger entries — must be written by the
//! code under test. A helper that seeded a `goods_receipt_note` would let a
//! test pass against a write path that never ran.

#![allow(dead_code)]

use rusqlite::{params, Connection};

use holler_edge_database::model::Outlet;
use holler_edge_database::repo;

pub const OUTLET: &str = "outlet-1";
pub const USER: &str = "user-1";
pub const SUPPLIER: &str = "supplier-1";
pub const RICE: &str = "item-rice";
pub const PO_ID: &str = "po-1";
pub const PO_LINE_ID: &str = "po-line-1";

/// The identity yield, 1_000_000 ppm = 100%.
pub const IDENTITY_PPM: i64 = 1_000_000;

/// `n` whole purchase units as a `*_micro` count. Written as a helper so no
/// test reproduces the x10^6 multiplier by hand — the same reasoning behind
/// `crate::inventory`'s typed constructors.
pub fn micro(n: i64) -> i64 {
    n * 1_000_000
}

pub fn seed_outlet(conn: &Connection, id: &str) {
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

pub fn seed_user(conn: &Connection, id: &str, outlet_id: &str) {
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

pub fn seed_inventory_item(
    conn: &Connection,
    id: &str,
    outlet_id: &str,
    name: &str,
    dimension: &str,
    yield_factor_ppm: i64,
) {
    conn.execute(
        "INSERT OR REPLACE INTO inventory_item
            (id, outlet_id, sku, name, dimension, is_active, yield_factor_ppm, config_version)
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

pub fn seed_supplier(conn: &Connection, id: &str, outlet_id: &str) {
    conn.execute(
        "INSERT OR REPLACE INTO supplier
            (id, outlet_id, code, name, payment_terms_days, is_active, config_version)
         VALUES (?1, ?2, ?3, 'Test Supplier', 0, 1, 1)",
        params![id, outlet_id, format!("SUP-{id}")],
    )
    .expect("seed supplier");
}

pub fn seed_supplier_item(
    conn: &Connection,
    supplier_id: &str,
    inventory_item_id: &str,
    purchase_unit: &str,
    pack_size_micro: i64,
    quantity_dimension: &str,
) {
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

/// An APPROVED, SENT purchase order with one line.
///
/// `approved_by_user_id` and `approved_at` are set TOGETHER, because the
/// table's own CHECK requires it and because a half-recorded approval is how
/// "who authorised this spend" becomes unanswerable a year later.
#[allow(clippy::too_many_arguments)]
pub fn seed_purchase_order_with_line(
    conn: &Connection,
    outlet_id: &str,
    supplier_id: &str,
    user_id: &str,
    inventory_item_id: &str,
    purchase_unit: &str,
    ordered_quantity_micro: i64,
) {
    conn.execute(
        "INSERT OR REPLACE INTO purchase_order
            (id, outlet_id, supplier_id, po_number, status, total_paise,
             approved_by_user_id, approved_at, created_at, config_version)
         VALUES (?1, ?2, ?3, 'PO-0001', 'SENT', 0, ?4, '2026-08-28T09:00:00Z',
                 '2026-08-28T09:00:00Z', 1)",
        params![PO_ID, outlet_id, supplier_id, user_id],
    )
    .expect("seed purchase_order");
    conn.execute(
        "INSERT OR REPLACE INTO purchase_order_line
            (id, purchase_order_id, inventory_item_id, line_number, purchase_unit,
             ordered_quantity_micro, quantity_dimension, unit_price_paise, line_total_paise)
         VALUES (?1, ?2, ?3, 1, ?4, ?5, 'MASS', 0, 0)",
        params![
            PO_LINE_ID,
            PO_ID,
            inventory_item_id,
            purchase_unit,
            ordered_quantity_micro
        ],
    )
    .expect("seed purchase_order_line");
}
