//! Typed repositories over the frozen contract tables. Config aggregates
//! (outlet, device, app_user, restaurant_table, menu_*) expose plain
//! upsert/read operations — the sync worker (not this crate) is responsible
//! for calling them with a cloud-authorized `config_version` (sync.md
//! §50.1). Operational aggregates (order, order_item, table_session) never
//! expose a bare insert: every write that creates or mutates one goes
//! through [`crate::Db::write_order`] / [`crate::Db::write_table_session`],
//! which write the `local_outbox` row in the same transaction (ADR-007) so
//! it is not possible to call this module and produce an order without an
//! outbox entry.

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::error::DbResult;
use crate::model::*;

fn bool_to_i64(b: bool) -> i64 {
    if b {
        1
    } else {
        0
    }
}

fn i64_to_bool(v: i64) -> bool {
    v != 0
}

// ---------------------------------------------------------------- outlet --

pub fn upsert_outlet(conn: &Connection, o: &Outlet) -> DbResult<()> {
    conn.execute(
        "INSERT INTO outlet (id, brand_id, name, timezone, config_version, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
            brand_id = excluded.brand_id,
            name = excluded.name,
            timezone = excluded.timezone,
            config_version = excluded.config_version,
            updated_at = excluded.updated_at
         WHERE excluded.config_version >= outlet.config_version",
        params![o.id, o.brand_id, o.name, o.timezone, o.config_version, o.created_at, o.updated_at],
    )?;
    Ok(())
}

pub fn get_outlet(conn: &Connection, id: &str) -> DbResult<Option<Outlet>> {
    conn.query_row(
        "SELECT id, brand_id, name, timezone, config_version, created_at, updated_at
         FROM outlet WHERE id = ?1",
        params![id],
        |row| {
            Ok(Outlet {
                id: row.get(0)?,
                brand_id: row.get(1)?,
                name: row.get(2)?,
                timezone: row.get(3)?,
                config_version: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

// ----------------------------------------------------------------- device --

pub fn upsert_device(conn: &Connection, d: &Device) -> DbResult<()> {
    conn.execute(
        "INSERT INTO device (id, outlet_id, kind, name, last_seen_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
            outlet_id = excluded.outlet_id,
            kind = excluded.kind,
            name = excluded.name,
            last_seen_at = excluded.last_seen_at",
        params![d.id, d.outlet_id, d.kind, d.name, d.last_seen_at, d.created_at],
    )?;
    Ok(())
}

pub fn get_device(conn: &Connection, id: &str) -> DbResult<Option<Device>> {
    conn.query_row(
        "SELECT id, outlet_id, kind, name, last_seen_at, created_at FROM device WHERE id = ?1",
        params![id],
        |row| {
            Ok(Device {
                id: row.get(0)?,
                outlet_id: row.get(1)?,
                kind: row.get(2)?,
                name: row.get(3)?,
                last_seen_at: row.get(4)?,
                created_at: row.get(5)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

// --------------------------------------------------------------- app_user --
// Config aggregate: cloud owns it, replaced wholesale per config_version
// (ADR-011 §1). This is the only table allowed to carry credential
// material, and it is never returned over any wire API by this crate.

pub fn replace_app_user(conn: &Connection, u: &AppUser) -> DbResult<()> {
    conn.execute(
        "INSERT INTO app_user
            (id, tenant_id, outlet_id, email, full_name, password_hash, pin_hash,
             is_active, permissions_json, config_version, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(id) DO UPDATE SET
            tenant_id = excluded.tenant_id,
            outlet_id = excluded.outlet_id,
            email = excluded.email,
            full_name = excluded.full_name,
            password_hash = excluded.password_hash,
            pin_hash = excluded.pin_hash,
            is_active = excluded.is_active,
            permissions_json = excluded.permissions_json,
            config_version = excluded.config_version,
            updated_at = excluded.updated_at
         WHERE excluded.config_version >= app_user.config_version",
        params![
            u.id,
            u.tenant_id,
            u.outlet_id,
            u.email,
            u.full_name,
            u.password_hash,
            u.pin_hash,
            bool_to_i64(u.is_active),
            u.permissions_json,
            u.config_version,
            u.updated_at,
        ],
    )?;
    Ok(())
}

fn row_to_app_user(row: &rusqlite::Row) -> rusqlite::Result<AppUser> {
    Ok(AppUser {
        id: row.get(0)?,
        tenant_id: row.get(1)?,
        outlet_id: row.get(2)?,
        email: row.get(3)?,
        full_name: row.get(4)?,
        password_hash: row.get(5)?,
        pin_hash: row.get(6)?,
        is_active: i64_to_bool(row.get(7)?),
        permissions_json: row.get(8)?,
        config_version: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

const APP_USER_COLUMNS: &str = "id, tenant_id, outlet_id, email, full_name, password_hash, pin_hash, \
     is_active, permissions_json, config_version, updated_at";

pub fn get_app_user_by_id(conn: &Connection, id: &str) -> DbResult<Option<AppUser>> {
    conn.query_row(
        &format!("SELECT {APP_USER_COLUMNS} FROM app_user WHERE id = ?1"),
        params![id],
        row_to_app_user,
    )
    .optional()
    .map_err(Into::into)
}

/// Looks up an active user by outlet + email for offline login. Inactive
/// users never verify, matching cloud behaviour.
pub fn get_active_app_user_by_email(
    conn: &Connection,
    outlet_id: &str,
    email: &str,
) -> DbResult<Option<AppUser>> {
    conn.query_row(
        &format!(
            "SELECT {APP_USER_COLUMNS} FROM app_user \
             WHERE outlet_id = ?1 AND email = ?2 AND is_active = 1"
        ),
        params![outlet_id, email],
        row_to_app_user,
    )
    .optional()
    .map_err(Into::into)
}

/// Verifies `plaintext` against the cached hash for `outlet_id`/`email`
/// without ever handling a raw SQL row outside this function — this is the
/// only offline-login entry point this crate exposes.
pub fn verify_offline_login(
    conn: &Connection,
    outlet_id: &str,
    email: &str,
    plaintext_password: &str,
) -> DbResult<AppUser> {
    let user = get_active_app_user_by_email(conn, outlet_id, email)?
        .ok_or(crate::error::DbError::CredentialMismatch)?;
    crate::auth::verify_password(plaintext_password, &user.password_hash)?;
    Ok(user)
}

// --------------------------------------------------------- restaurant_table --

pub fn upsert_restaurant_table(conn: &Connection, t: &RestaurantTable) -> DbResult<()> {
    conn.execute(
        "INSERT INTO restaurant_table (id, outlet_id, section, label, seat_count, is_active, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
            outlet_id = excluded.outlet_id,
            section = excluded.section,
            label = excluded.label,
            seat_count = excluded.seat_count,
            is_active = excluded.is_active,
            config_version = excluded.config_version
         WHERE excluded.config_version >= restaurant_table.config_version",
        params![
            t.id,
            t.outlet_id,
            t.section,
            t.label,
            t.seat_count,
            bool_to_i64(t.is_active),
            t.config_version
        ],
    )?;
    Ok(())
}

pub fn list_restaurant_tables(conn: &Connection, outlet_id: &str) -> DbResult<Vec<RestaurantTable>> {
    let mut stmt = conn.prepare(
        "SELECT id, outlet_id, section, label, seat_count, is_active, config_version
         FROM restaurant_table WHERE outlet_id = ?1 ORDER BY section, label",
    )?;
    let rows = stmt
        .query_map(params![outlet_id], |row| {
            Ok(RestaurantTable {
                id: row.get(0)?,
                outlet_id: row.get(1)?,
                section: row.get(2)?,
                label: row.get(3)?,
                seat_count: row.get(4)?,
                is_active: i64_to_bool(row.get(5)?),
                config_version: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ----------------------------------------------------------------- menu ----

pub fn upsert_menu_category(conn: &Connection, c: &MenuCategory) -> DbResult<()> {
    conn.execute(
        "INSERT INTO menu_category (id, outlet_id, name, sort_order, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
            outlet_id = excluded.outlet_id, name = excluded.name,
            sort_order = excluded.sort_order, config_version = excluded.config_version
         WHERE excluded.config_version >= menu_category.config_version",
        params![c.id, c.outlet_id, c.name, c.sort_order, c.config_version],
    )?;
    Ok(())
}

pub fn upsert_menu_item(conn: &Connection, m: &MenuItem) -> DbResult<()> {
    conn.execute(
        "INSERT INTO menu_item (id, outlet_id, category_id, name, base_price_paise, is_available, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
            outlet_id = excluded.outlet_id, category_id = excluded.category_id, name = excluded.name,
            base_price_paise = excluded.base_price_paise, is_available = excluded.is_available,
            config_version = excluded.config_version
         WHERE excluded.config_version >= menu_item.config_version",
        params![
            m.id,
            m.outlet_id,
            m.category_id,
            m.name,
            m.base_price_paise,
            bool_to_i64(m.is_available),
            m.config_version
        ],
    )?;
    Ok(())
}

pub fn upsert_menu_item_variant(conn: &Connection, v: &MenuItemVariant) -> DbResult<()> {
    conn.execute(
        "INSERT INTO menu_item_variant (id, menu_item_id, name, price_delta_paise, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
            menu_item_id = excluded.menu_item_id, name = excluded.name,
            price_delta_paise = excluded.price_delta_paise, config_version = excluded.config_version
         WHERE excluded.config_version >= menu_item_variant.config_version",
        params![v.id, v.menu_item_id, v.name, v.price_delta_paise, v.config_version],
    )?;
    Ok(())
}

pub fn upsert_menu_item_modifier(conn: &Connection, m: &MenuItemModifier) -> DbResult<()> {
    conn.execute(
        "INSERT INTO menu_item_modifier
            (id, menu_item_id, group_name, option_name, price_delta_paise, min_selection, max_selection, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
            menu_item_id = excluded.menu_item_id, group_name = excluded.group_name,
            option_name = excluded.option_name, price_delta_paise = excluded.price_delta_paise,
            min_selection = excluded.min_selection, max_selection = excluded.max_selection,
            config_version = excluded.config_version
         WHERE excluded.config_version >= menu_item_modifier.config_version",
        params![
            m.id,
            m.menu_item_id,
            m.group_name,
            m.option_name,
            m.price_delta_paise,
            m.min_selection,
            m.max_selection,
            m.config_version
        ],
    )?;
    Ok(())
}

/// Config aggregate, read-only from the edge (§50.1): the sync worker calls
/// `upsert_menu_category` with a cloud-authorized `config_version`; the POS
/// only ever reads it back.
pub fn list_menu_categories_for_outlet(
    conn: &Connection,
    outlet_id: &str,
) -> DbResult<Vec<MenuCategory>> {
    let mut stmt = conn.prepare(
        "SELECT id, outlet_id, name, sort_order, config_version
         FROM menu_category WHERE outlet_id = ?1 ORDER BY sort_order, name",
    )?;
    let rows = stmt
        .query_map(params![outlet_id], |row| {
            Ok(MenuCategory {
                id: row.get(0)?,
                outlet_id: row.get(1)?,
                name: row.get(2)?,
                sort_order: row.get(3)?,
                config_version: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn list_menu_items_for_outlet(conn: &Connection, outlet_id: &str) -> DbResult<Vec<MenuItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, outlet_id, category_id, name, base_price_paise, is_available, config_version
         FROM menu_item WHERE outlet_id = ?1 ORDER BY name",
    )?;
    let rows = stmt
        .query_map(params![outlet_id], |row| {
            Ok(MenuItem {
                id: row.get(0)?,
                outlet_id: row.get(1)?,
                category_id: row.get(2)?,
                name: row.get(3)?,
                base_price_paise: row.get(4)?,
                is_available: i64_to_bool(row.get(5)?),
                config_version: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// `menu_item_variant` has no `outlet_id` column of its own (it hangs off
/// `menu_item_id`), so "outlet-scoped" means joining through `menu_item` —
/// this is the read the POS needs to price "Large / Cheese Burst" without
/// a second round trip per item.
pub fn list_menu_item_variants_for_outlet(
    conn: &Connection,
    outlet_id: &str,
) -> DbResult<Vec<MenuItemVariant>> {
    let mut stmt = conn.prepare(
        "SELECT v.id, v.menu_item_id, v.name, v.price_delta_paise, v.config_version
         FROM menu_item_variant v
         JOIN menu_item m ON m.id = v.menu_item_id
         WHERE m.outlet_id = ?1
         ORDER BY v.menu_item_id, v.name",
    )?;
    let rows = stmt
        .query_map(params![outlet_id], |row| {
            Ok(MenuItemVariant {
                id: row.get(0)?,
                menu_item_id: row.get(1)?,
                name: row.get(2)?,
                price_delta_paise: row.get(3)?,
                config_version: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Same outlet-scoping-via-join rationale as
/// [`list_menu_item_variants_for_outlet`], for modifier groups/options.
pub fn list_menu_item_modifiers_for_outlet(
    conn: &Connection,
    outlet_id: &str,
) -> DbResult<Vec<MenuItemModifier>> {
    let mut stmt = conn.prepare(
        "SELECT mm.id, mm.menu_item_id, mm.group_name, mm.option_name, mm.price_delta_paise,
                mm.min_selection, mm.max_selection, mm.config_version
         FROM menu_item_modifier mm
         JOIN menu_item m ON m.id = mm.menu_item_id
         WHERE m.outlet_id = ?1
         ORDER BY mm.menu_item_id, mm.group_name, mm.option_name",
    )?;
    let rows = stmt
        .query_map(params![outlet_id], |row| {
            Ok(MenuItemModifier {
                id: row.get(0)?,
                menu_item_id: row.get(1)?,
                group_name: row.get(2)?,
                option_name: row.get(3)?,
                price_delta_paise: row.get(4)?,
                min_selection: row.get(5)?,
                max_selection: row.get(6)?,
                config_version: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ------------------------------------------------------------- "order" -----
// Reserved word: quoted in every statement, including inside format!
// strings, per the schema comment in 0001_init.sql.

pub(crate) fn insert_order(tx: &Transaction, o: &NewOrder) -> DbResult<()> {
    tx.execute(
        "INSERT INTO \"order\"
            (id, outlet_id, device_id, order_type, status, table_id,
             subtotal_paise, discount_paise, tax_paise, total_paise, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            o.id,
            o.outlet_id,
            o.device_id,
            o.order_type,
            o.status,
            o.table_id,
            o.subtotal_paise,
            o.discount_paise,
            o.tax_paise,
            o.total_paise,
            o.created_at,
            o.updated_at,
        ],
    )?;
    Ok(())
}

pub(crate) fn insert_order_item(tx: &Transaction, i: &NewOrderItem) -> DbResult<()> {
    tx.execute(
        "INSERT INTO order_item
            (id, order_id, menu_item_id, variant_id, quantity, unit_price_paise, line_total_paise, notes, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            i.id,
            i.order_id,
            i.menu_item_id,
            i.variant_id,
            i.quantity,
            i.unit_price_paise,
            i.line_total_paise,
            i.notes,
            i.created_at,
        ],
    )?;
    Ok(())
}

/// Enforces "amendment is only legal while the order is DRAFT" inside the
/// same transaction as the write it is guarding, so the check and the
/// mutation it protects can never race. Returns the order's `outlet_id` on
/// success (callers that need it, e.g. to build an outbox event envelope,
/// then avoid a second round trip). Returns `DbError::NotFound` if the
/// order does not exist, `DbError::OrderNotAmendable` if it exists but is
/// not DRAFT.
pub(crate) fn require_draft_order(tx: &Transaction, order_id: &str) -> DbResult<String> {
    let row: Option<(String, String)> = tx
        .query_row(
            "SELECT outlet_id, status FROM \"order\" WHERE id = ?1",
            params![order_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match row {
        None => Err(crate::error::DbError::NotFound("order")),
        Some((outlet_id, status)) if status == "DRAFT" => Ok(outlet_id),
        Some((_, status)) => Err(crate::error::DbError::OrderNotAmendable {
            order_id: order_id.to_string(),
            status,
        }),
    }
}

pub(crate) fn get_order_item_in_tx(tx: &Transaction, id: &str) -> DbResult<Option<OrderItem>> {
    tx.query_row(
        "SELECT id, order_id, menu_item_id, variant_id, quantity, unit_price_paise, line_total_paise, notes, created_at
         FROM order_item WHERE id = ?1",
        params![id],
        |row| {
            Ok(OrderItem {
                id: row.get(0)?,
                order_id: row.get(1)?,
                menu_item_id: row.get(2)?,
                variant_id: row.get(3)?,
                quantity: row.get(4)?,
                unit_price_paise: row.get(5)?,
                line_total_paise: row.get(6)?,
                notes: row.get(7)?,
                created_at: row.get(8)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn delete_order_item(tx: &Transaction, id: &str) -> DbResult<()> {
    let changed = tx.execute("DELETE FROM order_item WHERE id = ?1", params![id])?;
    if changed == 0 {
        return Err(crate::error::DbError::NotFound("order_item"));
    }
    Ok(())
}

// -------------------------------------------------- order_item_modifier ---
// Snapshot rows (contracts 0.2.3, 0003_order_item_modifiers.sql).
// modifier_id/group_name/option_name/price_delta_paise are deliberately NOT
// joined from menu_item_modifier at read time — see the migration's own
// comment. This crate never "helpfully" fills them from the live catalog.

pub(crate) fn insert_order_item_modifier(tx: &Transaction, m: &OrderItemModifier) -> DbResult<()> {
    tx.execute(
        "INSERT INTO order_item_modifier
            (id, order_item_id, modifier_id, group_name, option_name, price_delta_paise, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            m.id,
            m.order_item_id,
            m.modifier_id,
            m.group_name,
            m.option_name,
            m.price_delta_paise,
            m.created_at,
        ],
    )?;
    Ok(())
}

fn row_to_order_item_modifier(row: &rusqlite::Row) -> rusqlite::Result<OrderItemModifier> {
    Ok(OrderItemModifier {
        id: row.get(0)?,
        order_item_id: row.get(1)?,
        modifier_id: row.get(2)?,
        group_name: row.get(3)?,
        option_name: row.get(4)?,
        price_delta_paise: row.get(5)?,
        created_at: row.get(6)?,
    })
}

const ORDER_ITEM_MODIFIER_COLUMNS: &str =
    "id, order_item_id, modifier_id, group_name, option_name, price_delta_paise, created_at";

pub(crate) fn list_order_item_modifiers_in_tx(
    tx: &Transaction,
    order_item_id: &str,
) -> DbResult<Vec<OrderItemModifier>> {
    let mut stmt = tx.prepare(&format!(
        "SELECT {ORDER_ITEM_MODIFIER_COLUMNS} FROM order_item_modifier \
         WHERE order_item_id = ?1 ORDER BY created_at, id"
    ))?;
    let rows = stmt
        .query_map(params![order_item_id], row_to_order_item_modifier)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Public read path for an already-committed line's modifier selections
/// (e.g. rendering a receipt or the cart). Config-analogue read-only
/// accessor; there is no public write path — modifiers are only ever
/// written inside [`crate::Db::add_order_item_with_outbox`].
pub fn list_order_item_modifiers(
    conn: &Connection,
    order_item_id: &str,
) -> DbResult<Vec<OrderItemModifier>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {ORDER_ITEM_MODIFIER_COLUMNS} FROM order_item_modifier \
         WHERE order_item_id = ?1 ORDER BY created_at, id"
    ))?;
    let rows = stmt
        .query_map(params![order_item_id], row_to_order_item_modifier)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The MONEY INVARIANT from `0003_order_item_modifiers.sql`, the single
/// definition shared by the edge recompute path and (by contract) the cloud
/// replay path:
///     unit_price_paise = snapshot of base + variant delta
///     line_total_paise = (unit_price_paise + SUM(price_delta_paise)) * quantity
/// All `i64`; never floating point.
pub(crate) fn compute_line_total_paise(
    unit_price_paise: i64,
    quantity: i64,
    modifiers: &[OrderItemModifier],
) -> i64 {
    let modifier_delta_sum: i64 = modifiers.iter().map(|m| m.price_delta_paise).sum();
    (unit_price_paise + modifier_delta_sum) * quantity
}

/// The frozen outbox event types for order-line amendment, from
/// `packages/contracts/src/types/events.ts` `OUTBOX_EVENT_TYPES` (checked
/// against drift by `scripts/check-event-type-drift.mjs`). Do not change
/// these strings without updating that list first.
const EVENT_TYPE_ITEM_ADDED: &str = "ItemAdded";
const EVENT_TYPE_ITEM_REMOVED: &str = "ItemRemoved";

/// Builds the contract's `OrderItem` shape (`packages/contracts/src/types/order.ts`
/// `OrderItemSchema`) as JSON: `{ id, menu_item_id, variant_id, quantity,
/// unit_price_paise, line_total_paise, modifiers, notes }`, where each
/// modifier is `{ modifier_id, group_name, option_name, price_delta_paise }`
/// — exactly the columns `order_item_modifier` snapshots, no more. Shared by
/// the `ItemAdded` and `ItemRemoved` payload builders so the two events
/// describe a line identically.
#[allow(clippy::too_many_arguments)]
pub(crate) fn item_json(
    id: &str,
    menu_item_id: &str,
    variant_id: Option<&str>,
    quantity: i64,
    unit_price_paise: i64,
    line_total_paise: i64,
    notes: Option<&str>,
    modifiers: &[OrderItemModifier],
) -> serde_json::Value {
    let modifiers_json: Vec<serde_json::Value> = modifiers
        .iter()
        .map(|m| {
            serde_json::json!({
                "modifier_id": m.modifier_id,
                "group_name": m.group_name,
                "option_name": m.option_name,
                "price_delta_paise": m.price_delta_paise,
            })
        })
        .collect();

    serde_json::json!({
        "id": id,
        "menu_item_id": menu_item_id,
        "variant_id": variant_id,
        "quantity": quantity,
        "unit_price_paise": unit_price_paise,
        "line_total_paise": line_total_paise,
        "modifiers": modifiers_json,
        "notes": notes,
    })
}

/// Builds the `ItemAdded` event envelope + `data` payload from the
/// `order_item` row this crate is about to write (with its real modifier
/// snapshots, not an empty placeholder), matching `ItemAddedEventSchema` in
/// `packages/contracts/src/types/events.ts` exactly (`{ event_id,
/// event_type, occurred_at, outlet_id, schema_version, data: { order_id,
/// item } }`). The caller never supplies `event_type` or the item
/// description — both come from the row itself, so a caller cannot commit
/// a mismatched event for a real write.
fn build_item_added_payload(
    outlet_id: &str,
    order_id: &str,
    item: &NewOrderItem,
    modifiers: &[OrderItemModifier],
    event_id: &str,
    occurred_at: &str,
) -> String {
    let item_value = item_json(
        &item.id,
        &item.menu_item_id,
        item.variant_id.as_deref(),
        item.quantity,
        item.unit_price_paise,
        item.line_total_paise,
        item.notes.as_deref(),
        modifiers,
    );
    serde_json::json!({
        "event_id": event_id,
        "event_type": EVENT_TYPE_ITEM_ADDED,
        "occurred_at": occurred_at,
        "outlet_id": outlet_id,
        "schema_version": 1,
        "data": {
            "order_id": order_id,
            "item": item_value,
        }
    })
    .to_string()
}

/// Writes the `local_outbox` row for an `add_order_item_with_outbox` call.
/// `event_type`/`payload_json` are derived here, not accepted from the
/// caller — see [`build_item_added_payload`].
pub(crate) fn insert_item_added_outbox(
    tx: &Transaction,
    outlet_id: &str,
    order_id: &str,
    item: &NewOrderItem,
    modifiers: &[OrderItemModifier],
    meta: &OrderItemAddedMeta,
) -> DbResult<()> {
    let payload = build_item_added_payload(
        outlet_id,
        order_id,
        item,
        modifiers,
        &meta.outbox_id,
        &meta.occurred_at,
    );
    insert_outbox_entry(
        tx,
        &NewOutboxEntry {
            id: meta.outbox_id.clone(),
            aggregate_type: "order".to_string(),
            aggregate_id: order_id.to_string(),
            event_type: EVENT_TYPE_ITEM_ADDED.to_string(),
            payload_json: payload,
            created_at: meta.occurred_at.clone(),
        },
    )
}

/// Builds the `ItemRemoved` event envelope + `data` payload from the
/// `order_item` row about to be deleted (with its modifier snapshots read
/// *before* the delete — `order_item_modifier` cascades on delete, so they
/// are unrecoverable afterward), matching `ItemRemovedEventSchema` exactly.
/// The full item travels in the payload deliberately: once the row is gone
/// the cloud has no way to look up what left the order.
fn build_item_removed_payload(
    outlet_id: &str,
    order_id: &str,
    item: &OrderItem,
    modifiers: &[OrderItemModifier],
    event_id: &str,
    occurred_at: &str,
) -> String {
    let item_value = item_json(
        &item.id,
        &item.menu_item_id,
        item.variant_id.as_deref(),
        item.quantity,
        item.unit_price_paise,
        item.line_total_paise,
        item.notes.as_deref(),
        modifiers,
    );
    serde_json::json!({
        "event_id": event_id,
        "event_type": EVENT_TYPE_ITEM_REMOVED,
        "occurred_at": occurred_at,
        "outlet_id": outlet_id,
        "schema_version": 1,
        "data": {
            "order_id": order_id,
            "item": item_value,
        }
    })
    .to_string()
}

/// Writes the `local_outbox` row for a `remove_order_item_with_outbox`
/// call. `event_type`/`payload_json` are derived here, not accepted from
/// the caller — see [`build_item_removed_payload`].
pub(crate) fn insert_item_removed_outbox(
    tx: &Transaction,
    outlet_id: &str,
    order_id: &str,
    item: &OrderItem,
    modifiers: &[OrderItemModifier],
    meta: &OrderItemRemovedMeta,
) -> DbResult<()> {
    let payload = build_item_removed_payload(
        outlet_id,
        order_id,
        item,
        modifiers,
        &meta.outbox_id,
        &meta.occurred_at,
    );
    insert_outbox_entry(
        tx,
        &NewOutboxEntry {
            id: meta.outbox_id.clone(),
            aggregate_type: "order".to_string(),
            aggregate_id: order_id.to_string(),
            event_type: EVENT_TYPE_ITEM_REMOVED.to_string(),
            payload_json: payload,
            created_at: meta.occurred_at.clone(),
        },
    )
}

/// Recomputes `subtotal_paise`/`total_paise` from the snapshot
/// `line_total_paise` already stored on the remaining order_item rows —
/// never from the live menu (CLAUDE.md: order lines snapshot price at
/// order time). Each `line_total_paise` was itself computed at write time
/// by [`compute_line_total_paise`] per the money invariant in
/// `0003_order_item_modifiers.sql`
/// (`line_total_paise = (unit_price_paise + SUM(modifier price_delta_paise)) * quantity`),
/// so summing them here is consistent with that invariant rather than a
/// second, potentially divergent, computation of it.
/// `discount_paise`/`tax_paise` are set by pricing/tax rules outside this
/// crate's scope and are left as they are; only the subtotal (driven by the
/// lines) and the total that follows from it are recomputed here. Bumps
/// `version` and `updated_at` on the order, matching the
/// optimistic-concurrency contract on `"order".version`.
pub(crate) fn recompute_and_persist_order_totals(
    tx: &Transaction,
    order_id: &str,
    updated_at: &str,
) -> DbResult<()> {
    let subtotal_paise: i64 = tx.query_row(
        "SELECT COALESCE(SUM(line_total_paise), 0) FROM order_item WHERE order_id = ?1",
        params![order_id],
        |row| row.get(0),
    )?;
    let (discount_paise, tax_paise): (i64, i64) = tx.query_row(
        "SELECT discount_paise, tax_paise FROM \"order\" WHERE id = ?1",
        params![order_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let total_paise = subtotal_paise - discount_paise + tax_paise;

    let changed = tx.execute(
        "UPDATE \"order\" SET subtotal_paise = ?1, total_paise = ?2, version = version + 1, updated_at = ?3
         WHERE id = ?4",
        params![subtotal_paise, total_paise, updated_at, order_id],
    )?;
    if changed == 0 {
        return Err(crate::error::DbError::NotFound("order"));
    }
    Ok(())
}

pub fn get_order(conn: &Connection, id: &str) -> DbResult<Option<Order>> {
    conn.query_row(
        "SELECT id, outlet_id, device_id, order_type, status, table_id,
                subtotal_paise, discount_paise, tax_paise, total_paise, version, sync_status,
                created_at, updated_at
         FROM \"order\" WHERE id = ?1",
        params![id],
        |row| {
            Ok(Order {
                id: row.get(0)?,
                outlet_id: row.get(1)?,
                device_id: row.get(2)?,
                order_type: row.get(3)?,
                status: row.get(4)?,
                table_id: row.get(5)?,
                subtotal_paise: row.get(6)?,
                discount_paise: row.get(7)?,
                tax_paise: row.get(8)?,
                total_paise: row.get(9)?,
                version: row.get(10)?,
                sync_status: row.get(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn list_order_items(conn: &Connection, order_id: &str) -> DbResult<Vec<OrderItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, order_id, menu_item_id, variant_id, quantity, unit_price_paise, line_total_paise, notes, created_at
         FROM order_item WHERE order_id = ?1 ORDER BY created_at",
    )?;
    let rows = stmt
        .query_map(params![order_id], |row| {
            Ok(OrderItem {
                id: row.get(0)?,
                order_id: row.get(1)?,
                menu_item_id: row.get(2)?,
                variant_id: row.get(3)?,
                quantity: row.get(4)?,
                unit_price_paise: row.get(5)?,
                line_total_paise: row.get(6)?,
                notes: row.get(7)?,
                created_at: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Basic order list for the outlet, most recent first — Milestone 1's
/// "reporting beyond a basic order list" boundary; nothing beyond this
/// query belongs in this crate.
pub fn list_orders_for_outlet(conn: &Connection, outlet_id: &str) -> DbResult<Vec<Order>> {
    let mut stmt = conn.prepare(
        "SELECT id, outlet_id, device_id, order_type, status, table_id,
                subtotal_paise, discount_paise, tax_paise, total_paise, version, sync_status,
                created_at, updated_at
         FROM \"order\" WHERE outlet_id = ?1 ORDER BY created_at DESC",
    )?;
    let rows = stmt
        .query_map(params![outlet_id], |row| {
            Ok(Order {
                id: row.get(0)?,
                outlet_id: row.get(1)?,
                device_id: row.get(2)?,
                order_type: row.get(3)?,
                status: row.get(4)?,
                table_id: row.get(5)?,
                subtotal_paise: row.get(6)?,
                discount_paise: row.get(7)?,
                tax_paise: row.get(8)?,
                total_paise: row.get(9)?,
                version: row.get(10)?,
                sync_status: row.get(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ------------------------------------------------------------ table_session --

pub(crate) fn insert_table_session(tx: &Transaction, s: &NewTableSession) -> DbResult<()> {
    tx.execute(
        "INSERT INTO table_session
            (id, outlet_id, table_id, state, current_order_id, guest_count,
             opened_by_user_id, opened_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            s.id,
            s.outlet_id,
            s.table_id,
            s.state,
            s.current_order_id,
            s.guest_count,
            s.opened_by_user_id,
            s.opened_at,
            s.created_at,
            s.updated_at,
        ],
    )?;
    Ok(())
}

/// Updates the mutable fields of an open table session (state,
/// current_order_id, guest_count, closed_at), bumping `version`. Only ever
/// called from within [`crate::Db::write_table_session`] alongside its
/// outbox row.
pub(crate) fn update_table_session(
    tx: &Transaction,
    id: &str,
    state: &str,
    current_order_id: Option<&str>,
    guest_count: i64,
    closed_at: Option<&str>,
    updated_at: &str,
) -> DbResult<()> {
    let changed = tx.execute(
        "UPDATE table_session
         SET state = ?1, current_order_id = ?2, guest_count = ?3, closed_at = ?4,
             version = version + 1, updated_at = ?5
         WHERE id = ?6",
        params![state, current_order_id, guest_count, closed_at, updated_at, id],
    )?;
    if changed == 0 {
        return Err(crate::error::DbError::NotFound("table_session"));
    }
    Ok(())
}

pub fn get_table_session(conn: &Connection, id: &str) -> DbResult<Option<TableSession>> {
    conn.query_row(
        "SELECT id, outlet_id, table_id, state, current_order_id, guest_count,
                opened_by_user_id, opened_at, closed_at, version, sync_status, created_at, updated_at
         FROM table_session WHERE id = ?1",
        params![id],
        row_to_table_session,
    )
    .optional()
    .map_err(Into::into)
}

pub fn get_open_table_session(conn: &Connection, table_id: &str) -> DbResult<Option<TableSession>> {
    conn.query_row(
        "SELECT id, outlet_id, table_id, state, current_order_id, guest_count,
                opened_by_user_id, opened_at, closed_at, version, sync_status, created_at, updated_at
         FROM table_session WHERE table_id = ?1 AND closed_at IS NULL",
        params![table_id],
        row_to_table_session,
    )
    .optional()
    .map_err(Into::into)
}

fn row_to_table_session(row: &rusqlite::Row) -> rusqlite::Result<TableSession> {
    Ok(TableSession {
        id: row.get(0)?,
        outlet_id: row.get(1)?,
        table_id: row.get(2)?,
        state: row.get(3)?,
        current_order_id: row.get(4)?,
        guest_count: row.get(5)?,
        opened_by_user_id: row.get(6)?,
        opened_at: row.get(7)?,
        closed_at: row.get(8)?,
        version: row.get(9)?,
        sync_status: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

// ------------------------------------------------------------------- kot ---
// Repository only, per task scope: no station-routing or ticket-generation
// logic here (that is Milestone 2, docs/spec/kitchen.md).

pub fn insert_kot(conn: &Connection, k: &Kot) -> DbResult<()> {
    conn.execute(
        "INSERT INTO kot (id, order_id, station, sequence, status, items_json, created_by_device_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            k.id,
            k.order_id,
            k.station,
            k.sequence,
            k.status,
            k.items_json,
            k.created_by_device_id,
            k.created_at,
            k.updated_at
        ],
    )?;
    Ok(())
}

pub fn list_kots_for_order(conn: &Connection, order_id: &str) -> DbResult<Vec<Kot>> {
    let mut stmt = conn.prepare(
        "SELECT id, order_id, station, sequence, status, items_json, created_by_device_id, created_at, updated_at
         FROM kot WHERE order_id = ?1 ORDER BY sequence",
    )?;
    let rows = stmt
        .query_map(params![order_id], |row| {
            Ok(Kot {
                id: row.get(0)?,
                order_id: row.get(1)?,
                station: row.get(2)?,
                sequence: row.get(3)?,
                status: row.get(4)?,
                items_json: row.get(5)?,
                created_by_device_id: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// -------------------------------------------------------------- outbox -----

pub(crate) fn insert_outbox_entry(tx: &Transaction, e: &NewOutboxEntry) -> DbResult<()> {
    tx.execute(
        "INSERT INTO local_outbox (id, aggregate_type, aggregate_id, event_type, payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![e.id, e.aggregate_type, e.aggregate_id, e.event_type, e.payload_json, e.created_at],
    )?;
    Ok(())
}

/// Unpublished outbox entries, oldest first — what the (separate) sync
/// worker pushes next. This crate provides the accessor only; HTTP push,
/// retry and cursor logic belong to `edge/sync`.
pub fn list_unpublished_outbox(conn: &Connection, limit: i64) -> DbResult<Vec<OutboxEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, aggregate_type, aggregate_id, event_type, payload_json, created_at, published_at, attempt_count
         FROM local_outbox WHERE published_at IS NULL ORDER BY created_at ASC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |row| {
            Ok(OutboxEntry {
                id: row.get(0)?,
                aggregate_type: row.get(1)?,
                aggregate_id: row.get(2)?,
                event_type: row.get(3)?,
                payload_json: row.get(4)?,
                created_at: row.get(5)?,
                published_at: row.get(6)?,
                attempt_count: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Marks an outbox row published (ack'd by cloud). Never deletes the row —
/// "never delete local transactions immediately after sync" (sync.md).
pub fn mark_outbox_published(conn: &Connection, id: &str, published_at: &str) -> DbResult<()> {
    conn.execute(
        "UPDATE local_outbox SET published_at = ?1 WHERE id = ?2",
        params![published_at, id],
    )?;
    Ok(())
}

pub fn increment_outbox_attempt(conn: &Connection, id: &str) -> DbResult<()> {
    conn.execute(
        "UPDATE local_outbox SET attempt_count = attempt_count + 1 WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------- sync_state --

pub fn get_sync_state(conn: &Connection, outlet_id: &str) -> DbResult<Option<SyncState>> {
    conn.query_row(
        "SELECT outlet_id, last_pushed_outbox_id, last_applied_config_version,
                last_sync_attempt_at, last_sync_success_at, is_online
         FROM sync_state WHERE outlet_id = ?1",
        params![outlet_id],
        |row| {
            Ok(SyncState {
                outlet_id: row.get(0)?,
                last_pushed_outbox_id: row.get(1)?,
                last_applied_config_version: row.get(2)?,
                last_sync_attempt_at: row.get(3)?,
                last_sync_success_at: row.get(4)?,
                is_online: i64_to_bool(row.get(5)?),
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn init_sync_state(conn: &Connection, outlet_id: &str) -> DbResult<()> {
    conn.execute(
        "INSERT INTO sync_state (outlet_id) VALUES (?1) ON CONFLICT(outlet_id) DO NOTHING",
        params![outlet_id],
    )?;
    Ok(())
}

pub fn update_sync_cursor(
    conn: &Connection,
    outlet_id: &str,
    last_pushed_outbox_id: Option<&str>,
    last_applied_config_version: i64,
    last_sync_attempt_at: Option<&str>,
    last_sync_success_at: Option<&str>,
    is_online: bool,
) -> DbResult<()> {
    conn.execute(
        "UPDATE sync_state SET
            last_pushed_outbox_id = ?1,
            last_applied_config_version = ?2,
            last_sync_attempt_at = ?3,
            last_sync_success_at = ?4,
            is_online = ?5
         WHERE outlet_id = ?6",
        params![
            last_pushed_outbox_id,
            last_applied_config_version,
            last_sync_attempt_at,
            last_sync_success_at,
            bool_to_i64(is_online),
            outlet_id
        ],
    )?;
    Ok(())
}
