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

/// **Validates `o.timezone` before writing anything.** This is the real
/// config-apply boundary for `outlet.timezone` — the sync worker's only
/// path for landing a cloud-authored outlet row — so an unparseable IANA
/// identifier is rejected HERE, as a whole-write config defect, rather than
/// being absorbed later where `business_date` is computed
/// (`crate::deduction::business_date`'s doc comment states the rule this
/// enforces: a silent fallback to a different valid value is worse than a
/// rejection, because a rejection is visible and a plausible-but-wrong
/// stored date is not).
pub fn upsert_outlet(conn: &Connection, o: &Outlet) -> DbResult<()> {
    crate::deduction::business_date::OutletTimezone::parse(&o.timezone)?;
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
        params![
            o.id,
            o.brand_id,
            o.name,
            o.timezone,
            o.config_version,
            o.created_at,
            o.updated_at
        ],
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

/// The two `business_date` inputs (ADR-018 §9.2, 0013) for one outlet, read
/// directly rather than through the [`Outlet`] struct/[`get_outlet`]
/// deliberately: `Outlet` is a public type constructed by name at several
/// call sites outside this crate (`edge/sync` config-bundle tests), and this
/// crate's contract with them is additive-only — adding a field to a struct
/// they build as an exhaustive literal would be a breaking change to a
/// workspace this task does not own. A narrow, transaction-scoped read
/// avoids that cascade entirely.
///
/// Returns the ALREADY-VALIDATED types, never the raw strings — see
/// `crate::deduction::business_date`'s doc comment for why a bare `String`
/// here would put the failure mode back one call away from where it was
/// removed. `timezone` should never actually fail to parse here (`upsert_
/// outlet` rejects a bad one before it can be stored); `day_start_time` has
/// no equivalent config-apply gate yet (see [`crate::deduction::business_date
/// ::DayStartTime`]'s doc comment for that residual gap), so this is its
/// real validation boundary today. Either failing is a typed, propagated
/// `DbError::InvalidInput`, never a silent substitution.
///
/// `DbError::NotFound("outlet")` if the outlet row is missing, which should
/// be unreachable for a `confirm_order` call (the order's own `outlet_id`
/// FK already guarantees the row exists) but is still a typed, propagated
/// `DbError` rather than a silent default — an outlet that has vanished out
/// from under a confirm is a genuine data-integrity failure, not a config
/// gap `business_date` should paper over.
pub(crate) fn get_outlet_business_date_config(
    tx: &Transaction,
    outlet_id: &str,
) -> DbResult<(
    crate::deduction::business_date::OutletTimezone,
    crate::deduction::business_date::DayStartTime,
)> {
    let (timezone_str, day_start_str): (String, String) = tx
        .query_row(
            "SELECT timezone, day_start_time FROM outlet WHERE id = ?1",
            params![outlet_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or(crate::error::DbError::NotFound("outlet"))?;
    let timezone = crate::deduction::business_date::OutletTimezone::parse(&timezone_str)?;
    let day_start_time = crate::deduction::business_date::DayStartTime::parse(&day_start_str)?;
    Ok((timezone, day_start_time))
}

/// Applies `outlet.day_start_time` from a config bundle (ADR-018 §9.2,
/// M4 T4b). This is the config-apply write path the field's own type
/// ([`crate::deduction::business_date::DayStartTime`]) documented as
/// missing — closing it here rather than leaving the residual gap that
/// doc comment named.
///
/// **Validates before writing anything**, the same posture [`upsert_outlet`]
/// already takes for `timezone`: an unparseable `HH:MM` is rejected as a
/// whole-bundle-apply defect (the caller's transaction rolls back), never
/// silently substituted and never absorbed later where `business_date` is
/// computed. Only `day_start_time` and `config_version` are touched — every
/// other outlet column is left alone, since the config-bundle apply path
/// this function serves carries no other outlet field today.
pub fn upsert_outlet_day_start_time(
    conn: &Connection,
    outlet_id: &str,
    day_start_time: &str,
    config_version: i64,
) -> DbResult<()> {
    crate::deduction::business_date::DayStartTime::parse(day_start_time)?;
    conn.execute(
        "UPDATE outlet SET day_start_time = ?2, config_version = ?3
         WHERE id = ?1 AND ?3 >= config_version",
        params![outlet_id, day_start_time, config_version],
    )?;
    Ok(())
}

/// Raw (unvalidated) read of `outlet.day_start_time`, for callers outside
/// this crate that just want to display or assert the stored value —
/// `get_outlet_business_date_config` above is the validated, `pub(crate)`
/// equivalent used on the money/stock path.
pub fn get_outlet_day_start_time(conn: &Connection, outlet_id: &str) -> DbResult<Option<String>> {
    conn.query_row(
        "SELECT day_start_time FROM outlet WHERE id = ?1",
        params![outlet_id],
        |row| row.get(0),
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
        params![
            d.id,
            d.outlet_id,
            d.kind,
            d.name,
            d.last_seen_at,
            d.created_at
        ],
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

const APP_USER_COLUMNS: &str =
    "id, tenant_id, outlet_id, email, full_name, password_hash, pin_hash, \
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

pub fn list_restaurant_tables(
    conn: &Connection,
    outlet_id: &str,
) -> DbResult<Vec<RestaurantTable>> {
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
        "INSERT INTO menu_item (id, outlet_id, category_id, name, base_price_paise, is_available, config_version, tax_profile_id, hsn_sac)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(id) DO UPDATE SET
            outlet_id = excluded.outlet_id, category_id = excluded.category_id, name = excluded.name,
            base_price_paise = excluded.base_price_paise, is_available = excluded.is_available,
            config_version = excluded.config_version, tax_profile_id = excluded.tax_profile_id,
            hsn_sac = excluded.hsn_sac
         WHERE excluded.config_version >= menu_item.config_version",
        params![
            m.id,
            m.outlet_id,
            m.category_id,
            m.name,
            m.base_price_paise,
            bool_to_i64(m.is_available),
            m.config_version,
            m.tax_profile_id,
            m.hsn_sac,
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
        params![
            v.id,
            v.menu_item_id,
            v.name,
            v.price_delta_paise,
            v.config_version
        ],
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
        "SELECT id, outlet_id, category_id, name, base_price_paise, is_available, config_version, tax_profile_id, hsn_sac
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
                tax_profile_id: row.get(7)?,
                hsn_sac: row.get(8)?,
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

// ----------------------------------- inventory_item / recipe config (M4) --
// CONFIG, cloud->edge (ADR-018). Same upsert-by-id, config_version-gated
// shape as every other config row in this file. No `list_*` reader is added
// here beyond what `crate::inventory::resolve` already queries directly —
// these five functions exist because `devseed` (and, later, the real config
// sync worker) needs a write path for tables that had none.

pub fn upsert_inventory_item(conn: &Connection, item: &InventoryItem) -> DbResult<()> {
    conn.execute(
        "INSERT INTO inventory_item
            (id, outlet_id, sku, name, category, dimension, reorder_level_micro,
             par_level_micro, storage_location, is_active, yield_factor_ppm, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(id) DO UPDATE SET
            outlet_id = excluded.outlet_id, sku = excluded.sku, name = excluded.name,
            category = excluded.category, dimension = excluded.dimension,
            reorder_level_micro = excluded.reorder_level_micro,
            par_level_micro = excluded.par_level_micro,
            storage_location = excluded.storage_location, is_active = excluded.is_active,
            yield_factor_ppm = excluded.yield_factor_ppm, config_version = excluded.config_version
         WHERE excluded.config_version >= inventory_item.config_version",
        params![
            item.id,
            item.outlet_id,
            item.sku,
            item.name,
            item.category,
            item.dimension,
            item.reorder_level_micro,
            item.par_level_micro,
            item.storage_location,
            bool_to_i64(item.is_active),
            item.yield_factor_ppm,
            item.config_version,
        ],
    )?;
    Ok(())
}

pub fn upsert_item_unit_conversion(conn: &Connection, c: &ItemUnitConversion) -> DbResult<()> {
    conn.execute(
        "INSERT INTO item_unit_conversion
            (id, inventory_item_id, pack_unit_label, source_dimension, numerator, denominator, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
            inventory_item_id = excluded.inventory_item_id,
            pack_unit_label = excluded.pack_unit_label,
            source_dimension = excluded.source_dimension,
            numerator = excluded.numerator, denominator = excluded.denominator,
            config_version = excluded.config_version
         WHERE excluded.config_version >= item_unit_conversion.config_version",
        params![
            c.id,
            c.inventory_item_id,
            c.pack_unit_label,
            c.source_dimension,
            c.numerator,
            c.denominator,
            c.config_version,
        ],
    )?;
    Ok(())
}

pub fn upsert_recipe(conn: &Connection, r: &Recipe) -> DbResult<()> {
    conn.execute(
        "INSERT INTO recipe
            (id, menu_item_variant_id, name, recipe_version, output_dimension, output_quantity_micro, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
            menu_item_variant_id = excluded.menu_item_variant_id, name = excluded.name,
            recipe_version = excluded.recipe_version, output_dimension = excluded.output_dimension,
            output_quantity_micro = excluded.output_quantity_micro,
            config_version = excluded.config_version
         WHERE excluded.config_version >= recipe.config_version",
        params![
            r.id,
            r.menu_item_variant_id,
            r.name,
            r.recipe_version,
            r.output_dimension,
            r.output_quantity_micro,
            r.config_version,
        ],
    )?;
    Ok(())
}

pub fn upsert_recipe_ingredient(conn: &Connection, i: &RecipeIngredient) -> DbResult<()> {
    conn.execute(
        "INSERT INTO recipe_ingredient
            (id, recipe_id, component_kind, inventory_item_id, sub_recipe_id, quantity_micro,
             quantity_dimension, yield_factor_ppm, sort_order, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET
            recipe_id = excluded.recipe_id, component_kind = excluded.component_kind,
            inventory_item_id = excluded.inventory_item_id, sub_recipe_id = excluded.sub_recipe_id,
            quantity_micro = excluded.quantity_micro, quantity_dimension = excluded.quantity_dimension,
            yield_factor_ppm = excluded.yield_factor_ppm, sort_order = excluded.sort_order,
            config_version = excluded.config_version
         WHERE excluded.config_version >= recipe_ingredient.config_version",
        params![
            i.id,
            i.recipe_id,
            i.component_kind,
            i.inventory_item_id,
            i.sub_recipe_id,
            i.quantity_micro,
            i.quantity_dimension,
            i.yield_factor_ppm,
            i.sort_order,
            i.config_version,
        ],
    )?;
    Ok(())
}

pub fn upsert_modifier_ingredient_delta(
    conn: &Connection,
    d: &ModifierIngredientDelta,
) -> DbResult<()> {
    conn.execute(
        "INSERT INTO modifier_ingredient_delta
            (id, menu_item_modifier_id, inventory_item_id, quantity_micro, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
            menu_item_modifier_id = excluded.menu_item_modifier_id,
            inventory_item_id = excluded.inventory_item_id,
            quantity_micro = excluded.quantity_micro, config_version = excluded.config_version
         WHERE excluded.config_version >= modifier_ingredient_delta.config_version",
        params![
            d.id,
            d.menu_item_modifier_id,
            d.inventory_item_id,
            d.quantity_micro,
            d.config_version,
        ],
    )?;
    Ok(())
}

/// Read-by-id getters for the five M4 inventory config shapes, added for
/// M4 T4b (the config-bundle sync apply path needs somewhere to prove a row
/// landed, from `edge/sync`, a separate crate — `crate::inventory::resolve`
/// queries these tables directly and did not need one before).
pub fn get_inventory_item(conn: &Connection, id: &str) -> DbResult<Option<InventoryItem>> {
    conn.query_row(
        "SELECT id, outlet_id, sku, name, category, dimension, reorder_level_micro,
                par_level_micro, storage_location, is_active, yield_factor_ppm, config_version
         FROM inventory_item WHERE id = ?1",
        params![id],
        |row| {
            Ok(InventoryItem {
                id: row.get(0)?,
                outlet_id: row.get(1)?,
                sku: row.get(2)?,
                name: row.get(3)?,
                category: row.get(4)?,
                dimension: row.get(5)?,
                reorder_level_micro: row.get(6)?,
                par_level_micro: row.get(7)?,
                storage_location: row.get(8)?,
                is_active: i64_to_bool(row.get(9)?),
                yield_factor_ppm: row.get(10)?,
                config_version: row.get(11)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn get_item_unit_conversion(conn: &Connection, id: &str) -> DbResult<Option<ItemUnitConversion>> {
    conn.query_row(
        "SELECT id, inventory_item_id, pack_unit_label, source_dimension, numerator, denominator, config_version
         FROM item_unit_conversion WHERE id = ?1",
        params![id],
        |row| {
            Ok(ItemUnitConversion {
                id: row.get(0)?,
                inventory_item_id: row.get(1)?,
                pack_unit_label: row.get(2)?,
                source_dimension: row.get(3)?,
                numerator: row.get(4)?,
                denominator: row.get(5)?,
                config_version: row.get(6)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn get_recipe(conn: &Connection, id: &str) -> DbResult<Option<Recipe>> {
    conn.query_row(
        "SELECT id, menu_item_variant_id, name, recipe_version, output_dimension, output_quantity_micro, config_version
         FROM recipe WHERE id = ?1",
        params![id],
        |row| {
            Ok(Recipe {
                id: row.get(0)?,
                menu_item_variant_id: row.get(1)?,
                name: row.get(2)?,
                recipe_version: row.get(3)?,
                output_dimension: row.get(4)?,
                output_quantity_micro: row.get(5)?,
                config_version: row.get(6)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn get_recipe_ingredient(conn: &Connection, id: &str) -> DbResult<Option<RecipeIngredient>> {
    conn.query_row(
        "SELECT id, recipe_id, component_kind, inventory_item_id, sub_recipe_id, quantity_micro,
                quantity_dimension, yield_factor_ppm, sort_order, config_version
         FROM recipe_ingredient WHERE id = ?1",
        params![id],
        |row| {
            Ok(RecipeIngredient {
                id: row.get(0)?,
                recipe_id: row.get(1)?,
                component_kind: row.get(2)?,
                inventory_item_id: row.get(3)?,
                sub_recipe_id: row.get(4)?,
                quantity_micro: row.get(5)?,
                quantity_dimension: row.get(6)?,
                yield_factor_ppm: row.get(7)?,
                sort_order: row.get(8)?,
                config_version: row.get(9)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn get_modifier_ingredient_delta(
    conn: &Connection,
    id: &str,
) -> DbResult<Option<ModifierIngredientDelta>> {
    conn.query_row(
        "SELECT id, menu_item_modifier_id, inventory_item_id, quantity_micro, config_version
         FROM modifier_ingredient_delta WHERE id = ?1",
        params![id],
        |row| {
            Ok(ModifierIngredientDelta {
                id: row.get(0)?,
                menu_item_modifier_id: row.get(1)?,
                inventory_item_id: row.get(2)?,
                quantity_micro: row.get(3)?,
                config_version: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

// ------------------------------------------------------------- "order" -----
// Reserved word: quoted in every statement, including inside format!
// strings, per the schema comment in 0001_init.sql.

/// Renders `block` (0-based) as a bijective base-26 letter string: `0` ->
/// `"A"`, ..., `25` -> `"Z"`, `26` -> `"AA"`, `27` -> `"AB"`, .... This is
/// the SAME scheme spreadsheet column headers use, chosen because it is a
/// genuine bijection over non-negative integers — no two blocks ever render
/// the same letters, for any `block`, without bound. That is precisely the
/// property the single-letter (`A-Z` mod 26) scheme this replaces did NOT
/// have: it wrapped at block 26 and silently re-emitted block 0's letter,
/// which is the T7b gate finding (`#Z999` -> `#A1`, duplicating the
/// outlet's very first order number). See
/// [`format_order_display_number`]'s doc comment for the fix as a whole.
fn block_letters(block: i64) -> String {
    let mut n = block + 1; // shift to 1-based for the bijective encoding
    let mut letters = Vec::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        letters.push((b'A' + rem as u8) as char);
        n = (n - 1) / 26;
    }
    letters.iter().rev().collect()
}

/// Formats a 1-based sequence index (scoped to one outlet's one business
/// day — see [`mint_order_display_number`]) as the short human-facing
/// `#A184`-shaped string (contracts 0.4.0, ADR-016 §6): `#` + bijective
/// base-26 letters (cycling `A`, `B`, ... `Z`, `AA`, `AB`, ... as blocks of
/// 999 are exhausted) + a `1-999` number within that block. Never a raw
/// sequential id — CLAUDE.md forbids exposing a sequential PK as an
/// identifier, and this is a display string derived from a count, not a key.
///
/// **The T7b fix, and why it is two changes, not one:** a gate found this
/// function wrapped at sequence 25975 (block 26, `999*26 = 25974`), where
/// the old single-letter scheme (`letter = (b'A' + block % 26) as u8`)
/// re-emitted block 0's letter — `#Z999` rolled to `#A1`, duplicating that
/// outlet's very first order number. Two cooks holding tickets with the
/// same number is the exact confusion `display_number` exists to prevent.
///
/// 1. [`block_letters`] replaces the `mod 26` letter with a bijective
///    base-26 encoding, so the FORMATTING FUNCTION ITSELF never produces a
///    repeat, for any sequence index up to `i64::MAX` — this is what a test
///    that "drives the counter past its wrap point" can actually prove,
///    since the old function's collision was in the pure math, not merely
///    in how far a database ever counted.
/// 2. [`mint_order_display_number`] additionally resets the counter every
///    business day rather than counting an outlet's lifetime orders, which
///    is the useful-in-practice half: `#A1` meaning "the first order today"
///    is what a restaurant actually wants to see and say aloud, and it
///    keeps the letter block small under normal volume instead of climbing
///    toward multi-letter blocks after weeks of service. A wider letter
///    space alone (this function) would have been sufficient to stop
///    duplicates; the per-day reset is the deliberate, disclosed choice of
///    which defensible fix to combine it with (per this track's brief) —
///    restaurants think in service days, not lifetime counts.
fn format_order_display_number(sequence_index: i64) -> String {
    let zero_based = sequence_index - 1;
    let block = zero_based / 999;
    let num = zero_based % 999 + 1;
    let letters = block_letters(block);
    format!("#{letters}{num}")
}

/// Mints the next short display number for `outlet_id`, SCOPED TO THE
/// ORDER'S OWN UTC CALENDAR DAY (the first 10 characters of `created_at`,
/// which this crate always writes as an ISO8601 UTC string), inside the
/// same transaction as the order insert it backs — the count-then-insert
/// runs on the one write-serialized SQLite connection this outlet's edge
/// process owns (ADR-013: a single writer over one file), so no two orders
/// can ever mint the same index for the same day.
///
/// **UTC calendar day, not the outlet-local business day.** `invoice.
/// business_date` (ADR-016) is the outlet-local day that may cross midnight
/// for a real accounting record, and getting that right needs the outlet's
/// timezone. `display_number` is a lighter-weight kitchen/till convenience,
/// not a legal document, so this crate deliberately buckets it by the
/// `"order".created_at` UTC date directly rather than pulling in timezone
/// conversion for a display string — a disclosed simplification, not an
/// oversight. The practical effect is a reset that lands within a few hours
/// of the true local business-day boundary rather than exactly on it.
///
/// Counting `"order"` rows for the outlet on this UTC day is deliberately
/// used rather than a dedicated counter table: `packages/contracts` is
/// frozen and read-only to this crate (ADR-008), and no sequence table for
/// order numbering exists in it, so this stays entirely inside the one
/// column the contract actually added (`display_number`).
pub(crate) fn mint_order_display_number(
    tx: &Transaction,
    outlet_id: &str,
    created_at: &str,
) -> DbResult<String> {
    let day = created_at.get(0..10).ok_or_else(|| {
        crate::error::DbError::InvalidInput(format!(
            "created_at {created_at:?} is too short to carry a YYYY-MM-DD date"
        ))
    })?;
    let existing: i64 = tx.query_row(
        "SELECT COUNT(*) FROM \"order\" WHERE outlet_id = ?1 AND substr(created_at, 1, 10) = ?2",
        params![outlet_id, day],
        |row| row.get(0),
    )?;
    Ok(format_order_display_number(existing + 1))
}

#[cfg(test)]
mod order_display_number_wrap_tests {
    use super::*;

    /// The exact defect the T7b gate found: the OLD formatter
    /// (`letter = (b'A' + block % 26) as u8`) produced `format_order_display_number(25975) == "#A1"`,
    /// identical to `format_order_display_number(1)`. This test drives the
    /// counter past that boundary and asserts the CURRENT formatter never
    /// repeats — it fails immediately against a regression back to the
    /// `mod 26` scheme.
    #[test]
    fn formatter_never_repeats_past_the_old_wrap_point() {
        let old_wrap_point = 26 * 999 + 1; // 25975: where the old scheme repeated "#A1"
        let mut seen = std::collections::HashSet::new();
        for i in 1..=(old_wrap_point + 5_000) {
            let s = format_order_display_number(i);
            assert!(
                seen.insert(s.clone()),
                "display number {s:?} (index {i}) repeated an earlier index — the wrap regressed"
            );
        }
        // Pin the exact old-collision indices to two DIFFERENT strings.
        let at_index_1 = format_order_display_number(1);
        let at_old_wrap = format_order_display_number(old_wrap_point);
        assert_eq!(at_index_1, "#A1");
        assert_ne!(
            at_old_wrap, at_index_1,
            "index {old_wrap_point} must not collide with index 1 (the old #Z999 -> #A1 defect)"
        );
        assert_eq!(
            at_old_wrap, "#AA1",
            "block 26 must render as the two-letter block AA, never a repeat of A"
        );
    }

    /// Drives a full run from 1 well past several old wrap boundaries
    /// (26, 52, 78 blocks — i.e. the old scheme's 1st/2nd/3rd trips around
    /// the alphabet) and asserts global uniqueness across the entire run,
    /// not just around one boundary.
    #[test]
    fn formatter_is_unique_across_many_wrap_boundaries() {
        let mut seen = std::collections::HashSet::new();
        for i in 1..=(78 * 999 + 100) {
            let s = format_order_display_number(i);
            assert!(
                seen.insert(s),
                "duplicate display number produced at index {i}"
            );
        }
    }

    /// `block_letters` is a genuine bijection: distinct blocks must never
    /// render identical letters, which is what actually guarantees
    /// [`format_order_display_number`] cannot wrap into a duplicate no
    /// matter how large the counter grows.
    #[test]
    fn block_letters_never_collide() {
        let mut seen = std::collections::HashSet::new();
        for block in 0..3000 {
            let letters = block_letters(block);
            assert!(
                seen.insert(letters),
                "block_letters({block}) collided with an earlier block"
            );
        }
    }

    /// [`mint_order_display_number`] resets to `#A1` for a NEW UTC calendar
    /// day even after many orders were minted on a previous day — the
    /// per-business-day half of the T7b fix. A design choice test, not just
    /// a uniqueness one: it fails if a future change reverts to a lifetime
    /// counter.
    #[test]
    fn mint_resets_to_a1_on_a_new_utc_day() {
        let mut db = crate::Db::open_in_memory_for_tests().expect("open db");
        upsert_outlet(
            db.connection(),
            &Outlet {
                id: "outlet-1".to_string(),
                brand_id: "brand-1".to_string(),
                name: "Test Outlet".to_string(),
                timezone: "Asia/Kolkata".to_string(),
                config_version: 1,
                created_at: "2026-08-01T00:00:00Z".to_string(),
                updated_at: "2026-08-01T00:00:00Z".to_string(),
            },
        )
        .expect("seed outlet");
        upsert_device(
            db.connection(),
            &Device {
                id: "device-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                kind: "POS".to_string(),
                name: "POS 1".to_string(),
                last_seen_at: None,
                created_at: "2026-08-01T00:00:00Z".to_string(),
            },
        )
        .expect("seed device");

        fn new_order(id: &str, created_at: &str) -> NewOrder {
            NewOrder {
                id: id.to_string(),
                outlet_id: "outlet-1".to_string(),
                device_id: "device-1".to_string(),
                order_type: "DINE_IN".to_string(),
                status: "DRAFT".to_string(),
                table_id: None,
                subtotal_paise: 0,
                discount_paise: 0,
                taxes_paise: 0,
                total_paise: 0,
                source: "POS".to_string(),
                external_order_id: None,
                payment_status: "UNPAID".to_string(),
                payment_source: None,
                confirmed_at: None,
                source_payload_json: None,
                schema_version: 1,
                created_at: created_at.to_string(),
                updated_at: created_at.to_string(),
            }
        }

        fn outbox_for(order_id: &str, seq: i64) -> NewOutboxEntry {
            NewOutboxEntry {
                id: format!("outbox-{seq}"),
                aggregate_type: "order".to_string(),
                aggregate_id: order_id.to_string(),
                event_type: "OrderCreated".to_string(),
                payload_json: r#"{"data":{"order":{}}}"#.to_string(),
                created_at: "2026-08-12T10:00:00Z".to_string(),
            }
        }

        db.create_order_with_outbox(
            &new_order("order-1", "2026-08-12T10:00:00Z"),
            &[],
            &outbox_for("order-1", 1),
        )
        .expect("insert order 1");
        let n1 = get_order(db.connection(), "order-1")
            .expect("read order 1")
            .expect("order 1 exists")
            .display_number
            .expect("order 1 has a display number");
        assert_eq!(n1, "#A1");

        db.create_order_with_outbox(
            &new_order("order-2", "2026-08-12T11:00:00Z"),
            &[],
            &outbox_for("order-2", 2),
        )
        .expect("insert order 2");
        let n2 = get_order(db.connection(), "order-2")
            .expect("read order 2")
            .expect("order 2 exists")
            .display_number
            .expect("order 2 has a display number");
        assert_eq!(n2, "#A2", "same UTC day must continue the count");

        db.create_order_with_outbox(
            &new_order("order-3", "2026-08-13T00:00:01Z"),
            &[],
            &outbox_for("order-3", 3),
        )
        .expect("insert order 3");
        let n3 = get_order(db.connection(), "order-3")
            .expect("read order 3")
            .expect("order 3 exists")
            .display_number
            .expect("order 3 has a display number");
        assert_eq!(n3, "#A1", "a new UTC day must reset the count to #A1");
    }
}

/// Inserts the `"order"` row, minting its `display_number` internally, and
/// returns the number minted so the caller can patch it into the
/// `OrderCreated` outbox payload built before the insert ran — see
/// [`patch_order_created_display_number`].
pub(crate) fn insert_order(tx: &Transaction, o: &NewOrder) -> DbResult<String> {
    let display_number = mint_order_display_number(tx, &o.outlet_id, &o.created_at)?;
    tx.execute(
        "INSERT INTO \"order\"
            (id, outlet_id, device_id, order_type, status, table_id,
             subtotal_paise, discount_paise, taxes_paise, total_paise,
             source, external_order_id, payment_status, payment_source,
             confirmed_at, source_payload_json, schema_version,
             created_at, updated_at, display_number)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        params![
            o.id,
            o.outlet_id,
            o.device_id,
            o.order_type,
            o.status,
            o.table_id,
            o.subtotal_paise,
            o.discount_paise,
            o.taxes_paise,
            o.total_paise,
            o.source,
            o.external_order_id,
            o.payment_status,
            o.payment_source,
            o.confirmed_at,
            o.source_payload_json,
            o.schema_version,
            o.created_at,
            o.updated_at,
            display_number,
        ],
    )?;
    Ok(display_number)
}

/// Best-effort patch of an `OrderCreated` outbox payload's
/// `data.order.display_number`, called by [`crate::Db::create_order_with_outbox`]
/// / [`crate::Db::create_order_with_outbox_and_modifiers`] after
/// [`insert_order`] has minted the real number — the DTO the caller built the
/// payload from (`CanonicalOrder::from_new_order_and_items` in
/// `apps/pos/src-tauri`) is constructed *before* the row is persisted, so it
/// cannot know the minted value itself. Mirrors the lenient
/// parse-or-leave-unchanged shape `correct_pending_item_added_quantity`
/// already uses: a malformed payload is a caller bug elsewhere, not
/// something this patch step should mask by erroring the whole create.
pub(crate) fn patch_order_created_display_number(
    payload_json: &str,
    display_number: &str,
) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(payload_json) else {
        return payload_json.to_string();
    };
    if let Some(order) = value
        .get_mut("data")
        .and_then(|d| d.get_mut("order"))
        .and_then(|o| o.as_object_mut())
    {
        order.insert(
            "display_number".to_string(),
            serde_json::json!(display_number),
        );
    }
    serde_json::to_string(&value).unwrap_or_else(|_| payload_json.to_string())
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

/// Enforces "a line may be added, or an existing line's quantity changed,
/// while the order is still active" — the wider gate `#132-A` (post-DRAFT
/// item addition, docs/spec/kitchen.md's #132 -> #132-A change history) and
/// `SET_ORDER_ITEM_QUANTITY` need, versus [`require_draft_order`]'s
/// DRAFT-only gate that still guards removal and the order-shape correction.
/// Legal statuses are `DRAFT` (still building the cart), `CONFIRMED` (sent to
/// billing but not yet the kitchen) and `SENT_TO_KITCHEN`/`PREPARING` (the
/// kitchen already has some tickets, and a later
/// `send_order_to_kitchen_with_outbox` call is idempotent-by-delta, so a line
/// added or resized here reaches the kitchen as a fresh #132-A-style ticket
/// rather than mutating one already in flight). `READY`/`SERVED`/`BILLED`/
/// `PAID`/`CLOSED`/`CANCELLED` are terminal for line changes — by that point
/// the correction belongs to a new order or an explicit reopen, not a silent
/// edit of a bill already on its way out. Returns the order's `outlet_id` on
/// success. Returns `DbError::NotFound` if the order does not exist,
/// `DbError::OrderNotAmendable` if it exists but is in a terminal status.
pub(crate) fn require_amendable_for_item_changes(
    tx: &Transaction,
    order_id: &str,
) -> DbResult<String> {
    let row: Option<(String, String)> = tx
        .query_row(
            "SELECT outlet_id, status FROM \"order\" WHERE id = ?1",
            params![order_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match row {
        None => Err(crate::error::DbError::NotFound("order")),
        Some((outlet_id, status))
            if matches!(
                status.as_str(),
                "DRAFT" | "CONFIRMED" | "SENT_TO_KITCHEN" | "PREPARING"
            ) =>
        {
            Ok(outlet_id)
        }
        Some((_, status)) => Err(crate::error::DbError::OrderNotAmendable {
            order_id: order_id.to_string(),
            status,
        }),
    }
}

/// Enforces "confirmation is only legal from DRAFT" inside the same
/// transaction as the write it is guarding, so the check and the mutation it
/// protects can never race — the confirm-path analogue of
/// [`require_draft_order`]. Returns the order's `outlet_id` on success.
/// Returns `DbError::NotFound` if the order does not exist,
/// `DbError::OrderNotConfirmable` if it exists but is not DRAFT.
pub(crate) fn require_draft_order_for_confirm(
    tx: &Transaction,
    order_id: &str,
) -> DbResult<String> {
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
        Some((_, status)) => Err(crate::error::DbError::OrderNotConfirmable {
            order_id: order_id.to_string(),
            status,
        }),
    }
}

/// Stamps `status = 'CONFIRMED'`, `confirmed_at`, bumps `version` and
/// `updated_at` on the order row. Must only ever be called after
/// [`require_draft_order_for_confirm`] has passed in the same transaction.
pub(crate) fn stamp_order_confirmed(
    tx: &Transaction,
    order_id: &str,
    confirmed_at: &str,
    updated_at: &str,
) -> DbResult<()> {
    let changed = tx.execute(
        "UPDATE \"order\" SET status = 'CONFIRMED', confirmed_at = ?1, version = version + 1, updated_at = ?2
         WHERE id = ?3",
        params![confirmed_at, updated_at, order_id],
    )?;
    if changed == 0 {
        return Err(crate::error::DbError::NotFound("order"));
    }
    Ok(())
}

/// Sets `order_type`/`table_id`, bumps `version` and `updated_at` on the
/// order row. Must only ever be called after [`require_draft_order`] has
/// passed in the same transaction — this function itself does not check
/// status, matching the split already used by `stamp_order_confirmed`/
/// `recompute_and_persist_order_totals`.
pub(crate) fn update_order_shape(
    tx: &Transaction,
    order_id: &str,
    order_type: &str,
    table_id: Option<&str>,
    updated_at: &str,
) -> DbResult<()> {
    let changed = tx.execute(
        "UPDATE \"order\" SET order_type = ?1, table_id = ?2, version = version + 1, updated_at = ?3
         WHERE id = ?4",
        params![order_type, table_id, updated_at, order_id],
    )?;
    if changed == 0 {
        return Err(crate::error::DbError::NotFound("order"));
    }
    Ok(())
}

/// Best-effort correction of the still-unpublished `OrderCreated`
/// `local_outbox` row for this order, called from
/// [`crate::Db::update_order_shape_with_outbox`]. `order_type`/`table_id`
/// are part of the `OrderCreated` snapshot; the frozen event catalog
/// (`packages/contracts/src/types/events.ts`) has no separate "order shape
/// changed" event, so this crate does not invent one — instead, while that
/// specific event has not yet left the device (`published_at IS NULL`), its
/// payload is corrected in place so the cloud never observes the
/// pre-correction shape. A no-op (0 rows touched) once the event has
/// already published — see the doc comment on
/// [`crate::Db::update_order_shape_with_outbox`] for the residual gap that
/// leaves.
pub(crate) fn update_pending_order_created_payload(
    tx: &Transaction,
    order_id: &str,
    payload_json: &str,
) -> DbResult<()> {
    tx.execute(
        "UPDATE local_outbox SET payload_json = ?1
         WHERE aggregate_id = ?2 AND event_type = ?3 AND published_at IS NULL",
        params![payload_json, order_id, EVENT_TYPE_ORDER_CREATED],
    )?;
    Ok(())
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

/// Public (outside a write transaction) twin of [`get_order_item_in_tx`] —
/// for read paths that just need one line by id (e.g. before calling
/// [`crate::Db::update_order_item_quantity_with_outbox`] to render its
/// current quantity, or in tests).
pub fn get_order_item(conn: &Connection, id: &str) -> DbResult<Option<OrderItem>> {
    conn.query_row(
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

/// Sets `quantity`/`line_total_paise` on an existing `order_item` row — the
/// single write behind `SET_ORDER_ITEM_QUANTITY`
/// (`packages/contracts/src/types/order.ts`, contracts 0.4.0/ADR-016).
/// Deliberately an `UPDATE` of the existing row, never a delete-then-insert:
/// that would be two durable writes with a crash window between them, which
/// is exactly the loss the durable-cart work (`4b0c560`) eliminated
/// (docs/backlog-m2.md, docs/retro.md 2026-08-10). `line_total_paise` is
/// supplied by the caller ([`crate::Db::update_order_item_quantity_with_outbox`],
/// which recomputes it from the row's own `unit_price_paise` and its real
/// `order_item_modifier` rows via [`compute_line_total_paise`]) rather than
/// derived again here, so there is one definition of the money invariant,
/// not two.
pub(crate) fn update_order_item_quantity(
    tx: &Transaction,
    order_item_id: &str,
    quantity: i64,
    line_total_paise: i64,
) -> DbResult<()> {
    let changed = tx.execute(
        "UPDATE order_item SET quantity = ?1, line_total_paise = ?2 WHERE id = ?3",
        params![quantity, line_total_paise, order_item_id],
    )?;
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

/// Every modifier selection across every line of one order, grouped by
/// `order_item_id` — one query rather than N (a caller building a whole
/// order's read-back would otherwise issue one [`list_order_item_modifiers`]
/// call per line). Used by the POS command layer to fill in
/// `CanonicalOrder.items[].modifiers` on every order read path (`get_order`,
/// `list_orders`, `get_active_draft_order`, and the return value of every
/// mutation), which is what makes a modifier's `price_delta_paise` visible to
/// a caller after the write, not just present in the outbox event.
pub fn list_order_item_modifiers_for_order(
    conn: &Connection,
    order_id: &str,
) -> DbResult<std::collections::HashMap<String, Vec<OrderItemModifier>>> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.order_item_id, m.modifier_id, m.group_name, m.option_name, m.price_delta_paise, m.created_at
         FROM order_item_modifier m
         JOIN order_item i ON i.id = m.order_item_id
         WHERE i.order_id = ?1
         ORDER BY m.order_item_id, m.created_at, m.id",
    )?;
    let rows = stmt
        .query_map(params![order_id], row_to_order_item_modifier)?
        .collect::<Result<Vec<_>, _>>()?;
    let mut by_item: std::collections::HashMap<String, Vec<OrderItemModifier>> =
        std::collections::HashMap::new();
    for row in rows {
        by_item
            .entry(row.order_item_id.clone())
            .or_default()
            .push(row);
    }
    Ok(by_item)
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
/// Added at contracts 0.4.1 (ADR-016 addendum) — see
/// [`insert_item_quantity_changed_outbox`].
const EVENT_TYPE_ITEM_QUANTITY_CHANGED: &str = "ItemQuantityChanged";
const EVENT_TYPE_ORDER_CONFIRMED: &str = "OrderConfirmed";
/// Referenced (not written) by [`update_pending_order_created_payload`] —
/// this crate never originates a second `OrderCreated` row, only corrects
/// the one [`crate::Db::create_order_with_outbox`] already wrote, while it
/// is still unpublished.
const EVENT_TYPE_ORDER_CREATED: &str = "OrderCreated";

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

/// Best-effort correction of a still-unpublished `ItemAdded` payload's
/// `quantity`/`line_total_paise` fields, called from
/// [`crate::Db::update_order_item_quantity_with_outbox`] — the `ItemAdded`
/// analogue of [`update_pending_order_created_payload`].
///
/// **Why this exists rather than a dedicated event:** the frozen event
/// catalog (`packages/contracts/src/types/events.ts` `OUTBOX_EVENT_TYPES`)
/// has no "item quantity changed" event as of contracts 0.4.0 — only the
/// `SET_ORDER_ITEM_QUANTITY` *command* landed there, not a corresponding
/// outbox *event*. This crate never originates a wire event outside that
/// catalog (ADR-008; contracts are read-only to builder agents). So while the
/// line's own `ItemAdded` event has not yet left the device
/// (`published_at IS NULL`), the quantity change is folded into that
/// still-pending snapshot — a correction of a not-yet-observed fact, not a
/// second fact needing a second event, matching the shape
/// `update_pending_order_created_payload` already uses for order-shape
/// corrections.
///
/// A no-op (0 rows touched) if no unpublished `ItemAdded` row exists for
/// this exact `order_item_id` — either it was added and already published
/// (residual gap below), or (impossible in practice, since a quantity change
/// requires an existing line) it was never added at all.
///
/// **Residual gap, called out rather than hidden:** if the outlet is online
/// and the sync worker has already published this line's `ItemAdded` event
/// by the time the cashier changes its quantity, the cloud's copy stays
/// stale until a future milestone adds a proper "item quantity changed"
/// event (needs a contract ADR — out of this crate's authority today). The
/// edge's own `order_item` row is always correct regardless; only the
/// cloud's already-delivered copy of the original `ItemAdded` event can go
/// stale, exactly as `update_order_shape_with_outbox`'s doc comment
/// describes for order shape.
pub(crate) fn correct_pending_item_added_quantity(
    tx: &Transaction,
    order_id: &str,
    order_item_id: &str,
    quantity: i64,
    line_total_paise: i64,
) -> DbResult<()> {
    let mut stmt = tx.prepare(
        "SELECT id, payload_json FROM local_outbox
         WHERE aggregate_id = ?1 AND event_type = ?2 AND published_at IS NULL",
    )?;
    let candidates: Vec<(String, String)> = stmt
        .query_map(params![order_id, EVENT_TYPE_ITEM_ADDED], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    for (outbox_id, payload_json) in candidates {
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&payload_json) else {
            continue;
        };
        let matches_this_item = value
            .get("data")
            .and_then(|d| d.get("item"))
            .and_then(|i| i.get("id"))
            .and_then(|id| id.as_str())
            == Some(order_item_id);
        if !matches_this_item {
            continue;
        }
        if let Some(item) = value
            .get_mut("data")
            .and_then(|d| d.get_mut("item"))
            .and_then(|i| i.as_object_mut())
        {
            item.insert("quantity".to_string(), serde_json::json!(quantity));
            item.insert(
                "line_total_paise".to_string(),
                serde_json::json!(line_total_paise),
            );
            if let Ok(corrected) = serde_json::to_string(&value) {
                tx.execute(
                    "UPDATE local_outbox SET payload_json = ?1 WHERE id = ?2",
                    params![corrected, outbox_id],
                )?;
            }
        }
        // Exactly one ItemAdded row can ever describe this order_item_id —
        // it is minted once, at add-time — so the first match is the only
        // match.
        break;
    }
    Ok(())
}

/// Builds the `ItemQuantityChanged` event envelope + `data` payload from the
/// `order_item` row's *post-update* state, matching
/// `ItemQuantityChangedEventSchema` in
/// `packages/contracts/src/types/events.ts` exactly (`{ event_id,
/// event_type, occurred_at, outlet_id, schema_version, data: { order_id,
/// item, previous_quantity } }`, contracts 0.4.1, ADR-016 addendum). The
/// full corrected line travels in the payload, not a quantity delta — see
/// the schema's own doc comment for the §50.1 reasoning: the edge computes
/// money, the cloud only stores what it is told.
#[allow(clippy::too_many_arguments)]
fn build_item_quantity_changed_payload(
    outlet_id: &str,
    order_id: &str,
    item_id: &str,
    menu_item_id: &str,
    variant_id: Option<&str>,
    quantity: i64,
    unit_price_paise: i64,
    line_total_paise: i64,
    notes: Option<&str>,
    modifiers: &[OrderItemModifier],
    previous_quantity: i64,
    event_id: &str,
    occurred_at: &str,
) -> String {
    let item_value = item_json(
        item_id,
        menu_item_id,
        variant_id,
        quantity,
        unit_price_paise,
        line_total_paise,
        notes,
        modifiers,
    );
    serde_json::json!({
        "event_id": event_id,
        "event_type": EVENT_TYPE_ITEM_QUANTITY_CHANGED,
        "occurred_at": occurred_at,
        "outlet_id": outlet_id,
        "schema_version": 1,
        "data": {
            "order_id": order_id,
            "item": item_value,
            "previous_quantity": previous_quantity,
        }
    })
    .to_string()
}

/// Writes the `local_outbox` row for a `SET_ORDER_ITEM_QUANTITY` command —
/// the frozen `ItemQuantityChanged` event (contracts 0.4.1, ADR-016
/// addendum), which closes the money-staleness hole
/// `correct_pending_item_added_quantity` alone could not: once a line's
/// `ItemAdded` event has actually published, nothing else corrected the
/// cloud's `quantity`/`line_total_paise` for a later quantity change. This
/// event is emitted on *every* quantity change, published or not — a replay
/// applies `ItemAdded` (old quantity) then `ItemQuantityChanged` (the
/// correction) in order, so it is correct regardless of whether the
/// `ItemAdded` row happened to still be in-flight. `event_type`/`payload_json`
/// are derived here, not accepted from the caller, matching every other
/// outbox writer in this module.
#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_item_quantity_changed_outbox(
    tx: &Transaction,
    outlet_id: &str,
    order_id: &str,
    item_id: &str,
    menu_item_id: &str,
    variant_id: Option<&str>,
    quantity: i64,
    unit_price_paise: i64,
    line_total_paise: i64,
    notes: Option<&str>,
    modifiers: &[OrderItemModifier],
    previous_quantity: i64,
    meta: &OrderItemQuantitySetMeta,
) -> DbResult<()> {
    let payload = build_item_quantity_changed_payload(
        outlet_id,
        order_id,
        item_id,
        menu_item_id,
        variant_id,
        quantity,
        unit_price_paise,
        line_total_paise,
        notes,
        modifiers,
        previous_quantity,
        &meta.outbox_id,
        &meta.occurred_at,
    );
    insert_outbox_entry(
        tx,
        &NewOutboxEntry {
            id: meta.outbox_id.clone(),
            aggregate_type: "order".to_string(),
            aggregate_id: order_id.to_string(),
            event_type: EVENT_TYPE_ITEM_QUANTITY_CHANGED.to_string(),
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

/// Builds the `OrderConfirmed` event envelope + `data` payload from the
/// order this crate just stamped CONFIRMED, matching
/// `OrderConfirmedEventSchema` in `packages/contracts/src/types/events.ts`
/// exactly (`{ event_id, event_type, occurred_at, outlet_id, schema_version,
/// data: { order_id, confirmed_at } }`). The caller never supplies
/// `event_type` or `confirmed_at`'s appearance in the payload directly —
/// both are derived here from the meta the crate itself validated and
/// wrote, so a caller cannot commit a mismatched event for a real
/// confirmation.
fn build_order_confirmed_payload(
    outlet_id: &str,
    order_id: &str,
    confirmed_at: &str,
    event_id: &str,
    occurred_at: &str,
) -> String {
    serde_json::json!({
        "event_id": event_id,
        "event_type": EVENT_TYPE_ORDER_CONFIRMED,
        "occurred_at": occurred_at,
        "outlet_id": outlet_id,
        "schema_version": 1,
        "data": {
            "order_id": order_id,
            "confirmed_at": confirmed_at,
        }
    })
    .to_string()
}

/// Writes the `local_outbox` row for a `confirm_order_with_outbox` call.
/// `event_type`/`payload_json` are derived here, not accepted from the
/// caller — see [`build_order_confirmed_payload`].
pub(crate) fn insert_order_confirmed_outbox(
    tx: &Transaction,
    outlet_id: &str,
    order_id: &str,
    meta: &OrderConfirmedMeta,
) -> DbResult<()> {
    let payload = build_order_confirmed_payload(
        outlet_id,
        order_id,
        &meta.confirmed_at,
        &meta.outbox_id,
        &meta.occurred_at,
    );
    insert_outbox_entry(
        tx,
        &NewOutboxEntry {
            id: meta.outbox_id.clone(),
            aggregate_type: "order".to_string(),
            aggregate_id: order_id.to_string(),
            event_type: EVENT_TYPE_ORDER_CONFIRMED.to_string(),
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
/// `discount_paise`/`taxes_paise` are set by pricing/tax rules outside this
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
    let (discount_paise, taxes_paise): (i64, i64) = tx.query_row(
        "SELECT discount_paise, taxes_paise FROM \"order\" WHERE id = ?1",
        params![order_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let total_paise = subtotal_paise - discount_paise + taxes_paise;

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

const ORDER_COLUMNS: &str = "id, outlet_id, device_id, order_type, status, table_id,
                subtotal_paise, discount_paise, taxes_paise, total_paise,
                source, external_order_id, payment_status, payment_source,
                confirmed_at, source_payload_json, schema_version,
                version, sync_status, created_at, updated_at, display_number";

fn order_from_row(row: &rusqlite::Row) -> rusqlite::Result<Order> {
    Ok(Order {
        id: row.get(0)?,
        outlet_id: row.get(1)?,
        device_id: row.get(2)?,
        order_type: row.get(3)?,
        status: row.get(4)?,
        table_id: row.get(5)?,
        subtotal_paise: row.get(6)?,
        discount_paise: row.get(7)?,
        taxes_paise: row.get(8)?,
        total_paise: row.get(9)?,
        source: row.get(10)?,
        external_order_id: row.get(11)?,
        payment_status: row.get(12)?,
        payment_source: row.get(13)?,
        confirmed_at: row.get(14)?,
        source_payload_json: row.get(15)?,
        schema_version: row.get(16)?,
        version: row.get(17)?,
        sync_status: row.get(18)?,
        created_at: row.get(19)?,
        updated_at: row.get(20)?,
        display_number: row.get(21)?,
    })
}

pub fn get_order(conn: &Connection, id: &str) -> DbResult<Option<Order>> {
    conn.query_row(
        &format!("SELECT {ORDER_COLUMNS} FROM \"order\" WHERE id = ?1"),
        params![id],
        order_from_row,
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

/// Transaction-scoped twin of [`list_order_items`], for callers already
/// inside a write transaction (e.g. `send_order_to_kitchen_with_outbox`)
/// that must read a consistent snapshot of the order's lines.
pub(crate) fn list_order_items_in_tx(tx: &Transaction, order_id: &str) -> DbResult<Vec<OrderItem>> {
    let mut stmt = tx.prepare(
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
    let mut stmt = conn.prepare(&format!(
        "SELECT {ORDER_COLUMNS} FROM \"order\" WHERE outlet_id = ?1 ORDER BY created_at DESC"
    ))?;
    let rows = stmt
        .query_map(params![outlet_id], order_from_row)?
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
        params![
            state,
            current_order_id,
            guest_count,
            closed_at,
            updated_at,
            id
        ],
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

fn row_to_kot(row: &rusqlite::Row) -> rusqlite::Result<Kot> {
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
}

const KOT_COLUMNS_ALIASED: &str = "k.id, k.order_id, k.station, k.sequence, k.status, \
     k.items_json, k.created_by_device_id, k.created_at, k.updated_at";

/// KOTs for an outlet (joined through `"order"`, since `kot` carries no
/// `outlet_id` of its own), optionally narrowed to one station `code` — the
/// query a KDS/expo screen or the LAN server needs to answer "what's on
/// this station's pass right now".
pub fn list_kots_for_outlet(
    conn: &Connection,
    outlet_id: &str,
    station: Option<&str>,
) -> DbResult<Vec<Kot>> {
    let rows: Vec<Kot> = match station {
        Some(code) => {
            let mut stmt = conn.prepare(&format!(
                "SELECT {KOT_COLUMNS_ALIASED} FROM kot k \
                 JOIN \"order\" o ON o.id = k.order_id \
                 WHERE o.outlet_id = ?1 AND k.station = ?2 \
                 ORDER BY k.created_at"
            ))?;
            let result = stmt
                .query_map(params![outlet_id, code], row_to_kot)?
                .collect::<Result<Vec<_>, _>>()?;
            result
        }
        None => {
            let mut stmt = conn.prepare(&format!(
                "SELECT {KOT_COLUMNS_ALIASED} FROM kot k \
                 JOIN \"order\" o ON o.id = k.order_id \
                 WHERE o.outlet_id = ?1 \
                 ORDER BY k.created_at"
            ))?;
            let result = stmt
                .query_map(params![outlet_id], row_to_kot)?
                .collect::<Result<Vec<_>, _>>()?;
            result
        }
    };
    Ok(rows)
}

// --------------------------------------------- Milestone 2: kitchen config --
// station / menu_item_station / printer / station_printer are CONFIG
// aggregates (ADR-014 §1-2): cloud→edge, versioned by config_version,
// replaced wholesale. This crate stores what sync gives it and never
// originates a row here — mirrors the menu_* pattern above.

pub fn upsert_station(conn: &Connection, s: &Station) -> DbResult<()> {
    conn.execute(
        "INSERT INTO station (id, outlet_id, code, name, sort_order, is_active, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
            outlet_id = excluded.outlet_id, code = excluded.code, name = excluded.name,
            sort_order = excluded.sort_order, is_active = excluded.is_active,
            config_version = excluded.config_version
         WHERE excluded.config_version >= station.config_version",
        params![
            s.id,
            s.outlet_id,
            s.code,
            s.name,
            s.sort_order,
            bool_to_i64(s.is_active),
            s.config_version
        ],
    )?;
    Ok(())
}

fn row_to_station(row: &rusqlite::Row) -> rusqlite::Result<Station> {
    Ok(Station {
        id: row.get(0)?,
        outlet_id: row.get(1)?,
        code: row.get(2)?,
        name: row.get(3)?,
        sort_order: row.get(4)?,
        is_active: i64_to_bool(row.get(5)?),
        config_version: row.get(6)?,
    })
}

const STATION_COLUMNS: &str = "id, outlet_id, code, name, sort_order, is_active, config_version";

pub fn list_stations_for_outlet(conn: &Connection, outlet_id: &str) -> DbResult<Vec<Station>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {STATION_COLUMNS} FROM station WHERE outlet_id = ?1 ORDER BY sort_order, name"
    ))?;
    let rows = stmt
        .query_map(params![outlet_id], row_to_station)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_station(conn: &Connection, id: &str) -> DbResult<Option<Station>> {
    conn.query_row(
        &format!("SELECT {STATION_COLUMNS} FROM station WHERE id = ?1"),
        params![id],
        row_to_station,
    )
    .optional()
    .map_err(Into::into)
}

/// Replaces an item's station routing wholesale (PUT semantics — ADR-014
/// §2): deletes every existing `menu_item_station` row for `menu_item_id`
/// and inserts `station_ids` in one transaction, so a station the item no
/// longer belongs to is guaranteed gone rather than merged. An empty
/// `station_ids` is legitimate (a non-production line, e.g. a service
/// charge, produces no ticket).
pub fn replace_menu_item_stations(
    conn: &Connection,
    menu_item_id: &str,
    station_ids: &[String],
    config_version: i64,
) -> DbResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM menu_item_station WHERE menu_item_id = ?1",
        params![menu_item_id],
    )?;
    for station_id in station_ids {
        tx.execute(
            "INSERT INTO menu_item_station (menu_item_id, station_id, config_version)
             VALUES (?1, ?2, ?3)",
            params![menu_item_id, station_id, config_version],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// The stations one menu item routes to, active only, ordered so ticket
/// generation is deterministic. Used both by the routing resolver in
/// `crate::Db::send_order_to_kitchen_with_outbox` and by any caller that
/// just wants to display an item's routing.
pub(crate) fn list_stations_for_menu_item(
    tx: &Transaction,
    menu_item_id: &str,
) -> DbResult<Vec<Station>> {
    let mut stmt = tx.prepare(
        "SELECT s.id, s.outlet_id, s.code, s.name, s.sort_order, s.is_active, s.config_version
         FROM station s
         JOIN menu_item_station mis ON mis.station_id = s.id
         WHERE mis.menu_item_id = ?1 AND s.is_active = 1
         ORDER BY s.sort_order, s.code",
    )?;
    let rows = stmt
        .query_map(params![menu_item_id], row_to_station)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn upsert_printer(conn: &Connection, p: &Printer) -> DbResult<()> {
    conn.execute(
        "INSERT INTO printer
            (id, outlet_id, name, connection_kind, address, paper_width_mm, is_active, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
            outlet_id = excluded.outlet_id, name = excluded.name,
            connection_kind = excluded.connection_kind, address = excluded.address,
            paper_width_mm = excluded.paper_width_mm, is_active = excluded.is_active,
            config_version = excluded.config_version
         WHERE excluded.config_version >= printer.config_version",
        params![
            p.id,
            p.outlet_id,
            p.name,
            p.connection_kind,
            p.address,
            p.paper_width_mm,
            bool_to_i64(p.is_active),
            p.config_version
        ],
    )?;
    Ok(())
}

pub fn list_printers_for_outlet(conn: &Connection, outlet_id: &str) -> DbResult<Vec<Printer>> {
    let mut stmt = conn.prepare(
        "SELECT id, outlet_id, name, connection_kind, address, paper_width_mm, is_active, config_version
         FROM printer WHERE outlet_id = ?1 ORDER BY name",
    )?;
    let rows = stmt
        .query_map(params![outlet_id], |row| {
            Ok(Printer {
                id: row.get(0)?,
                outlet_id: row.get(1)?,
                name: row.get(2)?,
                connection_kind: row.get(3)?,
                address: row.get(4)?,
                paper_width_mm: row.get(5)?,
                is_active: i64_to_bool(row.get(6)?),
                config_version: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Replaces a station's printer routing wholesale (PUT semantics, same
/// rationale as [`replace_menu_item_stations`]).
pub fn replace_station_printers(
    conn: &Connection,
    station_id: &str,
    printer_ids: &[String],
    config_version: i64,
) -> DbResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM station_printer WHERE station_id = ?1",
        params![station_id],
    )?;
    for printer_id in printer_ids {
        tx.execute(
            "INSERT INTO station_printer (station_id, printer_id, config_version)
             VALUES (?1, ?2, ?3)",
            params![station_id, printer_id, config_version],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Replaces one printer's role set wholesale (PUT semantics, same rationale
/// as [`replace_station_printers`]) — `printer_role`, contracts 0.4.7.
/// Passing an empty slice removes every role, which is how a printer is
/// retired from both paths without deleting the device row.
pub fn replace_printer_roles(
    conn: &Connection,
    printer_id: &str,
    roles: &[String],
    config_version: i64,
) -> DbResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM printer_role WHERE printer_id = ?1",
        params![printer_id],
    )?;
    for role in roles {
        tx.execute(
            "INSERT INTO printer_role (printer_id, role, config_version)
             VALUES (?1, ?2, ?3)",
            params![printer_id, role, config_version],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Per-row upsert for one `printer_role`, keyed on its actual primary key
/// `(printer_id, role)` — M4 T4b's config-bundle apply path.
///
/// Deliberately NOT [`replace_printer_roles`]: that helper is DELETE-then-
/// INSERT scoped to one `printer_id`, correct for a caller that supplies a
/// printer's FULL current role set in one call (an admin "PUT this
/// printer's roles" action). The cloud sync bundle instead ships a
/// `since_version`-filtered DELTA of individual `printer_role` rows
/// (`backend/internal/kitchen/repository.go::PrinterRolesSince` filters on
/// `pr.config_version > sinceVersion`, per-row, not per-printer) — the same
/// shape `restaurant_table`/`menu_item` already sync in. Calling
/// `replace_printer_roles` against a delta would DELETE every role for a
/// printer that the delta happens to mention and re-insert only the rows
/// that changed, silently dropping any of that printer's older, unchanged
/// roles. A plain per-row upsert, config_version-gated like every other
/// delta-synced config row, is the correct shape here; role REMOVAL is not
/// representable by this delta model, which matches every other config
/// family this bundle already carries (removal is not signalled by absence
/// for any of them).
pub fn upsert_printer_role(conn: &Connection, r: &PrinterRole) -> DbResult<()> {
    conn.execute(
        "INSERT INTO printer_role (printer_id, role, config_version)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(printer_id, role) DO UPDATE SET
            config_version = excluded.config_version
         WHERE excluded.config_version >= printer_role.config_version",
        params![r.printer_id, r.role, r.config_version],
    )?;
    Ok(())
}

pub fn list_printer_roles(conn: &Connection, printer_id: &str) -> DbResult<Vec<PrinterRole>> {
    let mut stmt = conn.prepare(
        "SELECT printer_id, role, config_version FROM printer_role
         WHERE printer_id = ?1 ORDER BY role",
    )?;
    let rows = stmt
        .query_map(params![printer_id], |row| {
            Ok(PrinterRole {
                printer_id: row.get(0)?,
                role: row.get(1)?,
                config_version: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn list_printers_for_station(
    conn: &Connection,
    station_id: &str,
) -> DbResult<Vec<StationPrinter>> {
    let mut stmt = conn.prepare(
        "SELECT station_id, printer_id, config_version FROM station_printer WHERE station_id = ?1",
    )?;
    let rows = stmt
        .query_map(params![station_id], |row| {
            Ok(StationPrinter {
                station_id: row.get(0)?,
                printer_id: row.get(1)?,
                config_version: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------------------------------------- Milestone 2: KOT generation -----
// docs/spec/kitchen.md: never "print the entire order" — one KOT per
// station. ADR-014 §2-4.

const EVENT_TYPE_KOT_CREATED: &str = "KOTCreated";
const EVENT_TYPE_SENT_TO_KITCHEN: &str = "SentToKitchen";
const EVENT_TYPE_KOT_STATUS_CHANGED: &str = "KOTStatusChanged";
const EVENT_TYPE_ORDER_READY: &str = "OrderReady";

/// The order_item ids already on some (any-status) KOT for this order,
/// parsed out of each `kot.items_json` blob. An item is only ever ticketed
/// once across the order's lifetime — a later `send_order_to_kitchen`
/// call must skip it, producing a ticket only for the delta (ADR-014 /
/// docs/spec/kitchen.md's #132 -> #132-A history).
pub(crate) fn already_ticketed_order_item_ids(
    tx: &Transaction,
    order_id: &str,
) -> DbResult<std::collections::HashSet<String>> {
    let mut stmt = tx.prepare("SELECT items_json FROM kot WHERE order_id = ?1")?;
    let blobs: Vec<String> = stmt
        .query_map(params![order_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;

    let mut ids = std::collections::HashSet::new();
    for blob in blobs {
        let items: Vec<serde_json::Value> = serde_json::from_str(&blob).unwrap_or_default();
        for item in items {
            if let Some(id) = item.get("order_item_id").and_then(|v| v.as_str()) {
                ids.insert(id.to_string());
            }
        }
    }
    Ok(ids)
}

/// The station `code` of whichever earlier KOT for this order carries
/// `order_item_id` on its ticket, or `None` if the item was never
/// ticketed. An item is only ever ticketed once across the order's
/// lifetime (see [`already_ticketed_order_item_ids`]), so this returns at
/// most one station — used by
/// `crate::Db::cancel_kitchen_items_with_outbox` to route a cancellation
/// announcement to the same station(s) the original ticket went to.
pub(crate) fn find_ticketed_station_for_order_item(
    tx: &Transaction,
    order_id: &str,
    order_item_id: &str,
) -> DbResult<Option<String>> {
    let mut stmt = tx.prepare("SELECT station, items_json FROM kot WHERE order_id = ?1")?;
    let rows: Vec<(String, String)> = stmt
        .query_map(params![order_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;

    for (station, blob) in rows {
        let items: Vec<serde_json::Value> = serde_json::from_str(&blob).unwrap_or_default();
        let found = items
            .iter()
            .any(|i| i.get("order_item_id").and_then(|v| v.as_str()) == Some(order_item_id));
        if found {
            return Ok(Some(station));
        }
    }
    Ok(None)
}

pub(crate) fn next_kot_sequence(tx: &Transaction, order_id: &str) -> DbResult<i64> {
    let max: Option<i64> = tx.query_row(
        "SELECT MAX(sequence) FROM kot WHERE order_id = ?1",
        params![order_id],
        |row| row.get(0),
    )?;
    Ok(max.unwrap_or(0) + 1)
}

/// Enforces "send-to-kitchen is only legal once an order has been
/// confirmed, and before it has reached a terminal state" inside the same
/// transaction as the write it guards. Returns `outlet_id` on success.
pub(crate) fn require_sendable_order(tx: &Transaction, order_id: &str) -> DbResult<String> {
    let row: Option<(String, String)> = tx
        .query_row(
            "SELECT outlet_id, status FROM \"order\" WHERE id = ?1",
            params![order_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match row {
        None => Err(crate::error::DbError::NotFound("order")),
        Some((outlet_id, status))
            if matches!(
                status.as_str(),
                "CONFIRMED" | "SENT_TO_KITCHEN" | "PREPARING"
            ) =>
        {
            Ok(outlet_id)
        }
        Some((_, status)) => Err(crate::error::DbError::OrderNotSendableToKitchen {
            order_id: order_id.to_string(),
            status,
        }),
    }
}

/// `menu_item.name` + the order line's quantity/notes + its modifier option
/// names, matching `KotTicketItemSchema` (`packages/contracts/src/types/kot.ts`).
pub(crate) fn build_kot_ticket_item(tx: &Transaction, item: &OrderItem) -> DbResult<KotTicketItem> {
    let name: String = tx.query_row(
        "SELECT name FROM menu_item WHERE id = ?1",
        params![item.menu_item_id],
        |row| row.get(0),
    )?;
    let modifiers = list_order_item_modifiers_in_tx(tx, &item.id)?
        .into_iter()
        .map(|m| m.option_name)
        .collect();
    Ok(KotTicketItem {
        order_item_id: item.id.clone(),
        name,
        quantity: item.quantity,
        modifiers,
        notes: item.notes.clone(),
    })
}

pub(crate) fn kot_ticket_items_json(items: &[KotTicketItem]) -> String {
    let values: Vec<serde_json::Value> = items
        .iter()
        .map(|i| {
            serde_json::json!({
                "order_item_id": i.order_item_id,
                "name": i.name,
                "quantity": i.quantity,
                "modifiers": i.modifiers,
                "notes": i.notes,
            })
        })
        .collect();
    serde_json::Value::Array(values).to_string()
}

pub(crate) fn insert_kot_in_tx(tx: &Transaction, k: &Kot) -> DbResult<()> {
    tx.execute(
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

fn build_kot_created_payload(
    outlet_id: &str,
    kot: &Kot,
    items: &[KotTicketItem],
    event_id: &str,
    occurred_at: &str,
) -> String {
    let items_value: Vec<serde_json::Value> = items
        .iter()
        .map(|i| {
            serde_json::json!({
                "order_item_id": i.order_item_id,
                "name": i.name,
                "quantity": i.quantity,
                "modifiers": i.modifiers,
                "notes": i.notes,
            })
        })
        .collect();
    serde_json::json!({
        "event_id": event_id,
        "event_type": EVENT_TYPE_KOT_CREATED,
        "occurred_at": occurred_at,
        "outlet_id": outlet_id,
        "schema_version": 1,
        "data": {
            "kot": {
                "id": kot.id,
                "order_id": kot.order_id,
                "station": kot.station,
                "sequence": kot.sequence,
                "status": kot.status,
                "items": items_value,
                "created_by_device_id": kot.created_by_device_id,
                "created_at": kot.created_at,
                "updated_at": kot.updated_at,
                "schema_version": 1,
            }
        }
    })
    .to_string()
}

pub(crate) fn insert_kot_created_outbox(
    tx: &Transaction,
    outlet_id: &str,
    kot: &Kot,
    items: &[KotTicketItem],
    outbox_id: &str,
    occurred_at: &str,
) -> DbResult<()> {
    let payload = build_kot_created_payload(outlet_id, kot, items, outbox_id, occurred_at);
    insert_outbox_entry(
        tx,
        &NewOutboxEntry {
            id: outbox_id.to_string(),
            aggregate_type: "kot".to_string(),
            aggregate_id: kot.id.clone(),
            event_type: EVENT_TYPE_KOT_CREATED.to_string(),
            payload_json: payload,
            created_at: occurred_at.to_string(),
        },
    )
}

pub(crate) fn insert_sent_to_kitchen_outbox(
    tx: &Transaction,
    outlet_id: &str,
    order_id: &str,
    outbox_id: &str,
    occurred_at: &str,
) -> DbResult<()> {
    let payload = serde_json::json!({
        "event_id": outbox_id,
        "event_type": EVENT_TYPE_SENT_TO_KITCHEN,
        "occurred_at": occurred_at,
        "outlet_id": outlet_id,
        "schema_version": 1,
        "data": { "order_id": order_id }
    })
    .to_string();
    insert_outbox_entry(
        tx,
        &NewOutboxEntry {
            id: outbox_id.to_string(),
            aggregate_type: "order".to_string(),
            aggregate_id: order_id.to_string(),
            event_type: EVENT_TYPE_SENT_TO_KITCHEN.to_string(),
            payload_json: payload,
            created_at: occurred_at.to_string(),
        },
    )
}

/// Stamps `status = 'SENT_TO_KITCHEN'` unless the order has already moved
/// further along (e.g. a second send-to-kitchen call for an addition, once
/// the order is already PREPARING) — never regresses status backwards.
pub(crate) fn stamp_order_sent_to_kitchen_if_earlier(
    tx: &Transaction,
    order_id: &str,
    updated_at: &str,
) -> DbResult<()> {
    tx.execute(
        "UPDATE \"order\" SET status = 'SENT_TO_KITCHEN', version = version + 1, updated_at = ?1
         WHERE id = ?2 AND status = 'CONFIRMED'",
        params![updated_at, order_id],
    )?;
    Ok(())
}

// ------------------------------------------ Milestone 2: KOT status trail --

const LEGAL_KOT_TRANSITIONS: &[(&str, &[&str])] = &[
    ("NEW", &["ACKNOWLEDGED", "CANCELLED"]),
    ("ACKNOWLEDGED", &["PREPARING", "CANCELLED"]),
    ("PREPARING", &["READY", "CANCELLED"]),
    ("READY", &["SERVED"]),
];

pub(crate) fn is_legal_kot_transition(from: &str, to: &str) -> bool {
    LEGAL_KOT_TRANSITIONS
        .iter()
        .find(|(f, _)| *f == from)
        .is_some_and(|(_, tos)| tos.contains(&to))
}

/// Reads a KOT's current `order_id`/`status` inside the transaction that is
/// about to transition it, so the legality check and the mutation cannot
/// race. Returns `DbError::NotFound` if the KOT does not exist.
pub(crate) fn get_kot_status_for_transition(
    tx: &Transaction,
    kot_id: &str,
) -> DbResult<(String, String)> {
    tx.query_row(
        "SELECT order_id, status FROM kot WHERE id = ?1",
        params![kot_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()?
    .ok_or(crate::error::DbError::NotFound("kot"))
}

pub(crate) fn stamp_kot_status(
    tx: &Transaction,
    kot_id: &str,
    status: &str,
    updated_at: &str,
) -> DbResult<()> {
    let changed = tx.execute(
        "UPDATE kot SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![status, updated_at, kot_id],
    )?;
    if changed == 0 {
        return Err(crate::error::DbError::NotFound("kot"));
    }
    Ok(())
}

pub(crate) fn insert_kot_status_history(
    tx: &Transaction,
    entry: &KotStatusHistoryEntry,
) -> DbResult<()> {
    tx.execute(
        "INSERT INTO kot_status_history (id, kot_id, status, changed_by_device_id, changed_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            entry.id,
            entry.kot_id,
            entry.status,
            entry.changed_by_device_id,
            entry.changed_at
        ],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_kot_status_changed_outbox(
    tx: &Transaction,
    outlet_id: &str,
    kot_id: &str,
    order_id: &str,
    status: &str,
    changed_by_device_id: &str,
    outbox_id: &str,
    occurred_at: &str,
) -> DbResult<()> {
    let payload = serde_json::json!({
        "event_id": outbox_id,
        "event_type": EVENT_TYPE_KOT_STATUS_CHANGED,
        "occurred_at": occurred_at,
        "outlet_id": outlet_id,
        "schema_version": 1,
        "data": {
            "kot_id": kot_id,
            "order_id": order_id,
            "status": status,
            "changed_at": occurred_at,
            "changed_by_device_id": changed_by_device_id,
        }
    })
    .to_string();
    insert_outbox_entry(
        tx,
        &NewOutboxEntry {
            id: outbox_id.to_string(),
            aggregate_type: "kot".to_string(),
            aggregate_id: kot_id.to_string(),
            event_type: EVENT_TYPE_KOT_STATUS_CHANGED.to_string(),
            payload_json: payload,
            created_at: occurred_at.to_string(),
        },
    )
}

/// True when every non-cancelled KOT on the order is READY and there is at
/// least one such KOT — the order-status derivation from
/// docs/spec/kitchen.md ("An order becomes READY when all its non-cancelled
/// KOTs are READY").
pub(crate) fn order_is_kitchen_ready(tx: &Transaction, order_id: &str) -> DbResult<bool> {
    let (total_active, not_ready): (i64, i64) = tx.query_row(
        "SELECT
            COUNT(*) FILTER (WHERE status != 'CANCELLED'),
            COUNT(*) FILTER (WHERE status NOT IN ('CANCELLED','READY'))
         FROM kot WHERE order_id = ?1",
        params![order_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(total_active > 0 && not_ready == 0)
}

/// Stamps the order READY and returns `true` if it did — a caller-visible
/// signal for whether to also emit `OrderReady` — but only when the order
/// has not already moved past READY (SERVED/BILLED/PAID/CLOSED/CANCELLED),
/// so this derivation never regresses a further-along order backwards.
pub(crate) fn stamp_order_ready_if_applicable(
    tx: &Transaction,
    order_id: &str,
    updated_at: &str,
) -> DbResult<bool> {
    let changed = tx.execute(
        "UPDATE \"order\" SET status = 'READY', version = version + 1, updated_at = ?1
         WHERE id = ?2 AND status NOT IN ('READY','SERVED','BILLED','PAID','CLOSED','CANCELLED')",
        params![updated_at, order_id],
    )?;
    Ok(changed > 0)
}

pub(crate) fn insert_order_ready_outbox(
    tx: &Transaction,
    outlet_id: &str,
    order_id: &str,
    outbox_id: &str,
    occurred_at: &str,
) -> DbResult<()> {
    let payload = serde_json::json!({
        "event_id": outbox_id,
        "event_type": EVENT_TYPE_ORDER_READY,
        "occurred_at": occurred_at,
        "outlet_id": outlet_id,
        "schema_version": 1,
        "data": { "order_id": order_id }
    })
    .to_string();
    insert_outbox_entry(
        tx,
        &NewOutboxEntry {
            id: outbox_id.to_string(),
            aggregate_type: "order".to_string(),
            aggregate_id: order_id.to_string(),
            event_type: EVENT_TYPE_ORDER_READY.to_string(),
            payload_json: payload,
            created_at: occurred_at.to_string(),
        },
    )
}

pub(crate) fn get_order_outlet_id(tx: &Transaction, order_id: &str) -> DbResult<String> {
    tx.query_row(
        "SELECT outlet_id FROM \"order\" WHERE id = ?1",
        params![order_id],
        |row| row.get(0),
    )
    .optional()?
    .ok_or(crate::error::DbError::NotFound("order"))
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

/// The still-unpublished `local_outbox` payload for one `(aggregate_id,
/// event_type)` pair, if any — a targeted lookup for callers (the Tauri
/// order-shape command) that need to correct a specific queued event's
/// payload rather than scan the whole pending set. `None` both when no such
/// row exists and when it exists but has already published; the caller
/// cannot tell those apart from this alone, which is intentional — either
/// way there is nothing left for it to correct locally.
pub fn get_unpublished_outbox_payload(
    conn: &Connection,
    aggregate_id: &str,
    event_type: &str,
) -> DbResult<Option<String>> {
    conn.query_row(
        "SELECT payload_json FROM local_outbox
         WHERE aggregate_id = ?1 AND event_type = ?2 AND published_at IS NULL",
        params![aggregate_id, event_type],
        |row| row.get(0),
    )
    .optional()
    .map_err(crate::error::DbError::from)
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

// ----------------------------------------------- device_credential_cache --
// Config aggregate: cloud owns it, replaced wholesale per config_version
// (ADR-011 pattern extended to devices, ADR-017 amendment 0.4.3). Never
// returned over any wire API by this crate; `credential_hash` gets the same
// containment as `app_user.password_hash`.

pub fn replace_device_credential_cache(
    conn: &Connection,
    c: &DeviceCredentialCache,
) -> DbResult<()> {
    conn.execute(
        "INSERT INTO device_credential_cache
            (credential_id, device_id, tenant_id, outlet_id, credential_hash,
             device_kind, revoked_at, expires_at, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(credential_id) DO UPDATE SET
            device_id = excluded.device_id,
            tenant_id = excluded.tenant_id,
            outlet_id = excluded.outlet_id,
            credential_hash = excluded.credential_hash,
            device_kind = excluded.device_kind,
            revoked_at = excluded.revoked_at,
            expires_at = excluded.expires_at,
            config_version = excluded.config_version
         WHERE excluded.config_version >= device_credential_cache.config_version",
        params![
            c.credential_id,
            c.device_id,
            c.tenant_id,
            c.outlet_id,
            c.credential_hash,
            c.device_kind,
            c.revoked_at,
            c.expires_at,
            c.config_version,
        ],
    )?;
    Ok(())
}

fn row_to_device_credential_cache(row: &rusqlite::Row) -> rusqlite::Result<DeviceCredentialCache> {
    Ok(DeviceCredentialCache {
        credential_id: row.get(0)?,
        device_id: row.get(1)?,
        tenant_id: row.get(2)?,
        outlet_id: row.get(3)?,
        credential_hash: row.get(4)?,
        device_kind: row.get(5)?,
        revoked_at: row.get(6)?,
        expires_at: row.get(7)?,
        config_version: row.get(8)?,
    })
}

const DEVICE_CREDENTIAL_CACHE_COLUMNS: &str =
    "credential_id, device_id, tenant_id, outlet_id, credential_hash, \
     device_kind, revoked_at, expires_at, config_version";

/// Looks up a cached credential by its `credential_id` (the first component
/// of the `<credential_id>.<secret>` device token). Returns `Ok(None)` when
/// the row is not (yet) cached — this is deliberately NOT an error: while
/// offline, "not cached" is indistinguishable from "not yet synced", and the
/// caller (`holler_edge_device::auth`) must treat that as "unknown", never
/// as "revoked". Only `revoked_at`/`expires_at` on a row that DOES exist may
/// reject a presented token — see the schema's own column comments.
pub fn get_device_credential_cache_by_id(
    conn: &Connection,
    credential_id: &str,
) -> DbResult<Option<DeviceCredentialCache>> {
    conn.query_row(
        &format!(
            "SELECT {DEVICE_CREDENTIAL_CACHE_COLUMNS} FROM device_credential_cache \
             WHERE credential_id = ?1"
        ),
        params![credential_id],
        row_to_device_credential_cache,
    )
    .optional()
    .map_err(Into::into)
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

#[cfg(test)]
mod device_credential_cache_tests {
    use super::*;
    use crate::Db;

    fn seed_outlet(conn: &Connection) {
        upsert_outlet(
            conn,
            &Outlet {
                id: "outlet-1".to_string(),
                brand_id: "brand-1".to_string(),
                name: "Test Outlet".to_string(),
                timezone: "Asia/Kolkata".to_string(),
                config_version: 1,
                created_at: "2026-08-13T00:00:00Z".to_string(),
                updated_at: "2026-08-13T00:00:00Z".to_string(),
            },
        )
        .expect("seed outlet");
    }

    fn sample(config_version: i64) -> DeviceCredentialCache {
        DeviceCredentialCache {
            credential_id: "cred-1".to_string(),
            device_id: "device-1".to_string(),
            tenant_id: "tenant-1".to_string(),
            outlet_id: "outlet-1".to_string(),
            credential_hash: "argon2id$fake-verifier".to_string(),
            device_kind: "KDS".to_string(),
            revoked_at: None,
            expires_at: None,
            config_version,
        }
    }

    /// Round trip: an inserted credential is readable by `credential_id`,
    /// and a row that has never synced reads back as `None` — never as an
    /// error — because "not cached" must be distinguishable from "revoked"
    /// at the call site (ADR-017 amendment).
    #[test]
    fn missing_credential_is_none_not_an_error() {
        let db = Db::open_in_memory_for_tests().expect("open db");
        let got = get_device_credential_cache_by_id(db.connection(), "does-not-exist")
            .expect("lookup must not error on a missing row");
        assert!(got.is_none());
    }

    #[test]
    fn insert_then_read_back_round_trips_all_fields() {
        let db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection());
        replace_device_credential_cache(db.connection(), &sample(1)).expect("insert");

        let got = get_device_credential_cache_by_id(db.connection(), "cred-1")
            .expect("lookup")
            .expect("row must exist");
        assert_eq!(got.device_id, "device-1");
        assert_eq!(got.tenant_id, "tenant-1");
        assert_eq!(got.outlet_id, "outlet-1");
        assert_eq!(got.credential_hash, "argon2id$fake-verifier");
        assert_eq!(got.device_kind, "KDS");
        assert!(got.revoked_at.is_none());
        assert!(got.expires_at.is_none());
        assert_eq!(got.config_version, 1);
    }

    /// A revoked/expired row must still be present and readable — the whole
    /// point of syncing it at all (ADR-017 amendment: rejection decided by
    /// these fields, never by absence).
    #[test]
    fn revoked_and_expired_rows_still_read_back() {
        let db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection());
        let mut c = sample(1);
        c.revoked_at = Some("2026-08-13T00:00:00Z".to_string());
        c.expires_at = Some("2026-08-13T00:00:00Z".to_string());
        replace_device_credential_cache(db.connection(), &c).expect("insert");

        let got = get_device_credential_cache_by_id(db.connection(), "cred-1")
            .expect("lookup")
            .expect("a revoked/expired row must still be stored, not deleted");
        assert!(got.revoked_at.is_some());
        assert!(got.expires_at.is_some());
    }

    /// Mirrors `replace_app_user`'s config_version guard: an older or equal
    /// bundle must never regress an already-newer cached credential (this is
    /// what makes a revocation, which bumps config_version, stick rather
    /// than being overwritten by a stale replay).
    #[test]
    fn stale_config_version_does_not_overwrite_newer_row() {
        let db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection());
        replace_device_credential_cache(db.connection(), &sample(5)).expect("insert v5");

        let mut stale = sample(3);
        stale.credential_hash = "argon2id$should-not-apply".to_string();
        replace_device_credential_cache(db.connection(), &stale)
            .expect("stale write must not error");

        let got = get_device_credential_cache_by_id(db.connection(), "cred-1")
            .expect("lookup")
            .expect("row must exist");
        assert_eq!(
            got.config_version, 5,
            "newer row must survive a stale replay"
        );
        assert_eq!(got.credential_hash, "argon2id$fake-verifier");
    }
}

// -------------------------------------- Milestone 3: billing config (T7a) --
// compliance_version / tax_profile / tax_rule / outlet_fiscal_profile /
// invoice_series / discount_definition are CONFIG aggregates (ADR-016 §1):
// cloud→edge, versioned by config_version, replaced wholesale — same
// upsert-with-guard pattern as `upsert_station`/`upsert_printer` above. A
// stale bundle (config_version older than or equal to what is already
// stored) must never regress a newer row — the `WHERE excluded.config_version
// >= <table>.config_version` clause on every statement below is that guard.

pub fn upsert_compliance_version(conn: &Connection, v: &ComplianceVersion) -> DbResult<()> {
    conn.execute(
        "INSERT INTO compliance_version (id, outlet_id, label, effective_from, notes, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
            outlet_id = excluded.outlet_id, label = excluded.label,
            effective_from = excluded.effective_from, notes = excluded.notes,
            config_version = excluded.config_version
         WHERE excluded.config_version >= compliance_version.config_version",
        params![v.id, v.outlet_id, v.label, v.effective_from, v.notes, v.config_version],
    )?;
    Ok(())
}

fn row_to_compliance_version(row: &rusqlite::Row) -> rusqlite::Result<ComplianceVersion> {
    Ok(ComplianceVersion {
        id: row.get(0)?,
        outlet_id: row.get(1)?,
        label: row.get(2)?,
        effective_from: row.get(3)?,
        notes: row.get(4)?,
        config_version: row.get(5)?,
    })
}

const COMPLIANCE_VERSION_COLUMNS: &str =
    "id, outlet_id, label, effective_from, notes, config_version";

pub fn list_compliance_versions_for_outlet(
    conn: &Connection,
    outlet_id: &str,
) -> DbResult<Vec<ComplianceVersion>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COMPLIANCE_VERSION_COLUMNS} FROM compliance_version WHERE outlet_id = ?1 ORDER BY effective_from"
    ))?;
    let rows = stmt
        .query_map(params![outlet_id], row_to_compliance_version)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn upsert_tax_profile(conn: &Connection, p: &TaxProfile) -> DbResult<()> {
    conn.execute(
        "INSERT INTO tax_profile (id, outlet_id, code, name, pricing_mode, is_default, is_active, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
            outlet_id = excluded.outlet_id, code = excluded.code, name = excluded.name,
            pricing_mode = excluded.pricing_mode, is_default = excluded.is_default,
            is_active = excluded.is_active, config_version = excluded.config_version
         WHERE excluded.config_version >= tax_profile.config_version",
        params![
            p.id,
            p.outlet_id,
            p.code,
            p.name,
            p.pricing_mode,
            bool_to_i64(p.is_default),
            bool_to_i64(p.is_active),
            p.config_version
        ],
    )?;
    Ok(())
}

fn row_to_tax_profile(row: &rusqlite::Row) -> rusqlite::Result<TaxProfile> {
    Ok(TaxProfile {
        id: row.get(0)?,
        outlet_id: row.get(1)?,
        code: row.get(2)?,
        name: row.get(3)?,
        pricing_mode: row.get(4)?,
        is_default: i64_to_bool(row.get(5)?),
        is_active: i64_to_bool(row.get(6)?),
        config_version: row.get(7)?,
    })
}

const TAX_PROFILE_COLUMNS: &str =
    "id, outlet_id, code, name, pricing_mode, is_default, is_active, config_version";

pub fn list_tax_profiles_for_outlet(
    conn: &Connection,
    outlet_id: &str,
) -> DbResult<Vec<TaxProfile>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {TAX_PROFILE_COLUMNS} FROM tax_profile WHERE outlet_id = ?1 ORDER BY code"
    ))?;
    let rows = stmt
        .query_map(params![outlet_id], row_to_tax_profile)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn upsert_tax_rule(conn: &Connection, r: &TaxRule) -> DbResult<()> {
    conn.execute(
        "INSERT INTO tax_rule
            (id, tax_profile_id, compliance_version_id, component, rate_bps, effective_from, effective_to, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
            tax_profile_id = excluded.tax_profile_id, compliance_version_id = excluded.compliance_version_id,
            component = excluded.component, rate_bps = excluded.rate_bps,
            effective_from = excluded.effective_from, effective_to = excluded.effective_to,
            config_version = excluded.config_version
         WHERE excluded.config_version >= tax_rule.config_version",
        params![
            r.id,
            r.tax_profile_id,
            r.compliance_version_id,
            r.component,
            r.rate_bps,
            r.effective_from,
            r.effective_to,
            r.config_version
        ],
    )?;
    Ok(())
}

fn row_to_tax_rule(row: &rusqlite::Row) -> rusqlite::Result<TaxRule> {
    Ok(TaxRule {
        id: row.get(0)?,
        tax_profile_id: row.get(1)?,
        compliance_version_id: row.get(2)?,
        component: row.get(3)?,
        rate_bps: row.get(4)?,
        effective_from: row.get(5)?,
        effective_to: row.get(6)?,
        config_version: row.get(7)?,
    })
}

const TAX_RULE_COLUMNS: &str =
    "id, tax_profile_id, compliance_version_id, component, rate_bps, effective_from, effective_to, config_version";

/// Every rule for one profile, across every compliance version — the shape
/// `tax::resolve_rates` filters down by `(profile_id, compliance_version_id,
/// at)`. Not outlet-scoped in SQL because `tax_rule` carries no `outlet_id`
/// of its own (it hangs off `tax_profile_id`, the `menu_item_variant`
/// precedent) — callers already have the profile id from
/// `tax::resolve_tax_profile`.
pub fn list_tax_rules_for_profile(
    conn: &Connection,
    tax_profile_id: &str,
) -> DbResult<Vec<TaxRule>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {TAX_RULE_COLUMNS} FROM tax_rule WHERE tax_profile_id = ?1 ORDER BY effective_from"
    ))?;
    let rows = stmt
        .query_map(params![tax_profile_id], row_to_tax_rule)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn upsert_outlet_fiscal_profile(conn: &Connection, f: &OutletFiscalProfile) -> DbResult<()> {
    conn.execute(
        "INSERT INTO outlet_fiscal_profile
            (id, outlet_id, legal_name, trade_name, address_line1, address_line2, city,
             state_code, state_name, pincode, gstin, fssai_number, invoice_footer_text,
             effective_from, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(id) DO UPDATE SET
            outlet_id = excluded.outlet_id, legal_name = excluded.legal_name,
            trade_name = excluded.trade_name, address_line1 = excluded.address_line1,
            address_line2 = excluded.address_line2, city = excluded.city,
            state_code = excluded.state_code, state_name = excluded.state_name,
            pincode = excluded.pincode, gstin = excluded.gstin,
            fssai_number = excluded.fssai_number, invoice_footer_text = excluded.invoice_footer_text,
            effective_from = excluded.effective_from, config_version = excluded.config_version
         WHERE excluded.config_version >= outlet_fiscal_profile.config_version",
        params![
            f.id,
            f.outlet_id,
            f.legal_name,
            f.trade_name,
            f.address_line1,
            f.address_line2,
            f.city,
            f.state_code,
            f.state_name,
            f.pincode,
            f.gstin,
            f.fssai_number,
            f.invoice_footer_text,
            f.effective_from,
            f.config_version
        ],
    )?;
    Ok(())
}

fn row_to_outlet_fiscal_profile(row: &rusqlite::Row) -> rusqlite::Result<OutletFiscalProfile> {
    Ok(OutletFiscalProfile {
        id: row.get(0)?,
        outlet_id: row.get(1)?,
        legal_name: row.get(2)?,
        trade_name: row.get(3)?,
        address_line1: row.get(4)?,
        address_line2: row.get(5)?,
        city: row.get(6)?,
        state_code: row.get(7)?,
        state_name: row.get(8)?,
        pincode: row.get(9)?,
        gstin: row.get(10)?,
        fssai_number: row.get(11)?,
        invoice_footer_text: row.get(12)?,
        effective_from: row.get(13)?,
        config_version: row.get(14)?,
    })
}

const OUTLET_FISCAL_PROFILE_COLUMNS: &str = "id, outlet_id, legal_name, trade_name, address_line1, \
    address_line2, city, state_code, state_name, pincode, gstin, fssai_number, invoice_footer_text, \
    effective_from, config_version";

pub fn list_outlet_fiscal_profiles_for_outlet(
    conn: &Connection,
    outlet_id: &str,
) -> DbResult<Vec<OutletFiscalProfile>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {OUTLET_FISCAL_PROFILE_COLUMNS} FROM outlet_fiscal_profile WHERE outlet_id = ?1 ORDER BY effective_from"
    ))?;
    let rows = stmt
        .query_map(params![outlet_id], row_to_outlet_fiscal_profile)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn upsert_invoice_series(conn: &Connection, s: &InvoiceSeries) -> DbResult<()> {
    conn.execute(
        "INSERT INTO invoice_series
            (id, outlet_id, code, prefix_template, reset_policy, padding_width, is_active, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
            outlet_id = excluded.outlet_id, code = excluded.code,
            prefix_template = excluded.prefix_template, reset_policy = excluded.reset_policy,
            padding_width = excluded.padding_width, is_active = excluded.is_active,
            config_version = excluded.config_version
         WHERE excluded.config_version >= invoice_series.config_version",
        params![
            s.id,
            s.outlet_id,
            s.code,
            s.prefix_template,
            s.reset_policy,
            s.padding_width,
            bool_to_i64(s.is_active),
            s.config_version
        ],
    )?;
    Ok(())
}

fn row_to_invoice_series(row: &rusqlite::Row) -> rusqlite::Result<InvoiceSeries> {
    Ok(InvoiceSeries {
        id: row.get(0)?,
        outlet_id: row.get(1)?,
        code: row.get(2)?,
        prefix_template: row.get(3)?,
        reset_policy: row.get(4)?,
        padding_width: row.get(5)?,
        is_active: i64_to_bool(row.get(6)?),
        config_version: row.get(7)?,
    })
}

const INVOICE_SERIES_COLUMNS: &str =
    "id, outlet_id, code, prefix_template, reset_policy, padding_width, is_active, config_version";

pub fn list_invoice_series_for_outlet(
    conn: &Connection,
    outlet_id: &str,
) -> DbResult<Vec<InvoiceSeries>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {INVOICE_SERIES_COLUMNS} FROM invoice_series WHERE outlet_id = ?1 ORDER BY code"
    ))?;
    let rows = stmt
        .query_map(params![outlet_id], row_to_invoice_series)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn upsert_discount_definition(conn: &Connection, d: &DiscountDefinition) -> DbResult<()> {
    conn.execute(
        "INSERT INTO discount_definition
            (id, outlet_id, code, name, scope, method, value_bps, value_paise, max_discount_paise,
             required_permission, requires_reason, is_active, effective_from, effective_to, config_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(id) DO UPDATE SET
            outlet_id = excluded.outlet_id, code = excluded.code, name = excluded.name,
            scope = excluded.scope, method = excluded.method, value_bps = excluded.value_bps,
            value_paise = excluded.value_paise, max_discount_paise = excluded.max_discount_paise,
            required_permission = excluded.required_permission, requires_reason = excluded.requires_reason,
            is_active = excluded.is_active, effective_from = excluded.effective_from,
            effective_to = excluded.effective_to, config_version = excluded.config_version
         WHERE excluded.config_version >= discount_definition.config_version",
        params![
            d.id,
            d.outlet_id,
            d.code,
            d.name,
            d.scope,
            d.method,
            d.value_bps,
            d.value_paise,
            d.max_discount_paise,
            d.required_permission,
            bool_to_i64(d.requires_reason),
            bool_to_i64(d.is_active),
            d.effective_from,
            d.effective_to,
            d.config_version
        ],
    )?;
    Ok(())
}

fn row_to_discount_definition(row: &rusqlite::Row) -> rusqlite::Result<DiscountDefinition> {
    Ok(DiscountDefinition {
        id: row.get(0)?,
        outlet_id: row.get(1)?,
        code: row.get(2)?,
        name: row.get(3)?,
        scope: row.get(4)?,
        method: row.get(5)?,
        value_bps: row.get(6)?,
        value_paise: row.get(7)?,
        max_discount_paise: row.get(8)?,
        required_permission: row.get(9)?,
        requires_reason: i64_to_bool(row.get(10)?),
        is_active: i64_to_bool(row.get(11)?),
        effective_from: row.get(12)?,
        effective_to: row.get(13)?,
        config_version: row.get(14)?,
    })
}

const DISCOUNT_DEFINITION_COLUMNS: &str = "id, outlet_id, code, name, scope, method, value_bps, \
    value_paise, max_discount_paise, required_permission, requires_reason, is_active, \
    effective_from, effective_to, config_version";

pub fn list_discount_definitions_for_outlet(
    conn: &Connection,
    outlet_id: &str,
) -> DbResult<Vec<DiscountDefinition>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {DISCOUNT_DEFINITION_COLUMNS} FROM discount_definition WHERE outlet_id = ?1 ORDER BY code"
    ))?;
    let rows = stmt
        .query_map(params![outlet_id], row_to_discount_definition)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------------------------------------------- Milestone 3: invoicing (T7b) --
// invoice / invoice_line are EDGE-AUTHORITATIVE (ADR-016 §1). invoice_sequence
// is EDGE-LOCAL: SQLite only, never mirrored, never given an AggregateType or
// a sync direction (§2) — see [`next_invoice_sequence_value`]'s doc comment
// for the atomicity argument that makes it §33-safe.

/// Atomically advances the counter for `(series_id, period_key)` and
/// returns the NEW value, inside the SAME transaction as the `invoice`
/// insert that will use it (ADR-016 §2/§33, task instructions "the counter
/// increment and the invoice insert must be one atomic step").
///
/// **Why a crash cannot duplicate or burn a number.** `INSERT ... ON
/// CONFLICT ... RETURNING` is one SQLite statement; SQLite either applies it
/// in full or not at all, and it never leaves the connection mid-statement.
/// This call always runs as one statement inside the caller's transaction,
/// alongside the `invoice`/`invoice_line` inserts that use the value it
/// returns:
///   - A crash BEFORE this statement executes: nothing changed. The next
///     `Db::open` sees the pre-crash counter and the caller's whole
///     transaction (which never reached `COMMIT`) is gone — the invoice was
///     never issued, so no number was owed.
///   - A crash AFTER this statement but BEFORE the enclosing transaction's
///     `COMMIT`: SQLite's rollback journal/WAL discards the ENTIRE
///     transaction on the next open, including this statement's effect —
///     `invoice_sequence.last_value` reverts along with the invoice/lines
///     that would have used it. The number is neither burned (nothing
///     observed it — the invoice row it would have numbered does not
///     exist either) nor reused for a different invoice (a fresh call to
///     this function after recovery reissues the SAME next value, for a
///     fresh issuance attempt).
///   - A crash AFTER `COMMIT` returns: both the counter bump and the
///     invoice are durable together (ADR-013: WAL mode, single writer, one
///     file) — there is nothing left to retry.
///
/// There is no window in which the counter advances durably without the
/// invoice it produced also becoming durable, or vice versa — which is
/// exactly "one atomic step". See
/// `tests::invoice_survives_a_crash_mid_session_and_reads_back_on_independent_reopen`
/// in `src/lib.rs` for the test that drops the whole `Db` mid-issue and
/// reopens an independent handle on the sealed file to prove this (it lives
/// there, not under `tests/`, because it needs the `#[cfg(test)]`-only
/// `Db::simulate_crash_for_tests` seam, which is intentionally not part of
/// this crate's public API).
pub(crate) fn next_invoice_sequence_value(
    tx: &Transaction,
    series_id: &str,
    period_key: &str,
    updated_at: &str,
) -> DbResult<i64> {
    tx.query_row(
        "INSERT INTO invoice_sequence (series_id, period_key, last_value, updated_at)
         VALUES (?1, ?2, 1, ?3)
         ON CONFLICT(series_id, period_key) DO UPDATE SET
            last_value = invoice_sequence.last_value + 1,
            updated_at = excluded.updated_at
         RETURNING last_value",
        params![series_id, period_key, updated_at],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

// ------------------------------------------------ Milestone 4: inventory (T2) --
// stock_ledger_sequence is EDGE-LOCAL (ADR-018 0.5.3 addendum,
// sqlite/0021_stock_ledger_sequence.sql) — the `invoice_sequence` shape minus
// the period bucket: a durable, per-outlet, monotonic-forever counter, never
// mirrored, never given an AggregateType or sync direction.
//
// WHY THIS REPLACED MAX(entry_seq) + 1. That derivation was correct only
// because `stock_ledger_entry` carries a no-delete trigger, and ADR-018 §9's
// retention design requires removing exactly that trigger once archival
// ships. A derived counter that restarts after rows are removed is the same
// defect class as 0.5.1's rescaled sub-recipe and 0.5.2's reclassified
// ingredient: a stored value silently meaning something different after an
// unrelated operation. This table is the fix, one operation earlier.

/// Atomically advances the durable per-outlet `entry_seq` counter and
/// returns the NEW value, inside the SAME transaction as the
/// `stock_ledger_entry` insert that will use it — the exact atomicity
/// argument [`next_invoice_sequence_value`]'s doc comment makes for invoice
/// numbers, unchanged: a crash before this statement leaves the counter
/// untouched; a crash after it but before the enclosing `COMMIT` reverts it
/// along with the row that would have used it (WAL rollback); a crash after
/// `COMMIT` returns makes both durable together. There is no window in
/// which the mark advances without the row it belongs to also landing, or
/// vice versa.
pub(crate) fn next_stock_ledger_sequence_value(
    tx: &Transaction,
    outlet_id: &str,
    updated_at: &str,
) -> DbResult<i64> {
    tx.query_row(
        "INSERT INTO stock_ledger_sequence (outlet_id, last_value, updated_at)
         VALUES (?1, 1, ?2)
         ON CONFLICT(outlet_id) DO UPDATE SET
            last_value = stock_ledger_sequence.last_value + 1,
            updated_at = excluded.updated_at
         RETURNING last_value",
        params![outlet_id, updated_at],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

pub(crate) fn insert_invoice(tx: &Transaction, inv: &NewInvoice) -> DbResult<()> {
    tx.execute(
        "INSERT INTO invoice
            (id, outlet_id, order_id, split_group_id, split_index, split_count,
             series_id, invoice_number, invoice_date, business_date, status,
             customer_name, customer_phone, customer_gstin, place_of_supply_state_code,
             subtotal_paise, discount_paise, taxable_value_paise, cgst_paise, sgst_paise,
             igst_paise, cess_paise, round_off_paise, grand_total_paise,
             compliance_version_id, tax_snapshot_json, fiscal_profile_json,
             channel, tax_liability_party, eco_operator_name, eco_operator_gstin,
             supply_classification, created_by_user_id, created_at, updated_at,
             version, sync_status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'ISSUED',
                 ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23,
                 ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, 1, 'PENDING')",
        params![
            inv.id,
            inv.outlet_id,
            inv.order_id,
            inv.split_group_id,
            inv.split_index,
            inv.split_count,
            inv.series_id,
            inv.invoice_number,
            inv.invoice_date,
            inv.business_date,
            inv.customer_name,
            inv.customer_phone,
            inv.customer_gstin,
            inv.place_of_supply_state_code,
            inv.subtotal_paise,
            inv.discount_paise,
            inv.taxable_value_paise,
            inv.cgst_paise,
            inv.sgst_paise,
            inv.igst_paise,
            inv.cess_paise,
            inv.round_off_paise,
            inv.grand_total_paise,
            inv.compliance_version_id,
            inv.tax_snapshot_json,
            inv.fiscal_profile_json,
            inv.channel,
            inv.tax_liability_party,
            inv.eco_operator_name,
            inv.eco_operator_gstin,
            inv.supply_classification,
            inv.created_by_user_id,
            inv.created_at,
            inv.updated_at,
        ],
    )?;
    Ok(())
}

pub(crate) fn insert_invoice_line(tx: &Transaction, l: &NewInvoiceLine) -> DbResult<()> {
    tx.execute(
        "INSERT INTO invoice_line
            (id, invoice_id, order_item_id, line_no, description, hsn_sac, quantity,
             unit_price_paise, gross_paise, discount_paise, taxable_value_paise,
             tax_profile_id, cgst_rate_bps, cgst_paise, sgst_rate_bps, sgst_paise,
             igst_rate_bps, igst_paise, cess_rate_bps, cess_paise, total_paise)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                 ?16, ?17, ?18, ?19, ?20, ?21)",
        params![
            l.id,
            l.invoice_id,
            l.order_item_id,
            l.line_no,
            l.description,
            l.hsn_sac,
            l.quantity,
            l.unit_price_paise,
            l.gross_paise,
            l.discount_paise,
            l.taxable_value_paise,
            l.tax_profile_id,
            l.cgst_rate_bps,
            l.cgst_paise,
            l.sgst_rate_bps,
            l.sgst_paise,
            l.igst_rate_bps,
            l.igst_paise,
            l.cess_rate_bps,
            l.cess_paise,
            l.total_paise,
        ],
    )?;
    Ok(())
}

fn row_to_invoice(row: &rusqlite::Row) -> rusqlite::Result<Invoice> {
    Ok(Invoice {
        id: row.get(0)?,
        outlet_id: row.get(1)?,
        order_id: row.get(2)?,
        split_group_id: row.get(3)?,
        split_index: row.get(4)?,
        split_count: row.get(5)?,
        series_id: row.get(6)?,
        invoice_number: row.get(7)?,
        invoice_date: row.get(8)?,
        business_date: row.get(9)?,
        status: row.get(10)?,
        cancelled_reason: row.get(11)?,
        cancelled_at: row.get(12)?,
        customer_name: row.get(13)?,
        customer_phone: row.get(14)?,
        customer_gstin: row.get(15)?,
        place_of_supply_state_code: row.get(16)?,
        subtotal_paise: row.get(17)?,
        discount_paise: row.get(18)?,
        taxable_value_paise: row.get(19)?,
        cgst_paise: row.get(20)?,
        sgst_paise: row.get(21)?,
        igst_paise: row.get(22)?,
        cess_paise: row.get(23)?,
        round_off_paise: row.get(24)?,
        grand_total_paise: row.get(25)?,
        compliance_version_id: row.get(26)?,
        tax_snapshot_json: row.get(27)?,
        fiscal_profile_json: row.get(28)?,
        channel: row.get(29)?,
        tax_liability_party: row.get(30)?,
        eco_operator_name: row.get(31)?,
        eco_operator_gstin: row.get(32)?,
        supply_classification: row.get(33)?,
        created_by_user_id: row.get(34)?,
        created_at: row.get(35)?,
        updated_at: row.get(36)?,
        version: row.get(37)?,
        sync_status: row.get(38)?,
    })
}

const INVOICE_COLUMNS: &str = "id, outlet_id, order_id, split_group_id, split_index, split_count, \
    series_id, invoice_number, invoice_date, business_date, status, cancelled_reason, cancelled_at, \
    customer_name, customer_phone, customer_gstin, place_of_supply_state_code, \
    subtotal_paise, discount_paise, taxable_value_paise, cgst_paise, sgst_paise, igst_paise, \
    cess_paise, round_off_paise, grand_total_paise, compliance_version_id, tax_snapshot_json, \
    fiscal_profile_json, channel, tax_liability_party, eco_operator_name, eco_operator_gstin, \
    supply_classification, created_by_user_id, created_at, updated_at, version, sync_status";

pub fn get_invoice(conn: &Connection, id: &str) -> DbResult<Option<Invoice>> {
    conn.query_row(
        &format!("SELECT {INVOICE_COLUMNS} FROM invoice WHERE id = ?1"),
        params![id],
        row_to_invoice,
    )
    .optional()
    .map_err(Into::into)
}

/// Transaction-scoped twin of [`get_invoice`], for
/// [`crate::payment::tender::validate_forward`] reading the invoice it is
/// about to validate a tender against inside the same transaction as the
/// write (T9 retry: the double-settlement guard).
pub(crate) fn get_invoice_in_tx(tx: &Transaction, id: &str) -> DbResult<Option<Invoice>> {
    tx.query_row(
        &format!("SELECT {INVOICE_COLUMNS} FROM invoice WHERE id = ?1"),
        params![id],
        row_to_invoice,
    )
    .optional()
    .map_err(Into::into)
}

pub fn list_invoices_for_order(conn: &Connection, order_id: &str) -> DbResult<Vec<Invoice>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {INVOICE_COLUMNS} FROM invoice WHERE order_id = ?1 ORDER BY split_index, created_at"
    ))?;
    let rows = stmt
        .query_map(params![order_id], row_to_invoice)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Every invoice sharing one `split_group_id` (ADR-016 §4) — what the §66
/// conservation property is checked over.
pub fn list_invoices_for_split_group(
    conn: &Connection,
    split_group_id: &str,
) -> DbResult<Vec<Invoice>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {INVOICE_COLUMNS} FROM invoice WHERE split_group_id = ?1 ORDER BY split_index"
    ))?;
    let rows = stmt
        .query_map(params![split_group_id], row_to_invoice)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn row_to_invoice_line(row: &rusqlite::Row) -> rusqlite::Result<InvoiceLine> {
    Ok(InvoiceLine {
        id: row.get(0)?,
        invoice_id: row.get(1)?,
        order_item_id: row.get(2)?,
        line_no: row.get(3)?,
        description: row.get(4)?,
        hsn_sac: row.get(5)?,
        quantity: row.get(6)?,
        unit_price_paise: row.get(7)?,
        gross_paise: row.get(8)?,
        discount_paise: row.get(9)?,
        taxable_value_paise: row.get(10)?,
        tax_profile_id: row.get(11)?,
        cgst_rate_bps: row.get(12)?,
        cgst_paise: row.get(13)?,
        sgst_rate_bps: row.get(14)?,
        sgst_paise: row.get(15)?,
        igst_rate_bps: row.get(16)?,
        igst_paise: row.get(17)?,
        cess_rate_bps: row.get(18)?,
        cess_paise: row.get(19)?,
        total_paise: row.get(20)?,
    })
}

const INVOICE_LINE_COLUMNS: &str = "id, invoice_id, order_item_id, line_no, description, hsn_sac, \
    quantity, unit_price_paise, gross_paise, discount_paise, taxable_value_paise, tax_profile_id, \
    cgst_rate_bps, cgst_paise, sgst_rate_bps, sgst_paise, igst_rate_bps, igst_paise, cess_rate_bps, \
    cess_paise, total_paise";

pub fn list_invoice_lines(conn: &Connection, invoice_id: &str) -> DbResult<Vec<InvoiceLine>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {INVOICE_LINE_COLUMNS} FROM invoice_line WHERE invoice_id = ?1 ORDER BY line_no"
    ))?;
    let rows = stmt
        .query_map(params![invoice_id], row_to_invoice_line)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Transaction-scoped twin of [`list_invoice_lines`], for
/// [`crate::invoice::assemble`] reading back what it just wrote (inside the
/// SAME transaction) to build the `InvoiceCreated` outbox payload.
pub(crate) fn list_invoice_lines_in_tx(
    tx: &Transaction,
    invoice_id: &str,
) -> DbResult<Vec<InvoiceLine>> {
    let mut stmt = tx.prepare(&format!(
        "SELECT {INVOICE_LINE_COLUMNS} FROM invoice_line WHERE invoice_id = ?1 ORDER BY line_no"
    ))?;
    let rows = stmt
        .query_map(params![invoice_id], row_to_invoice_line)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

const EVENT_TYPE_INVOICE_CREATED: &str = "InvoiceCreated";

/// Builds the `InvoiceCreated` event envelope + `data` payload, matching
/// `InvoiceCreatedEventSchema` in `packages/contracts/src/types/events.ts`
/// exactly (`{ event_id, event_type, occurred_at, outlet_id, schema_version,
/// data: { invoice: {...InvoiceSchema, lines: [...InvoiceLineSchema] } } }`).
/// Built from the rows this crate just wrote (`inv`/`lines`), never from
/// caller-supplied fields, so a caller cannot commit a mismatched event for
/// a real issuance.
fn build_invoice_created_payload(
    inv: &Invoice,
    lines: &[InvoiceLine],
    event_id: &str,
    occurred_at: &str,
) -> DbResult<String> {
    let tax_snapshot: serde_json::Value =
        serde_json::from_str(&inv.tax_snapshot_json).map_err(|e| {
            crate::error::DbError::InvalidInput(format!(
                "stored tax_snapshot_json is not valid JSON: {e}"
            ))
        })?;
    let fiscal_profile: serde_json::Value = serde_json::from_str(&inv.fiscal_profile_json)
        .map_err(|e| {
            crate::error::DbError::InvalidInput(format!(
                "stored fiscal_profile_json is not valid JSON: {e}"
            ))
        })?;

    let lines_json: Vec<serde_json::Value> = lines
        .iter()
        .map(|l| {
            serde_json::json!({
                "id": l.id,
                "invoice_id": l.invoice_id,
                "order_item_id": l.order_item_id,
                "line_no": l.line_no,
                "description": l.description,
                "hsn_sac": l.hsn_sac,
                "quantity": l.quantity,
                "unit_price_paise": l.unit_price_paise,
                "gross_paise": l.gross_paise,
                "discount_paise": l.discount_paise,
                "taxable_value_paise": l.taxable_value_paise,
                "tax_profile_id": l.tax_profile_id,
                "cgst_rate_bps": l.cgst_rate_bps,
                "cgst_paise": l.cgst_paise,
                "sgst_rate_bps": l.sgst_rate_bps,
                "sgst_paise": l.sgst_paise,
                "igst_rate_bps": l.igst_rate_bps,
                "igst_paise": l.igst_paise,
                "cess_rate_bps": l.cess_rate_bps,
                "cess_paise": l.cess_paise,
                "total_paise": l.total_paise,
                "schema_version": 1,
            })
        })
        .collect();

    let invoice_json = serde_json::json!({
        "id": inv.id,
        "outlet_id": inv.outlet_id,
        "order_id": inv.order_id,
        "split_group_id": inv.split_group_id,
        "split_index": inv.split_index,
        "split_count": inv.split_count,
        "series_id": inv.series_id,
        "invoice_number": inv.invoice_number,
        "invoice_date": inv.invoice_date,
        "business_date": inv.business_date,
        "status": inv.status,
        "cancelled_reason": inv.cancelled_reason,
        "cancelled_at": inv.cancelled_at,
        "customer_name": inv.customer_name,
        "customer_phone": inv.customer_phone,
        "customer_gstin": inv.customer_gstin,
        "place_of_supply_state_code": inv.place_of_supply_state_code,
        "lines": lines_json,
        "subtotal_paise": inv.subtotal_paise,
        "discount_paise": inv.discount_paise,
        "taxable_value_paise": inv.taxable_value_paise,
        "cgst_paise": inv.cgst_paise,
        "sgst_paise": inv.sgst_paise,
        "igst_paise": inv.igst_paise,
        "cess_paise": inv.cess_paise,
        "round_off_paise": inv.round_off_paise,
        "grand_total_paise": inv.grand_total_paise,
        "compliance_version_id": inv.compliance_version_id,
        "tax_snapshot": tax_snapshot,
        "fiscal_profile": fiscal_profile,
        "channel": inv.channel,
        "tax_liability_party": inv.tax_liability_party,
        "eco_operator_name": inv.eco_operator_name,
        "eco_operator_gstin": inv.eco_operator_gstin,
        "supply_classification": inv.supply_classification,
        "created_by_user_id": inv.created_by_user_id,
        "created_at": inv.created_at,
        "updated_at": inv.updated_at,
        "version": inv.version,
        "schema_version": 1,
    });

    Ok(serde_json::json!({
        "event_id": event_id,
        "event_type": EVENT_TYPE_INVOICE_CREATED,
        "occurred_at": occurred_at,
        "outlet_id": inv.outlet_id,
        "schema_version": 1,
        "data": { "invoice": invoice_json },
    })
    .to_string())
}

/// Writes the `local_outbox` row for one issued invoice (ADR-016 §1: invoice
/// is EDGE_TO_CLOUD, so every issuance replays through this event) — the
/// `InvoiceCreated` analogue of [`insert_order_confirmed_outbox`].
pub(crate) fn insert_invoice_created_outbox(
    tx: &Transaction,
    inv: &Invoice,
    lines: &[InvoiceLine],
    meta: &InvoiceOutboxMeta,
) -> DbResult<()> {
    let payload = build_invoice_created_payload(inv, lines, &meta.outbox_id, &meta.occurred_at)?;
    insert_outbox_entry(
        tx,
        &NewOutboxEntry {
            id: meta.outbox_id.clone(),
            aggregate_type: "invoice".to_string(),
            aggregate_id: inv.id.clone(),
            event_type: EVENT_TYPE_INVOICE_CREATED.to_string(),
            payload_json: payload,
            created_at: meta.occurred_at.clone(),
        },
    )
}

// -------------------------------------------------- Milestone 3: payments (T7c) --
// `payment` is APPEND-ONLY (docs/spec/payments.md §Conflict policy). This
// module never issues an UPDATE or DELETE against it — `crate::payment`
// enforces the amount-sign/reversal rules before calling `insert_payment`,
// the only writer.

pub(crate) fn insert_payment(tx: &Transaction, p: &NewPayment) -> DbResult<()> {
    tx.execute(
        "INSERT INTO payment
            (id, outlet_id, order_id, cash_shift_id, method, status, amount_paise,
             tendered_paise, change_paise, reference, external_id, reverses_payment_id,
             captured_at, created_by_user_id, created_at, updated_at, version, sync_status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 1, 'PENDING')",
        params![
            p.id,
            p.outlet_id,
            p.order_id,
            p.cash_shift_id,
            p.method,
            p.status,
            p.amount_paise,
            p.tendered_paise,
            p.change_paise,
            p.reference,
            p.external_id,
            p.reverses_payment_id,
            p.captured_at,
            p.created_by_user_id,
            p.created_at,
            p.updated_at,
        ],
    )?;
    Ok(())
}

const PAYMENT_COLUMNS: &str = "id, outlet_id, order_id, cash_shift_id, method, status, \
    amount_paise, tendered_paise, change_paise, reference, external_id, reverses_payment_id, \
    captured_at, created_by_user_id, created_at, updated_at, version, sync_status";

fn row_to_payment(row: &rusqlite::Row) -> rusqlite::Result<Payment> {
    Ok(Payment {
        id: row.get(0)?,
        outlet_id: row.get(1)?,
        order_id: row.get(2)?,
        cash_shift_id: row.get(3)?,
        method: row.get(4)?,
        status: row.get(5)?,
        amount_paise: row.get(6)?,
        tendered_paise: row.get(7)?,
        change_paise: row.get(8)?,
        reference: row.get(9)?,
        external_id: row.get(10)?,
        reverses_payment_id: row.get(11)?,
        captured_at: row.get(12)?,
        created_by_user_id: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
        version: row.get(16)?,
        sync_status: row.get(17)?,
    })
}

pub fn get_payment(conn: &Connection, id: &str) -> DbResult<Option<Payment>> {
    conn.query_row(
        &format!("SELECT {PAYMENT_COLUMNS} FROM payment WHERE id = ?1"),
        params![id],
        row_to_payment,
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn get_payment_in_tx(tx: &Transaction, id: &str) -> DbResult<Option<Payment>> {
    tx.query_row(
        &format!("SELECT {PAYMENT_COLUMNS} FROM payment WHERE id = ?1"),
        params![id],
        row_to_payment,
    )
    .optional()
    .map_err(Into::into)
}

pub fn list_payments_for_order(conn: &Connection, order_id: &str) -> DbResult<Vec<Payment>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {PAYMENT_COLUMNS} FROM payment WHERE order_id = ?1 ORDER BY created_at, id"
    ))?;
    let rows = stmt
        .query_map(params![order_id], row_to_payment)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Every reversal row (`reverses_payment_id = payment_id`) posted so far,
/// oldest first — what [`crate::payment::tender::validate_reversal`] sums
/// against the original to derive how much of a payment is left to reverse.
pub(crate) fn list_reversals_for_payment_in_tx(
    tx: &Transaction,
    payment_id: &str,
) -> DbResult<Vec<Payment>> {
    let mut stmt = tx.prepare(&format!(
        "SELECT {PAYMENT_COLUMNS} FROM payment WHERE reverses_payment_id = ?1 ORDER BY created_at, id"
    ))?;
    let rows = stmt
        .query_map(params![payment_id], row_to_payment)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Writes one `payment_allocation` row — how one tender settles against one
/// invoice (T9 retry: the double-settlement guard needs this table actually
/// written, not merely present in the schema). Only writer of this table.
pub(crate) fn insert_payment_allocation(
    tx: &Transaction,
    a: &NewPaymentAllocation,
) -> DbResult<()> {
    tx.execute(
        "INSERT INTO payment_allocation (id, payment_id, invoice_id, amount_paise)
         VALUES (?1, ?2, ?3, ?4)",
        params![a.id, a.payment_id, a.invoice_id, a.amount_paise],
    )?;
    Ok(())
}

/// Net paise already allocated to `invoice_id` across every
/// `payment_allocation` row committed so far in this transaction (forward
/// tenders positive, reversals non-positive — the same signed-sum discipline
/// [`list_reversals_for_payment_in_tx`]'s caller already uses). What
/// [`crate::payment::tender::validate_forward`] subtracts from
/// `invoice.grand_total_paise` to derive remaining due.
pub(crate) fn sum_payment_allocations_for_invoice_in_tx(
    tx: &Transaction,
    invoice_id: &str,
) -> DbResult<i64> {
    tx.query_row(
        "SELECT COALESCE(SUM(amount_paise), 0) FROM payment_allocation WHERE invoice_id = ?1",
        params![invoice_id],
        |row| row.get::<_, i64>(0),
    )
    .map_err(Into::into)
}

/// The single invoice `payment_id` was allocated against, if any — what a
/// reversal derives its own allocation target from, rather than trusting a
/// caller to re-supply it (T9 retry). `None` when the original payment was
/// never allocated to an invoice (a cash sale taken before a bill existed)
/// OR — defensively — when it was allocated to more than one, which nothing
/// in this crate currently produces; a reversal in that shape simply posts
/// no allocation of its own rather than guessing which invoice to credit.
pub(crate) fn invoice_id_for_payment_in_tx(
    tx: &Transaction,
    payment_id: &str,
) -> DbResult<Option<String>> {
    let mut stmt = tx.prepare(
        "SELECT invoice_id FROM payment_allocation WHERE payment_id = ?1 ORDER BY id LIMIT 2",
    )?;
    let rows: Vec<String> = stmt
        .query_map(params![payment_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(match rows.len() {
        1 => Some(rows.into_iter().next().expect("len checked == 1")),
        _ => None,
    })
}

const EVENT_TYPE_PAYMENT_RECEIVED: &str = "PaymentReceived";
const EVENT_TYPE_PAYMENT_REFUNDED: &str = "PaymentRefunded";

/// Builds the `PaymentSchema`-shaped JSON embedded in both payment events —
/// `packages/contracts/src/types/payment.ts`. `allocations` carries every
/// `payment_allocation` row this tender actually produced (T9 retry: the
/// double-settlement fix wires this table, so the event payload no longer
/// silently claims a tender is allocated to nothing).
fn payment_json(p: &Payment, allocations: &[PaymentAllocation]) -> serde_json::Value {
    let allocations_json: Vec<serde_json::Value> = allocations
        .iter()
        .map(|a| {
            serde_json::json!({
                "id": a.id,
                "payment_id": a.payment_id,
                "invoice_id": a.invoice_id,
                "amount_paise": a.amount_paise,
                "schema_version": 1,
            })
        })
        .collect();
    serde_json::json!({
        "id": p.id,
        "outlet_id": p.outlet_id,
        "order_id": p.order_id,
        "cash_shift_id": p.cash_shift_id,
        "method": p.method,
        "status": p.status,
        "amount_paise": p.amount_paise,
        "tendered_paise": p.tendered_paise,
        "change_paise": p.change_paise,
        "reference": p.reference,
        "external_id": p.external_id,
        "reverses_payment_id": p.reverses_payment_id,
        "captured_at": p.captured_at,
        "allocations": allocations_json,
        "created_by_user_id": p.created_by_user_id,
        "created_at": p.created_at,
        "updated_at": p.updated_at,
        "version": p.version,
        "schema_version": 1,
    })
}

/// Writes the `local_outbox` row for a forward tender (`PaymentReceived`,
/// ADR-016 §1: `payment` is EDGE_TO_CLOUD).
pub(crate) fn insert_payment_received_outbox(
    tx: &Transaction,
    p: &Payment,
    allocations: &[PaymentAllocation],
    meta: &PaymentOutboxMeta,
) -> DbResult<()> {
    let payload = serde_json::json!({
        "event_id": meta.outbox_id,
        "event_type": EVENT_TYPE_PAYMENT_RECEIVED,
        "occurred_at": meta.occurred_at,
        "outlet_id": p.outlet_id,
        "schema_version": 1,
        "data": { "payment": payment_json(p, allocations) },
    })
    .to_string();
    insert_outbox_entry(
        tx,
        &NewOutboxEntry {
            id: meta.outbox_id.clone(),
            aggregate_type: "payment".to_string(),
            aggregate_id: p.id.clone(),
            event_type: EVENT_TYPE_PAYMENT_RECEIVED.to_string(),
            payload_json: payload,
            created_at: meta.occurred_at.clone(),
        },
    )
}

/// Writes the `local_outbox` row for a reversal (`PaymentRefunded`),
/// carrying `reverses_payment_id` alongside the reversal row itself so the
/// cloud never has to infer it (ADR-016 §1; `PaymentRefundedEventSchema`).
pub(crate) fn insert_payment_refunded_outbox(
    tx: &Transaction,
    p: &Payment,
    allocations: &[PaymentAllocation],
    reverses_payment_id: &str,
    meta: &PaymentOutboxMeta,
) -> DbResult<()> {
    let payload = serde_json::json!({
        "event_id": meta.outbox_id,
        "event_type": EVENT_TYPE_PAYMENT_REFUNDED,
        "occurred_at": meta.occurred_at,
        "outlet_id": p.outlet_id,
        "schema_version": 1,
        "data": {
            "payment": payment_json(p, allocations),
            "reverses_payment_id": reverses_payment_id,
        },
    })
    .to_string();
    insert_outbox_entry(
        tx,
        &NewOutboxEntry {
            id: meta.outbox_id.clone(),
            aggregate_type: "payment".to_string(),
            aggregate_id: p.id.clone(),
            event_type: EVENT_TYPE_PAYMENT_REFUNDED.to_string(),
            payload_json: payload,
            created_at: meta.occurred_at.clone(),
        },
    )
}

// ------------------------------------------------ Milestone 3: cash shift (T7c) --
// `cash_shift` is a workflow row with exactly one legal in-place transition
// (OPEN -> CLOSED, ADR-016 §1 / §39) — unlike `payment`, this table IS
// updated in place, once, by `close_cash_shift_in_tx` below. `cash_movement`
// rows underneath it are append-only, same discipline as `payment`.

pub(crate) fn count_open_cash_shifts_for_device_cashier(
    tx: &Transaction,
    device_id: &str,
    cashier_user_id: &str,
) -> DbResult<Option<String>> {
    tx.query_row(
        "SELECT id FROM cash_shift WHERE device_id = ?1 AND cashier_user_id = ?2 AND status = 'OPEN'",
        params![device_id, cashier_user_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn insert_cash_shift(tx: &Transaction, s: &NewCashShift) -> DbResult<()> {
    tx.execute(
        "INSERT INTO cash_shift
            (id, outlet_id, device_id, cashier_user_id, status, opened_at, opening_cash_paise,
             business_date, created_at, updated_at, version, sync_status)
         VALUES (?1, ?2, ?3, ?4, 'OPEN', ?5, ?6, ?7, ?8, ?9, 1, 'PENDING')",
        params![
            s.id,
            s.outlet_id,
            s.device_id,
            s.cashier_user_id,
            s.opened_at,
            s.opening_cash_paise,
            s.business_date,
            s.created_at,
            s.updated_at,
        ],
    )?;
    Ok(())
}

const CASH_SHIFT_COLUMNS: &str = "id, outlet_id, device_id, cashier_user_id, status, opened_at, \
    opening_cash_paise, closed_at, expected_cash_paise, actual_cash_paise, variance_paise, \
    variance_reason, business_date, created_at, updated_at, version, sync_status";

fn row_to_cash_shift(row: &rusqlite::Row) -> rusqlite::Result<CashShift> {
    Ok(CashShift {
        id: row.get(0)?,
        outlet_id: row.get(1)?,
        device_id: row.get(2)?,
        cashier_user_id: row.get(3)?,
        status: row.get(4)?,
        opened_at: row.get(5)?,
        opening_cash_paise: row.get(6)?,
        closed_at: row.get(7)?,
        expected_cash_paise: row.get(8)?,
        actual_cash_paise: row.get(9)?,
        variance_paise: row.get(10)?,
        variance_reason: row.get(11)?,
        business_date: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
        version: row.get(15)?,
        sync_status: row.get(16)?,
    })
}

pub fn get_cash_shift(conn: &Connection, id: &str) -> DbResult<Option<CashShift>> {
    conn.query_row(
        &format!("SELECT {CASH_SHIFT_COLUMNS} FROM cash_shift WHERE id = ?1"),
        params![id],
        row_to_cash_shift,
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn get_cash_shift_in_tx(tx: &Transaction, id: &str) -> DbResult<Option<CashShift>> {
    tx.query_row(
        &format!("SELECT {CASH_SHIFT_COLUMNS} FROM cash_shift WHERE id = ?1"),
        params![id],
        row_to_cash_shift,
    )
    .optional()
    .map_err(Into::into)
}

/// The cashier's currently OPEN shift on this device, if any (T9 retry —
/// "cash shift restart is an operational dead end"). Mirrors
/// [`count_open_cash_shifts_for_device_cashier`]'s own query
/// (`idx_cash_shift_open_device_cashier` guarantees at most one row), but
/// returns the full row rather than just the id, and is `pub` so the POS can
/// call it on startup to recover a shift orphaned by a restart — the
/// automatic recovery this crate previously had no query for at all.
pub fn find_open_cash_shift(
    conn: &Connection,
    device_id: &str,
    cashier_user_id: &str,
) -> DbResult<Option<CashShift>> {
    conn.query_row(
        &format!(
            "SELECT {CASH_SHIFT_COLUMNS} FROM cash_shift \
             WHERE device_id = ?1 AND cashier_user_id = ?2 AND status = 'OPEN'"
        ),
        params![device_id, cashier_user_id],
        row_to_cash_shift,
    )
    .optional()
    .map_err(Into::into)
}

/// The ONLY place this crate updates a `cash_shift` row after insert — the
/// single OPEN -> CLOSED transition (§39). Guarded by `AND status = 'OPEN'`
/// so a double-close (a race or a caller bug) affects zero rows rather than
/// silently re-closing; the caller checks `status` up front with
/// [`get_cash_shift_in_tx`] and treats zero rows here as a logic error
/// rather than the normal not-open rejection path.
#[allow(clippy::too_many_arguments)]
pub(crate) fn close_cash_shift_in_tx(
    tx: &Transaction,
    id: &str,
    closed_at: &str,
    expected_cash_paise: i64,
    actual_cash_paise: i64,
    variance_paise: i64,
    variance_reason: Option<&str>,
    updated_at: &str,
) -> DbResult<usize> {
    let n = tx.execute(
        "UPDATE cash_shift SET status = 'CLOSED', closed_at = ?1, expected_cash_paise = ?2, \
         actual_cash_paise = ?3, variance_paise = ?4, variance_reason = ?5, updated_at = ?6, \
         version = version + 1 \
         WHERE id = ?7 AND status = 'OPEN'",
        params![
            closed_at,
            expected_cash_paise,
            actual_cash_paise,
            variance_paise,
            variance_reason,
            updated_at,
            id,
        ],
    )?;
    Ok(n)
}

pub(crate) fn insert_cash_movement(tx: &Transaction, m: &NewCashMovement) -> DbResult<()> {
    tx.execute(
        "INSERT INTO cash_movement (id, cash_shift_id, kind, amount_paise, reason, payment_id, \
            created_by_user_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            m.id,
            m.cash_shift_id,
            m.kind,
            m.amount_paise,
            m.reason,
            m.payment_id,
            m.created_by_user_id,
            m.created_at,
        ],
    )?;
    Ok(())
}

const CASH_MOVEMENT_COLUMNS: &str =
    "id, cash_shift_id, kind, amount_paise, reason, payment_id, created_by_user_id, created_at";

fn row_to_cash_movement(row: &rusqlite::Row) -> rusqlite::Result<CashMovement> {
    Ok(CashMovement {
        id: row.get(0)?,
        cash_shift_id: row.get(1)?,
        kind: row.get(2)?,
        amount_paise: row.get(3)?,
        reason: row.get(4)?,
        payment_id: row.get(5)?,
        created_by_user_id: row.get(6)?,
        created_at: row.get(7)?,
    })
}

pub fn list_cash_movements_for_shift(
    conn: &Connection,
    cash_shift_id: &str,
) -> DbResult<Vec<CashMovement>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {CASH_MOVEMENT_COLUMNS} FROM cash_movement WHERE cash_shift_id = ?1 ORDER BY created_at, id"
    ))?;
    let rows = stmt
        .query_map(params![cash_shift_id], row_to_cash_movement)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub(crate) fn list_cash_movements_for_shift_in_tx(
    tx: &Transaction,
    cash_shift_id: &str,
) -> DbResult<Vec<CashMovement>> {
    let mut stmt = tx.prepare(&format!(
        "SELECT {CASH_MOVEMENT_COLUMNS} FROM cash_movement WHERE cash_shift_id = ?1 ORDER BY created_at, id"
    ))?;
    let rows = stmt
        .query_map(params![cash_shift_id], row_to_cash_movement)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The derived "Expected Cash" (§39): the sum of every `cash_movement`
/// posted against the shift so far. Never accepted from a caller — always
/// recomputed from the movement rows at close time
/// ([`crate::payment::cash_shift::close_cash_shift`]).
pub(crate) fn sum_cash_movements_for_shift_in_tx(
    tx: &Transaction,
    cash_shift_id: &str,
) -> DbResult<i64> {
    let total: Option<i64> = tx.query_row(
        "SELECT SUM(amount_paise) FROM cash_movement WHERE cash_shift_id = ?1",
        params![cash_shift_id],
        |row| row.get(0),
    )?;
    Ok(total.unwrap_or(0))
}

fn cash_movement_json(m: &CashMovement) -> serde_json::Value {
    serde_json::json!({
        "id": m.id,
        "cash_shift_id": m.cash_shift_id,
        "kind": m.kind,
        "amount_paise": m.amount_paise,
        "reason": m.reason,
        "payment_id": m.payment_id,
        "created_by_user_id": m.created_by_user_id,
        "created_at": m.created_at,
        "schema_version": 1,
    })
}

fn cash_shift_json(s: &CashShift, movements: &[CashMovement]) -> serde_json::Value {
    serde_json::json!({
        "id": s.id,
        "outlet_id": s.outlet_id,
        "device_id": s.device_id,
        "cashier_user_id": s.cashier_user_id,
        "status": s.status,
        "opened_at": s.opened_at,
        "opening_cash_paise": s.opening_cash_paise,
        "closed_at": s.closed_at,
        "expected_cash_paise": s.expected_cash_paise,
        "actual_cash_paise": s.actual_cash_paise,
        "variance_paise": s.variance_paise,
        "variance_reason": s.variance_reason,
        "business_date": s.business_date,
        "movements": movements.iter().map(cash_movement_json).collect::<Vec<_>>(),
        "created_at": s.created_at,
        "updated_at": s.updated_at,
        "version": s.version,
        "schema_version": 1,
    })
}

const EVENT_TYPE_CASH_SHIFT_OPENED: &str = "CashShiftOpened";
const EVENT_TYPE_CASH_SHIFT_CLOSED: &str = "CashShiftClosed";

pub(crate) fn insert_cash_shift_opened_outbox(
    tx: &Transaction,
    s: &CashShift,
    movements: &[CashMovement],
    meta: &CashShiftOutboxMeta,
) -> DbResult<()> {
    let payload = serde_json::json!({
        "event_id": meta.outbox_id,
        "event_type": EVENT_TYPE_CASH_SHIFT_OPENED,
        "occurred_at": meta.occurred_at,
        "outlet_id": s.outlet_id,
        "schema_version": 1,
        "data": { "shift": cash_shift_json(s, movements) },
    })
    .to_string();
    insert_outbox_entry(
        tx,
        &NewOutboxEntry {
            id: meta.outbox_id.clone(),
            aggregate_type: "cash_shift".to_string(),
            aggregate_id: s.id.clone(),
            event_type: EVENT_TYPE_CASH_SHIFT_OPENED.to_string(),
            payload_json: payload,
            created_at: meta.occurred_at.clone(),
        },
    )
}

pub(crate) fn insert_cash_shift_closed_outbox(
    tx: &Transaction,
    s: &CashShift,
    movements: &[CashMovement],
    meta: &CashShiftOutboxMeta,
) -> DbResult<()> {
    let payload = serde_json::json!({
        "event_id": meta.outbox_id,
        "event_type": EVENT_TYPE_CASH_SHIFT_CLOSED,
        "occurred_at": meta.occurred_at,
        "outlet_id": s.outlet_id,
        "schema_version": 1,
        "data": { "shift": cash_shift_json(s, movements) },
    })
    .to_string();
    insert_outbox_entry(
        tx,
        &NewOutboxEntry {
            id: meta.outbox_id.clone(),
            aggregate_type: "cash_shift".to_string(),
            aggregate_id: s.id.clone(),
            event_type: EVENT_TYPE_CASH_SHIFT_CLOSED.to_string(),
            payload_json: payload,
            created_at: meta.occurred_at.clone(),
        },
    )
}

#[cfg(test)]
mod m3_billing_config_tests {
    use super::*;
    use crate::Db;

    fn seed_outlet(conn: &Connection, outlet_id: &str) {
        upsert_outlet(
            conn,
            &Outlet {
                id: outlet_id.to_string(),
                brand_id: "brand-1".to_string(),
                name: "Test Outlet".to_string(),
                timezone: "Asia/Kolkata".to_string(),
                config_version: 1,
                created_at: "2026-08-14T00:00:00Z".to_string(),
                updated_at: "2026-08-14T00:00:00Z".to_string(),
            },
        )
        .expect("seed outlet");
    }

    fn sample_profile(config_version: i64) -> TaxProfile {
        TaxProfile {
            id: "profile-1".to_string(),
            outlet_id: "outlet-1".to_string(),
            code: "GST_5_RESTAURANT".to_string(),
            name: "GST 5% Restaurant".to_string(),
            pricing_mode: "EXCLUSIVE".to_string(),
            is_default: true,
            is_active: true,
            config_version,
        }
    }

    #[test]
    fn tax_profile_round_trips() {
        let db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");
        upsert_tax_profile(db.connection(), &sample_profile(1)).expect("insert");

        let got = list_tax_profiles_for_outlet(db.connection(), "outlet-1").expect("list");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].code, "GST_5_RESTAURANT");
        assert!(got[0].is_default);
    }

    /// Mirrors `upsert_station`'s config_version guard: a stale config bundle
    /// must never regress an already-newer row.
    #[test]
    fn stale_config_version_does_not_overwrite_newer_tax_profile() {
        let db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");
        let mut newer = sample_profile(5);
        newer.name = "Newer Name".to_string();
        upsert_tax_profile(db.connection(), &newer).expect("insert v5");

        let mut stale = sample_profile(3);
        stale.name = "Stale Name".to_string();
        upsert_tax_profile(db.connection(), &stale).expect("stale write must not error");

        let got = list_tax_profiles_for_outlet(db.connection(), "outlet-1").expect("list");
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].config_version, 5,
            "newer row must survive a stale replay"
        );
        assert_eq!(got[0].name, "Newer Name");
    }

    #[test]
    fn compliance_version_tax_rule_and_fiscal_profile_round_trip() {
        let db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");
        upsert_tax_profile(db.connection(), &sample_profile(1)).expect("profile");

        upsert_compliance_version(
            db.connection(),
            &ComplianceVersion {
                id: "cv-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                label: "GST 2026-04".to_string(),
                effective_from: "2026-04-01T00:00:00Z".to_string(),
                notes: None,
                config_version: 1,
            },
        )
        .expect("compliance version");

        upsert_tax_rule(
            db.connection(),
            &TaxRule {
                id: "rule-1".to_string(),
                tax_profile_id: "profile-1".to_string(),
                compliance_version_id: "cv-1".to_string(),
                component: "CGST".to_string(),
                rate_bps: 250,
                effective_from: "2026-04-01T00:00:00Z".to_string(),
                effective_to: None,
                config_version: 1,
            },
        )
        .expect("tax rule");

        upsert_outlet_fiscal_profile(
            db.connection(),
            &OutletFiscalProfile {
                id: "fiscal-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                legal_name: "Test Restaurant Pvt Ltd".to_string(),
                trade_name: "Test Restaurant".to_string(),
                address_line1: "123 Main St".to_string(),
                address_line2: None,
                city: "Pune".to_string(),
                state_code: "27".to_string(),
                state_name: "Maharashtra".to_string(),
                pincode: "411001".to_string(),
                gstin: "27AAAAA0000A1Z5".to_string(),
                fssai_number: None,
                invoice_footer_text: None,
                effective_from: "2026-04-01T00:00:00Z".to_string(),
                config_version: 1,
            },
        )
        .expect("fiscal profile");

        assert_eq!(
            list_compliance_versions_for_outlet(db.connection(), "outlet-1")
                .expect("list")
                .len(),
            1
        );
        let rules = list_tax_rules_for_profile(db.connection(), "profile-1").expect("list");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].rate_bps, 250);
        assert_eq!(
            list_outlet_fiscal_profiles_for_outlet(db.connection(), "outlet-1")
                .expect("list")
                .len(),
            1
        );
    }

    #[test]
    fn invoice_series_and_discount_definition_round_trip() {
        let db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");

        upsert_invoice_series(
            db.connection(),
            &InvoiceSeries {
                id: "series-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                code: "SALES".to_string(),
                prefix_template: "FY{FY}/{OUTLET}/".to_string(),
                reset_policy: "FY".to_string(),
                padding_width: 6,
                is_active: true,
                config_version: 1,
            },
        )
        .expect("series");

        upsert_discount_definition(
            db.connection(),
            &DiscountDefinition {
                id: "discount-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                code: "STAFF".to_string(),
                name: "Staff 20%".to_string(),
                scope: "BILL".to_string(),
                method: "PERCENT".to_string(),
                value_bps: Some(2000),
                value_paise: None,
                max_discount_paise: None,
                required_permission: None,
                requires_reason: false,
                is_active: true,
                effective_from: "2026-04-01T00:00:00Z".to_string(),
                effective_to: None,
                config_version: 1,
            },
        )
        .expect("discount");

        assert_eq!(
            list_invoice_series_for_outlet(db.connection(), "outlet-1")
                .expect("list")
                .len(),
            1
        );
        let discounts =
            list_discount_definitions_for_outlet(db.connection(), "outlet-1").expect("list");
        assert_eq!(discounts.len(), 1);
        assert_eq!(discounts[0].value_bps, Some(2000));
    }

    // -------------------------------------------------- stale-config-version --
    // Every M3 config upsert shares the same `WHERE excluded.config_version
    // >= <table>.config_version` guard as `upsert_tax_profile` (already
    // covered above). Each of the remaining five gets its own dedicated
    // stale-replay test rather than being judged uniform by inspection —
    // inspection is exactly the standard of evidence a copy-paste slip in
    // one WHERE clause survives.

    fn sample_compliance_version(config_version: i64) -> ComplianceVersion {
        ComplianceVersion {
            id: "cv-1".to_string(),
            outlet_id: "outlet-1".to_string(),
            label: "GST 2026-04".to_string(),
            effective_from: "2026-04-01T00:00:00Z".to_string(),
            notes: None,
            config_version,
        }
    }

    #[test]
    fn stale_config_version_does_not_overwrite_newer_compliance_version() {
        let db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");

        let mut newer = sample_compliance_version(5);
        newer.label = "Newer Label".to_string();
        upsert_compliance_version(db.connection(), &newer).expect("insert v5");

        let mut stale = sample_compliance_version(3);
        stale.label = "Stale Label".to_string();
        upsert_compliance_version(db.connection(), &stale).expect("stale write must not error");

        let got = list_compliance_versions_for_outlet(db.connection(), "outlet-1").expect("list");
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].config_version, 5,
            "newer row must survive a stale replay"
        );
        assert_eq!(got[0].label, "Newer Label");
    }

    fn sample_tax_rule(config_version: i64) -> TaxRule {
        TaxRule {
            id: "rule-1".to_string(),
            tax_profile_id: "profile-1".to_string(),
            compliance_version_id: "cv-1".to_string(),
            component: "CGST".to_string(),
            rate_bps: 250,
            effective_from: "2026-04-01T00:00:00Z".to_string(),
            effective_to: None,
            config_version,
        }
    }

    #[test]
    fn stale_config_version_does_not_overwrite_newer_tax_rule() {
        let db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");
        upsert_tax_profile(db.connection(), &sample_profile(1)).expect("profile");
        upsert_compliance_version(db.connection(), &sample_compliance_version(1)).expect("cv");

        let mut newer = sample_tax_rule(5);
        newer.rate_bps = 900;
        upsert_tax_rule(db.connection(), &newer).expect("insert v5");

        let mut stale = sample_tax_rule(3);
        stale.rate_bps = 100;
        upsert_tax_rule(db.connection(), &stale).expect("stale write must not error");

        let got = list_tax_rules_for_profile(db.connection(), "profile-1").expect("list");
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].config_version, 5,
            "newer row must survive a stale replay"
        );
        assert_eq!(
            got[0].rate_bps, 900,
            "a stale replay must not regress the rate"
        );
    }

    fn sample_fiscal_profile(config_version: i64) -> OutletFiscalProfile {
        OutletFiscalProfile {
            id: "fiscal-1".to_string(),
            outlet_id: "outlet-1".to_string(),
            legal_name: "Test Restaurant Pvt Ltd".to_string(),
            trade_name: "Test Restaurant".to_string(),
            address_line1: "123 Main St".to_string(),
            address_line2: None,
            city: "Pune".to_string(),
            state_code: "27".to_string(),
            state_name: "Maharashtra".to_string(),
            pincode: "411001".to_string(),
            gstin: "27AAAAA0000A1Z5".to_string(),
            fssai_number: None,
            invoice_footer_text: None,
            effective_from: "2026-04-01T00:00:00Z".to_string(),
            config_version,
        }
    }

    #[test]
    fn stale_config_version_does_not_overwrite_newer_outlet_fiscal_profile() {
        let db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");

        let mut newer = sample_fiscal_profile(5);
        newer.gstin = "27NEWGSTIN0001Z5".to_string();
        upsert_outlet_fiscal_profile(db.connection(), &newer).expect("insert v5");

        let mut stale = sample_fiscal_profile(3);
        stale.gstin = "27STALEGSTIN001Z5".to_string();
        upsert_outlet_fiscal_profile(db.connection(), &stale).expect("stale write must not error");

        let got =
            list_outlet_fiscal_profiles_for_outlet(db.connection(), "outlet-1").expect("list");
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].config_version, 5,
            "newer row must survive a stale replay"
        );
        assert_eq!(got[0].gstin, "27NEWGSTIN0001Z5");
    }

    fn sample_invoice_series(config_version: i64) -> InvoiceSeries {
        InvoiceSeries {
            id: "series-1".to_string(),
            outlet_id: "outlet-1".to_string(),
            code: "SALES".to_string(),
            prefix_template: "FY{FY}/{OUTLET}/".to_string(),
            reset_policy: "FY".to_string(),
            padding_width: 6,
            is_active: true,
            config_version,
        }
    }

    #[test]
    fn stale_config_version_does_not_overwrite_newer_invoice_series() {
        let db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");

        let mut newer = sample_invoice_series(5);
        newer.is_active = false; // e.g. the series was retired at v5
        upsert_invoice_series(db.connection(), &newer).expect("insert v5");

        let mut stale = sample_invoice_series(3);
        stale.is_active = true;
        upsert_invoice_series(db.connection(), &stale).expect("stale write must not error");

        let got = list_invoice_series_for_outlet(db.connection(), "outlet-1").expect("list");
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].config_version, 5,
            "newer row must survive a stale replay"
        );
        assert!(
            !got[0].is_active,
            "a stale replay must not resurrect a series retired at a newer config_version"
        );
    }

    fn sample_discount_definition(config_version: i64) -> DiscountDefinition {
        DiscountDefinition {
            id: "discount-1".to_string(),
            outlet_id: "outlet-1".to_string(),
            code: "STAFF".to_string(),
            name: "Staff 20%".to_string(),
            scope: "BILL".to_string(),
            method: "PERCENT".to_string(),
            value_bps: Some(2000),
            value_paise: None,
            max_discount_paise: None,
            required_permission: None,
            requires_reason: false,
            is_active: true,
            effective_from: "2026-04-01T00:00:00Z".to_string(),
            effective_to: None,
            config_version,
        }
    }

    #[test]
    fn stale_config_version_does_not_overwrite_newer_discount_definition() {
        let db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");

        let mut newer = sample_discount_definition(5);
        newer.value_bps = Some(1000); // discount policy tightened at v5
        upsert_discount_definition(db.connection(), &newer).expect("insert v5");

        let mut stale = sample_discount_definition(3);
        stale.value_bps = Some(2000);
        upsert_discount_definition(db.connection(), &stale).expect("stale write must not error");

        let got = list_discount_definitions_for_outlet(db.connection(), "outlet-1").expect("list");
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].config_version, 5,
            "newer row must survive a stale replay"
        );
        assert_eq!(
            got[0].value_bps,
            Some(1000),
            "a stale replay must not loosen a discount tightened at a newer config_version"
        );
    }
}

// ---------------------------------- Milestone 4: wastage / counts / variance
// / snapshot sealing (T3, ADR-018) -------------------------------------------
// Business logic (permission gating is the caller's, per ADR-018 §11 — see
// `crate::stock`'s module doc comment) lives in `crate::stock::*`; this
// section is the plain SQL these functions call, matching the split
// `crate::payment::cash_shift` already draws against this file.

/// `stock_ledger_entry`, as stored — the reverse of `deduction::ledger::
/// insert_stock_ledger_entry`'s column list. `&Connection`, not
/// `&Transaction`: callers that need this inside an open write transaction
/// pass `&tx` (`rusqlite::Transaction` derefs to `Connection`), the same
/// pattern `get_outlet_business_date_config` documents for the reverse
/// direction.
fn stock_ledger_entry_from_row(row: &rusqlite::Row) -> rusqlite::Result<StockLedgerEntry> {
    Ok(StockLedgerEntry {
        id: row.get(0)?,
        outlet_id: row.get(1)?,
        entry_seq: row.get(2)?,
        inventory_item_id: row.get(3)?,
        inventory_item_name: row.get(4)?,
        dimension: row.get(5)?,
        entry_type: row.get(6)?,
        origin: row.get(7)?,
        quantity_applied_micro: row.get(8)?,
        recipe_id: row.get(9)?,
        recipe_version: row.get(10)?,
        recipe_name: row.get(11)?,
        source_order_id: row.get(12)?,
        source_order_item_id: row.get(13)?,
        reason_code: row.get(14)?,
        note: row.get(15)?,
        occurred_at: row.get(16)?,
        business_date: row.get(17)?,
        created_by_user_id: row.get(18)?,
        modifier_delta_id: row.get(19)?,
        modifier_name: row.get(20)?,
        modifier_delta_version: row.get(21)?,
        unit_cost_paise: row.get(22)?,
        source_stock_count_id: row.get(23)?,
    })
}

const STOCK_LEDGER_ENTRY_COLUMNS: &str = "id, outlet_id, entry_seq, inventory_item_id, \
    inventory_item_name, dimension, entry_type, origin, quantity_applied_micro, recipe_id, \
    recipe_version, recipe_name, source_order_id, source_order_item_id, reason_code, note, \
    occurred_at, business_date, created_by_user_id, modifier_delta_id, modifier_name, \
    modifier_delta_version, unit_cost_paise, source_stock_count_id";

/// Fetches the exact row a just-completed insert wrote, by `(outlet_id,
/// entry_seq)` — the table's own uniqueness key (0016) — so
/// `crate::stock::wastage::record_wastage` can hand the caller back what was
/// actually persisted rather than re-deriving it from the request.
pub(crate) fn get_stock_ledger_entry_by_seq(
    conn: &Connection,
    outlet_id: &str,
    entry_seq: i64,
) -> DbResult<Option<StockLedgerEntry>> {
    conn.query_row(
        &format!(
            "SELECT {STOCK_LEDGER_ENTRY_COLUMNS} FROM stock_ledger_entry \
             WHERE outlet_id = ?1 AND entry_seq = ?2"
        ),
        params![outlet_id, entry_seq],
        stock_ledger_entry_from_row,
    )
    .optional()
    .map_err(Into::into)
}

/// `(name, dimension)` for one `inventory_item` — the snapshot every M4
/// stock write needs and no config row's own identity should ever be
/// trusted to still carry later (0016's no-FK provenance rule).
pub(crate) fn get_inventory_item_snapshot(
    conn: &Connection,
    inventory_item_id: &str,
) -> DbResult<Option<(String, String)>> {
    conn.query_row(
        "SELECT name, dimension FROM inventory_item WHERE id = ?1",
        params![inventory_item_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(Into::into)
}

// -------------------------------------------------------- stock_count ------

pub(crate) fn insert_stock_count(
    conn: &Connection,
    id: &str,
    outlet_id: &str,
    business_date: &str,
    started_at: &str,
    counted_by_user_id: Option<&str>,
    note: Option<&str>,
) -> DbResult<()> {
    conn.execute(
        "INSERT INTO stock_count
            (id, outlet_id, business_date, status, started_at, completed_at,
             counted_by_user_id, note)
         VALUES (?1, ?2, ?3, 'OPEN', ?4, NULL, ?5, ?6)",
        params![
            id,
            outlet_id,
            business_date,
            started_at,
            counted_by_user_id,
            note
        ],
    )?;
    Ok(())
}

fn stock_count_from_row(row: &rusqlite::Row) -> rusqlite::Result<StockCount> {
    Ok(StockCount {
        id: row.get(0)?,
        outlet_id: row.get(1)?,
        business_date: row.get(2)?,
        status: row.get(3)?,
        started_at: row.get(4)?,
        completed_at: row.get(5)?,
        counted_by_user_id: row.get(6)?,
        note: row.get(7)?,
    })
}

const STOCK_COUNT_COLUMNS: &str = "id, outlet_id, business_date, status, started_at, \
    completed_at, counted_by_user_id, note";

pub(crate) fn get_stock_count(conn: &Connection, id: &str) -> DbResult<Option<StockCount>> {
    conn.query_row(
        &format!("SELECT {STOCK_COUNT_COLUMNS} FROM stock_count WHERE id = ?1"),
        params![id],
        stock_count_from_row,
    )
    .optional()
    .map_err(Into::into)
}

/// Marks a stock count COMPLETED. Returns the number of rows affected (`0`
/// or `1`) rather than erroring itself — `crate::stock::count` checks
/// `status == 'OPEN'` before calling this in the SAME transaction, so `0`
/// here can only mean a logic error in that caller, not a legitimate race
/// (the edge is a single SQLite writer, ADR-018 Rule 3) — the same
/// "checked-then-affected" shape `close_cash_shift_in_tx` already uses.
pub(crate) fn mark_stock_count_completed(
    tx: &Transaction,
    id: &str,
    completed_at: &str,
) -> DbResult<usize> {
    let affected = tx.execute(
        "UPDATE stock_count SET completed_at = ?2, status = 'COMPLETED' \
         WHERE id = ?1 AND status = 'OPEN'",
        params![id, completed_at],
    )?;
    Ok(affected)
}

fn stock_count_line_from_row(row: &rusqlite::Row) -> rusqlite::Result<StockCountLine> {
    Ok(StockCountLine {
        id: row.get(0)?,
        stock_count_id: row.get(1)?,
        inventory_item_id: row.get(2)?,
        inventory_item_name: row.get(3)?,
        dimension: row.get(4)?,
        counted_quantity_micro: row.get(5)?,
        expected_quantity_micro: row.get(6)?,
        note: row.get(7)?,
    })
}

const STOCK_COUNT_LINE_COLUMNS: &str = "id, stock_count_id, inventory_item_id, \
    inventory_item_name, dimension, counted_quantity_micro, expected_quantity_micro, note";

/// Inserts a new line, or corrects an existing one for the same
/// `(stock_count_id, inventory_item_id)` (the table's own `UNIQUE` index) —
/// "mutable while OPEN" (0016's header). `new_id_if_inserted` is used only
/// on the INSERT branch; on a conflict the existing row's `id` survives
/// untouched (the `DO UPDATE SET` clause never assigns `id`), so a line's
/// identity is stable across corrections. If `stock_count_id` names a count
/// that is not `OPEN`, the table's own
/// `stock_count_line_is_immutable_once_completed` trigger aborts this —
/// `crate::stock::count::add_or_update_count_line` checks status first so
/// the caller gets `DbError::StockCountNotOpen` instead of a raw trigger
/// failure surfacing as `DbError::Sqlite`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn upsert_stock_count_line(
    tx: &Transaction,
    new_id_if_inserted: &str,
    stock_count_id: &str,
    inventory_item_id: &str,
    inventory_item_name: &str,
    dimension: &str,
    counted_quantity_micro: i64,
    expected_quantity_micro: i64,
    note: Option<&str>,
) -> DbResult<StockCountLine> {
    tx.query_row(
        &format!(
            "INSERT INTO stock_count_line
                (id, stock_count_id, inventory_item_id, inventory_item_name, dimension,
                 counted_quantity_micro, expected_quantity_micro, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(stock_count_id, inventory_item_id) DO UPDATE SET
                inventory_item_name = excluded.inventory_item_name,
                dimension = excluded.dimension,
                counted_quantity_micro = excluded.counted_quantity_micro,
                expected_quantity_micro = excluded.expected_quantity_micro,
                note = excluded.note
             RETURNING {STOCK_COUNT_LINE_COLUMNS}"
        ),
        params![
            new_id_if_inserted,
            stock_count_id,
            inventory_item_id,
            inventory_item_name,
            dimension,
            counted_quantity_micro,
            expected_quantity_micro,
            note,
        ],
        stock_count_line_from_row,
    )
    .map_err(Into::into)
}

pub(crate) fn list_stock_count_lines(
    conn: &Connection,
    stock_count_id: &str,
) -> DbResult<Vec<StockCountLine>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {STOCK_COUNT_LINE_COLUMNS} FROM stock_count_line \
         WHERE stock_count_id = ?1 ORDER BY inventory_item_name"
    ))?;
    let rows = stmt
        .query_map(params![stock_count_id], stock_count_line_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The total sellable units sold with an unresolvable recipe (ADR-018 §10.1
/// "N sales unaccounted"), for one outlet, up to and including
/// `business_date` — the same cumulative posture the bounded stock read
/// itself takes, since a count's `expected_quantity_micro` is also
/// cumulative-since-forever, not scoped to one day.
pub(crate) fn sum_unaccounted_sales_through_business_date(
    conn: &Connection,
    outlet_id: &str,
    business_date: &str,
) -> DbResult<i64> {
    conn.query_row(
        "SELECT COALESCE(SUM(quantity), 0) FROM stock_deduction_gap \
         WHERE outlet_id = ?1 AND business_date <= ?2",
        params![outlet_id, business_date],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

// --------------------------------------------- stock_balance_snapshot ------
// EDGE-LOCAL, no AggregateType, no sync direction, ever (ADR-018 §9). See
// `crate::stock::snapshot` for the sealing algorithm these functions serve.

/// The bounded stock read (ADR-018 §9's formula, verbatim): the latest
/// SEALED snapshot's closing balance plus every entry NOT COVERED BY ITS
/// MARK — `entry_seq > through_entry_seq`, never `business_date > business_date`
/// (a late arrival carrying a sealed day's business_date must still be
/// counted; see the 0017 migration header for the falsified failure mode).
/// `0` covers an item that has never been sealed AND never had a ledger
/// entry — genuinely zero stock recorded, not a sentinel.
pub(crate) fn get_current_stock(
    conn: &Connection,
    outlet_id: &str,
    inventory_item_id: &str,
) -> DbResult<i64> {
    let sealed: Option<(i64, i64)> = conn
        .query_row(
            "SELECT closing_quantity_micro, through_entry_seq FROM stock_balance_snapshot \
             WHERE outlet_id = ?1 AND inventory_item_id = ?2 \
             ORDER BY business_date DESC LIMIT 1",
            params![outlet_id, inventory_item_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (closing, mark) = sealed.unwrap_or((0, 0));
    let delta: i64 = conn.query_row(
        "SELECT COALESCE(SUM(quantity_applied_micro), 0) FROM stock_ledger_entry \
         WHERE outlet_id = ?1 AND inventory_item_id = ?2 AND entry_seq > ?3",
        params![outlet_id, inventory_item_id, mark],
        |row| row.get(0),
    )?;
    Ok(closing + delta)
}

/// The outlet-wide bounded stock read T5's low-stock surfacing reads:
/// every `is_active` `inventory_item`, joined against
/// [`get_current_stock`]'s same formula per item (a correlated subquery,
/// not a materialised join — ADR-018 §9: "there is no materialized
/// current-stock table"). Ordered by name for a stable, human-readable list.
pub(crate) fn list_current_stock_for_outlet(
    conn: &Connection,
    outlet_id: &str,
) -> DbResult<Vec<CurrentStockLine>> {
    let mut stmt = conn.prepare(
        "SELECT ii.id, ii.name, ii.dimension, ii.reorder_level_micro, ii.par_level_micro, \
                COALESCE(snap.closing_quantity_micro, 0) + COALESCE((
                    SELECT SUM(sle.quantity_applied_micro) FROM stock_ledger_entry sle
                     WHERE sle.outlet_id = ii.outlet_id AND sle.inventory_item_id = ii.id
                       AND sle.entry_seq > COALESCE(snap.through_entry_seq, 0)
                ), 0) AS current_quantity_micro
         FROM inventory_item ii
         LEFT JOIN stock_balance_snapshot snap
           ON snap.outlet_id = ii.outlet_id AND snap.inventory_item_id = ii.id
          AND snap.business_date = (
              SELECT MAX(s2.business_date) FROM stock_balance_snapshot s2
               WHERE s2.outlet_id = ii.outlet_id AND s2.inventory_item_id = ii.id
          )
         WHERE ii.outlet_id = ?1 AND ii.is_active = 1
         ORDER BY ii.name",
    )?;
    let rows = stmt
        .query_map(params![outlet_id], |row| {
            Ok(CurrentStockLine {
                inventory_item_id: row.get(0)?,
                inventory_item_name: row.get(1)?,
                dimension: row.get(2)?,
                reorder_level_micro: row.get(3)?,
                par_level_micro: row.get(4)?,
                current_quantity_micro: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Every distinct `(inventory_item_id, business_date)` pair that has ledger
/// activity strictly before `today` and no existing sealed snapshot for
/// that exact day, ascending by date then item — the catch-up work list
/// (ADR-018 §9.1: "every unsealed prior business day is sealed, in order").
pub(crate) fn find_unsealed_item_days_before(
    tx: &Transaction,
    outlet_id: &str,
    today_business_date: &str,
) -> DbResult<Vec<(String, String)>> {
    let mut stmt = tx.prepare(
        "SELECT DISTINCT sle.inventory_item_id, sle.business_date
         FROM stock_ledger_entry sle
         WHERE sle.outlet_id = ?1 AND sle.business_date < ?2
           AND NOT EXISTS (
               SELECT 1 FROM stock_balance_snapshot s
                WHERE s.outlet_id = sle.outlet_id AND s.inventory_item_id = sle.inventory_item_id
                  AND s.business_date = sle.business_date
           )
         ORDER BY sle.business_date ASC, sle.inventory_item_id ASC",
    )?;
    let rows = stmt
        .query_map(params![outlet_id, today_business_date], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// For one `(outlet, item)`, as of the end of `through_business_date`
/// (inclusive): the item's own most recent `dimension`, the HIGH-WATER MARK
/// (`MAX(entry_seq)` among its own entries dated on-or-before that day —
/// each item's mark is independent, since the bounded read filters by item
/// too) and the full closing balance up to that mark. Recomputed as a full
/// sum rather than incrementally from a prior seal: catch-up sealing is rare
/// (it runs once per skipped day, at `Db::open`), so the simpler, obviously-
/// correct form is worth more here than the saved re-scan.
pub(crate) fn compute_seal_for_item_day(
    tx: &Transaction,
    outlet_id: &str,
    inventory_item_id: &str,
    through_business_date: &str,
) -> DbResult<Option<(String, i64, i64)>> {
    tx.query_row(
        "SELECT
            (SELECT dimension FROM stock_ledger_entry
              WHERE outlet_id = ?1 AND inventory_item_id = ?2 AND business_date <= ?3
              ORDER BY entry_seq DESC LIMIT 1) AS dimension,
            MAX(entry_seq) AS mark,
            SUM(quantity_applied_micro) AS closing
         FROM stock_ledger_entry
         WHERE outlet_id = ?1 AND inventory_item_id = ?2 AND business_date <= ?3",
        params![outlet_id, inventory_item_id, through_business_date],
        |row| {
            let dimension: Option<String> = row.get(0)?;
            let mark: Option<i64> = row.get(1)?;
            let closing: Option<i64> = row.get(2)?;
            Ok(match (dimension, mark, closing) {
                (Some(d), Some(m), Some(c)) => Some((d, m, c)),
                _ => None,
            })
        },
    )
    .map_err(Into::into)
}

/// Seals one `(outlet, item, business_date)` — an INSERT whose primary-key
/// collision is a NO-OP, never an UPDATE (the 0017 trigger enforces this;
/// `INSERT OR IGNORE` is how this function stays idempotent without ever
/// tripping it — ADR-018 §9.1: "sealing an already-sealed day is a no-op,
/// not an error and not a second row").
#[allow(clippy::too_many_arguments)]
pub(crate) fn seal_snapshot(
    tx: &Transaction,
    outlet_id: &str,
    inventory_item_id: &str,
    business_date: &str,
    closing_quantity_micro: i64,
    dimension: &str,
    through_entry_seq: i64,
    sealed_at: &str,
) -> DbResult<()> {
    tx.execute(
        "INSERT OR IGNORE INTO stock_balance_snapshot
            (outlet_id, inventory_item_id, business_date, closing_quantity_micro, dimension,
             through_entry_seq, sealed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            outlet_id,
            inventory_item_id,
            business_date,
            closing_quantity_micro,
            dimension,
            through_entry_seq,
            sealed_at,
        ],
    )?;
    Ok(())
}

pub(crate) fn list_all_outlet_ids(tx: &Transaction) -> DbResult<Vec<String>> {
    let mut stmt = tx.prepare("SELECT id FROM outlet")?;
    let rows = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
