//! Config pull (cloud→edge, §50.1). `GET /sync/config` returns a bundle of
//! `CLOUD_TO_EDGE` aggregates that the edge applies as a wholesale replace
//! per `config_version` — never merged, never last-write-wins (ADR-009).
//!
//! Credential handling (ADR-011): `EdgeUserCacheEntry::password_hash` and
//! `pin_hash` are handled only as opaque strings copied straight into the
//! encrypted-at-rest `app_user` table via `holler_edge_database::repo`. This
//! module never formats, logs, or wraps them in any error — see
//! `tests::password_hash_never_appears_in_error_display` for the proof.

use chrono::Utc;
use holler_edge_database::{model, repo, Db};
use serde::Deserialize;

use crate::client::HttpClient;
use crate::error::SyncResult;

/// No `Debug` derive, deliberately: `users` carries `WireAppUser`, which
/// must never be formatted (ADR-011) — see `WireAppUser`'s own doc comment.
#[derive(Deserialize)]
pub struct ConfigBundle {
    pub config_version: i64,
    pub users: Vec<WireAppUser>,
    #[serde(default)]
    pub roles: Vec<serde_json::Value>,
    pub tables: Vec<WireRestaurantTable>,
    pub categories: Vec<WireMenuCategory>,
    pub items: Vec<WireMenuItem>,
    // Added at 0.4.3 (ADR-017 amendment). Argon2id VERIFIERS for devices
    // enrolled at this outlet — the ADR-011 pattern applied to devices, so a
    // KDS LAN handshake can be verified with the uplink down. `#[serde(
    // default)]` so a bundle from a cloud that has not deployed this field
    // yet still parses (mirrors `roles` above) — `apply_bundle` does not
    // treat an empty `device_credentials` array as an error the way it does
    // an empty `users` array, since a legitimately device-less outlet is not
    // ruled out the way a staff-less one is.
    #[serde(default)]
    pub device_credentials: Vec<WireDeviceCredential>,
    // ---------------------------------------------------------- M4 (T4b) --
    // Added at 0.5.0/ADR-018. `#[serde(default)]` on every field below is
    // the same "absent field ≠ error" posture as `device_credentials`
    // above: an OLDER cloud that has not deployed a given family yet must
    // still parse a bundle, and simply contributes nothing for it this
    // pull. That is distinct from the family's ARRAY being present but
    // EMPTY, which is also not an error — a fresh tenant genuinely has zero
    // recipes — both cases just mean "apply nothing for this family",
    // covered by the same `#[serde(default)]`/empty-`Vec` code path.
    //
    // `day_start_time` is the one exception (a scalar, not an array):
    // `Option<String>` with `#[serde(default)]` distinguishes "not sent /
    // not yet deployed" (`None`, skip apply, leave the existing stored
    // value untouched) from a sent value, which is validated and applied
    // or rejects the whole bundle — see `apply_bundle`.
    #[serde(default)]
    pub day_start_time: Option<String>,
    /// `printer_role` rows since `since_version` — see
    /// `holler_edge_database::repo::upsert_printer_role`'s doc comment for
    /// why this applies as a per-row upsert and not the wholesale-replace
    /// helper of the same table.
    #[serde(default)]
    pub printer_roles: Vec<WirePrinterRole>,
    /// ASSUMPTION (stated per this task's own instruction): the backend
    /// bundle does not carry `menu_item_variants`/`menu_item_modifiers` as
    /// of this crate's HEAD (`backend/cmd/api/syncconfig.go`'s
    /// `syncConfigResponse` has no such fields yet — that half of T4b is a
    /// concurrent, not-yet-landed backend track). These field names are
    /// chosen to match `packages/contracts/go/menu.go`'s `MenuItemVariant`/
    /// `MenuItemModifier` type names in snake_case, the same convention
    /// every other family in this bundle already uses (`tables` for
    /// `RestaurantTable`, `items` for `MenuItem`). `#[serde(default)]` means
    /// this compiles and functions correctly today (an absent field, empty
    /// `Vec`, nothing applied) and needs no further edge-side change once
    /// the backend field lands with this name.
    #[serde(default)]
    pub menu_item_variants: Vec<WireMenuItemVariant>,
    #[serde(default)]
    pub menu_item_modifiers: Vec<WireMenuItemModifier>,
    #[serde(default)]
    pub inventory_items: Vec<WireInventoryItem>,
    #[serde(default)]
    pub item_unit_conversions: Vec<WireItemUnitConversion>,
    #[serde(default)]
    pub recipes: Vec<WireRecipe>,
    #[serde(default)]
    pub recipe_ingredients: Vec<WireRecipeIngredient>,
    #[serde(default)]
    pub modifier_ingredient_deltas: Vec<WireModifierIngredientDelta>,
}

/// Mirrors `MenuItemVariant` (`packages/contracts/go/menu.go`). See
/// `ConfigBundle::menu_item_variants`'s doc comment for the field-name
/// assumption. NOTE: the contract's `MenuItemVariant` does not yet carry
/// `is_default` (ADR-018 §2.1 / migration `0014_menu_default_variant.sql`)
/// even though the SQLite column exists — the Go/TS wire types this struct
/// is required to mirror exactly have not caught up. `is_default` therefore
/// cannot be synced by this crate yet; the column keeps its migration
/// default (`0`) until contracts adds the field. Reported as an open risk.
#[derive(Debug, Deserialize)]
pub struct WireMenuItemVariant {
    pub id: String,
    pub menu_item_id: String,
    pub name: String,
    pub price_delta_paise: i64,
    pub config_version: i64,
}

/// Mirrors `MenuItemModifier` (`packages/contracts/go/menu.go`).
#[derive(Debug, Deserialize)]
pub struct WireMenuItemModifier {
    pub id: String,
    pub menu_item_id: String,
    pub group_name: String,
    pub option_name: String,
    pub price_delta_paise: i64,
    pub min_selection: i64,
    pub max_selection: i64,
    pub config_version: i64,
}

/// Mirrors `PrinterRole` (`packages/contracts/go/printer.go`, 0.4.7).
/// `role` is `"KITCHEN" | "BILL"` — carried as the raw stored string, the
/// same convention `WireMenuItem`'s siblings use for other CHECK-
/// constrained enums.
#[derive(Debug, Deserialize)]
pub struct WirePrinterRole {
    pub printer_id: String,
    pub role: String,
    pub config_version: i64,
}

/// Mirrors `InventoryItem` (`packages/contracts/go/inventory.go`, ADR-018).
#[derive(Debug, Deserialize)]
pub struct WireInventoryItem {
    pub id: String,
    pub outlet_id: String,
    pub sku: String,
    pub name: String,
    #[serde(default)]
    pub category: Option<String>,
    pub dimension: String,
    #[serde(default)]
    pub reorder_level_micro: Option<i64>,
    #[serde(default)]
    pub par_level_micro: Option<i64>,
    #[serde(default)]
    pub storage_location: Option<String>,
    pub is_active: bool,
    pub yield_factor_ppm: i64,
    pub config_version: i64,
}

/// Mirrors `ItemUnitConversion` (`packages/contracts/go/inventory.go`).
#[derive(Debug, Deserialize)]
pub struct WireItemUnitConversion {
    pub id: String,
    pub inventory_item_id: String,
    pub pack_unit_label: String,
    pub source_dimension: String,
    pub numerator: i64,
    pub denominator: i64,
    pub config_version: i64,
}

/// Mirrors `Recipe` (`packages/contracts/go/inventory.go`, 0.5.1 addendum —
/// `output_dimension`/`output_quantity_micro` are NOT NULL on every recipe).
#[derive(Debug, Deserialize)]
pub struct WireRecipe {
    pub id: String,
    pub menu_item_variant_id: String,
    pub name: String,
    pub recipe_version: i64,
    pub output_dimension: String,
    pub output_quantity_micro: i64,
    pub config_version: i64,
}

/// Mirrors `RecipeIngredient` (`packages/contracts/go/inventory.go`).
#[derive(Debug, Deserialize)]
pub struct WireRecipeIngredient {
    pub id: String,
    pub recipe_id: String,
    pub component_kind: String,
    #[serde(default)]
    pub inventory_item_id: Option<String>,
    #[serde(default)]
    pub sub_recipe_id: Option<String>,
    pub quantity_micro: i64,
    pub quantity_dimension: String,
    pub yield_factor_ppm: i64,
    pub sort_order: i64,
    pub config_version: i64,
}

/// Mirrors `ModifierIngredientDelta` (`packages/contracts/go/inventory.go`).
#[derive(Debug, Deserialize)]
pub struct WireModifierIngredientDelta {
    pub id: String,
    pub menu_item_modifier_id: String,
    pub inventory_item_id: String,
    pub quantity_micro: i64,
    pub config_version: i64,
}

/// Mirrors `EdgeDeviceCredential` (openapi.yaml / packages/contracts 0.4.3).
/// Radioactive like `WireAppUser`: `credential_hash` is a verifier, never a
/// bearer token, but gets the exact same containment as `password_hash` —
/// this type deliberately does not derive `Debug` so an accidental `{:?}`
/// log is a compile error.
#[derive(Deserialize)]
pub struct WireDeviceCredential {
    pub credential_id: String,
    pub device_id: String,
    pub tenant_id: String,
    pub outlet_id: String,
    pub credential_hash: String,
    pub device_kind: String,
    #[serde(default)]
    pub revoked_at: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    pub config_version: i64,
}

/// Mirrors `EdgeUserCacheEntry` (openapi.yaml). Radioactive (ADR-011):
/// `password_hash`/`pin_hash` must never be logged or placed in an error.
/// This type deliberately does not derive `Debug` in order to make an
/// accidental `{:?}` log of a whole entry a compile error.
#[derive(Deserialize)]
pub struct WireAppUser {
    pub id: String,
    pub tenant_id: String,
    pub outlet_id: String,
    pub email: String,
    pub full_name: String,
    pub password_hash: String,
    #[serde(default)]
    pub pin_hash: Option<String>,
    pub is_active: bool,
    pub permissions: Vec<String>,
    pub config_version: i64,
}

#[derive(Debug, Deserialize)]
pub struct WireRestaurantTable {
    pub id: String,
    pub outlet_id: String,
    pub section: String,
    pub label: String,
    pub seat_count: i64,
    pub is_active: bool,
    pub config_version: i64,
}

#[derive(Debug, Deserialize)]
pub struct WireMenuCategory {
    pub id: String,
    pub outlet_id: String,
    pub name: String,
    pub sort_order: i64,
    pub config_version: i64,
}

#[derive(Debug, Deserialize)]
pub struct WireMenuItem {
    pub id: String,
    pub outlet_id: String,
    pub category_id: String,
    pub name: String,
    pub base_price_paise: i64,
    pub is_available: bool,
    /// Mirrors `MenuItem.TaxProfileID` (contracts 0.4.2, `packages/contracts/go/menu.go`).
    /// The Go wire struct has no `omitempty` on this field, so the key is
    /// always present (`null` when unset) — deliberately NOT `#[serde(
    /// default)]`, so a bundle that omits the key outright (rather than
    /// sending `null`) fails to parse instead of silently caching `None`
    /// over whatever real value the outlet already had.
    pub tax_profile_id: Option<String>,
    /// Mirrors `MenuItem.HSNSAC` (contracts 0.4.5, `packages/contracts/go/menu.go`).
    /// Same reasoning as `tax_profile_id`: always-present key, no
    /// `#[serde(default)]`.
    pub hsn_sac: Option<String>,
    pub config_version: i64,
}

/// Fetches `/sync/config?outlet_id=..&since_version=..` and applies it if
/// (and only if) it is strictly newer than the locally applied version.
/// Returns `true` if a new bundle was applied, `false` if the cloud (or the
/// version check here) found nothing newer.
///
/// Roles: `EdgeUserCacheEntry.permissions` already carries the flattened,
/// per-outlet permission list (ADR-011 §1), and the frozen edge SQLite
/// schema (`packages/contracts/sqlite/0002_m1_identity_tables.sql`) has no
/// `role` table — so the bundle's `roles` array is read (to keep the wire
/// contract honest) but intentionally not persisted; there is nowhere
/// contracted to put it and nothing at the edge currently reads it.
pub fn pull_and_apply_config(
    db: &mut Db,
    client: &HttpClient,
    outlet_id: &str,
) -> SyncResult<bool> {
    let since_version = repo::get_sync_state(db.connection(), outlet_id)?
        .map(|s| s.last_applied_config_version)
        .unwrap_or(0);

    let bundle: ConfigBundle = client.get_json(&format!(
        "/sync/config?outlet_id={outlet_id}&since_version={since_version}"
    ))?;

    apply_bundle(db, outlet_id, since_version, bundle)
}

/// Applies an already-fetched bundle. Split out from [`pull_and_apply_config`]
/// so tests can exercise "ignore an older/equal version" and "replace at a
/// newer version" without a real HTTP round trip.
pub fn apply_bundle(
    db: &mut Db,
    outlet_id: &str,
    since_version: i64,
    bundle: ConfigBundle,
) -> SyncResult<bool> {
    if bundle.config_version <= since_version {
        // Older or equal: ignored outright, per §50.1 ("replaced, never
        // merged" — an equal-or-older bundle has nothing newer to replace
        // with).
        return Ok(false);
    }

    // ADR-017 "Consequences": an empty `users` array is an error, not an
    // empty set, and this check runs before touching SQLite at all — a
    // suspect bundle applies nothing rather than replacing tables/menu while
    // silently zeroing out login credentials. A legitimately staffless
    // outlet is not a case this backend produces (ListEdgeUserCache reflects
    // enrolled users), so failing loudly here has no legitimate false
    // positive to weigh against the M1-acceptance-threatening silent failure
    // this closes.
    if bundle.users.is_empty() {
        return Err(crate::error::SyncError::EmptyUserCache {
            config_version: bundle.config_version,
        });
    }

    let conn = db.connection();
    exec_batch(conn, "BEGIN")?;

    let result = (|| -> SyncResult<()> {
        for u in &bundle.users {
            let permissions_json = serde_json::to_string(&u.permissions)?;
            repo::replace_app_user(
                conn,
                &model::AppUser {
                    id: u.id.clone(),
                    tenant_id: u.tenant_id.clone(),
                    outlet_id: u.outlet_id.clone(),
                    email: u.email.clone(),
                    full_name: u.full_name.clone(),
                    password_hash: u.password_hash.clone(),
                    pin_hash: u.pin_hash.clone(),
                    is_active: u.is_active,
                    permissions_json,
                    config_version: u.config_version,
                    // EdgeUserCacheEntry (openapi.yaml) carries no
                    // updated_at field though app_user.updated_at is
                    // NOT NULL in the frozen schema; the pull's own wall
                    // clock stands in for it (see this crate's report —
                    // flagged as a contract gap, not invented schema).
                    updated_at: Utc::now().to_rfc3339(),
                },
            )?;
        }
        for t in &bundle.tables {
            repo::upsert_restaurant_table(
                conn,
                &model::RestaurantTable {
                    id: t.id.clone(),
                    outlet_id: t.outlet_id.clone(),
                    section: t.section.clone(),
                    label: t.label.clone(),
                    seat_count: t.seat_count,
                    is_active: t.is_active,
                    config_version: t.config_version,
                },
            )?;
        }
        for c in &bundle.categories {
            repo::upsert_menu_category(
                conn,
                &model::MenuCategory {
                    id: c.id.clone(),
                    outlet_id: c.outlet_id.clone(),
                    name: c.name.clone(),
                    sort_order: c.sort_order,
                    config_version: c.config_version,
                },
            )?;
        }
        for i in &bundle.items {
            repo::upsert_menu_item(
                conn,
                &model::MenuItem {
                    id: i.id.clone(),
                    outlet_id: i.outlet_id.clone(),
                    category_id: i.category_id.clone(),
                    name: i.name.clone(),
                    base_price_paise: i.base_price_paise,
                    is_available: i.is_available,
                    tax_profile_id: i.tax_profile_id.clone(),
                    hsn_sac: i.hsn_sac.clone(),
                    config_version: i.config_version,
                },
            )?;
        }
        // M4 T4b, ASSUMPTION (see `ConfigBundle::menu_item_variants`'s doc
        // comment): field names not yet confirmed against a shipped backend
        // bundle. Applied after `items` (FK: menu_item_id -> menu_item.id)
        // and before `recipes` (FK: recipe.menu_item_variant_id ->
        // menu_item_variant.id).
        for v in &bundle.menu_item_variants {
            repo::upsert_menu_item_variant(
                conn,
                &model::MenuItemVariant {
                    id: v.id.clone(),
                    menu_item_id: v.menu_item_id.clone(),
                    name: v.name.clone(),
                    price_delta_paise: v.price_delta_paise,
                    config_version: v.config_version,
                },
            )?;
        }
        for m in &bundle.menu_item_modifiers {
            repo::upsert_menu_item_modifier(
                conn,
                &model::MenuItemModifier {
                    id: m.id.clone(),
                    menu_item_id: m.menu_item_id.clone(),
                    group_name: m.group_name.clone(),
                    option_name: m.option_name.clone(),
                    price_delta_paise: m.price_delta_paise,
                    min_selection: m.min_selection,
                    max_selection: m.max_selection,
                    config_version: m.config_version,
                },
            )?;
        }
        // ADR-017 amendment (0.4.3): persisted exactly as `users` is, into
        // the encrypted-at-rest device_credential_cache table, so a KDS LAN
        // handshake can be verified with the uplink down. A revoked/expired
        // credential is written as-is (never skipped) — the edge learns a
        // credential is dead by syncing it, not by its absence.
        for c in &bundle.device_credentials {
            repo::replace_device_credential_cache(
                conn,
                &model::DeviceCredentialCache {
                    credential_id: c.credential_id.clone(),
                    device_id: c.device_id.clone(),
                    tenant_id: c.tenant_id.clone(),
                    outlet_id: c.outlet_id.clone(),
                    credential_hash: c.credential_hash.clone(),
                    device_kind: c.device_kind.clone(),
                    revoked_at: c.revoked_at.clone(),
                    expires_at: c.expires_at.clone(),
                    config_version: c.config_version,
                },
            )?;
        }
        // M4 T4b (ADR-018 §9.2). `None` means the field was not sent (an
        // older cloud, or a bundle from before this field existed) — skip
        // applying, leave whatever the outlet row already has (the
        // migration's `DEFAULT '00:00'` on a fresh install). `Some(s)` is
        // VALIDATED before writing anything (`repo::upsert_outlet_day_
        // start_time` calls `DayStartTime::parse` first) — an unparseable
        // value propagates an `Err` out of this closure, which rolls back
        // the WHOLE bundle apply (the outer `if let Err(e) = result`
        // below), never landing a bad value and never silently keeping
        // the old one while claiming success.
        if let Some(day_start_time) = &bundle.day_start_time {
            repo::upsert_outlet_day_start_time(
                conn,
                outlet_id,
                day_start_time,
                bundle.config_version,
            )?;
        }
        // `printer_role` — per-row upsert, not the wholesale-replace helper
        // of the same table; see `repo::upsert_printer_role`'s doc comment
        // for why (the bundle ships a since_version-filtered DELTA of
        // individual rows, not each printer's full current role set).
        // Depends on the referenced `printer` row already existing locally
        // (`printer_role.printer_id REFERENCES printer(id)`, FK enforced) —
        // this crate has no config-apply path for `printer`/`station`
        // itself yet (see this track's report: a pre-existing gap, not
        // introduced here), so a printer_role for an outlet whose printers
        // were never separately seeded will fail this apply with a foreign
        // key error, rolling back the whole bundle rather than landing a
        // dangling row.
        for r in &bundle.printer_roles {
            repo::upsert_printer_role(
                conn,
                &model::PrinterRole {
                    printer_id: r.printer_id.clone(),
                    role: r.role.clone(),
                    config_version: r.config_version,
                },
            )?;
        }
        // M4 inventory config (ADR-018 §1, §10 landing checklist item 3).
        // Applied in FK order: inventory_item -> item_unit_conversion
        // (inventory_item_id), recipe (menu_item_variant_id, applied
        // above) -> recipe_ingredient (recipe_id, inventory_item_id) and,
        // separately, menu_item_modifier (applied above) ->
        // modifier_ingredient_delta (menu_item_modifier_id,
        // inventory_item_id). An empty array here is "this outlet has zero
        // recipes configured", a legitimate value, never an error — the
        // `stock_deduction_gap` mechanism (ADR-018 Rule 2) is what a
        // missing recipe produces at deduction time, not a rejected sync.
        for item in &bundle.inventory_items {
            repo::upsert_inventory_item(
                conn,
                &model::InventoryItem {
                    id: item.id.clone(),
                    outlet_id: item.outlet_id.clone(),
                    sku: item.sku.clone(),
                    name: item.name.clone(),
                    category: item.category.clone(),
                    dimension: item.dimension.clone(),
                    reorder_level_micro: item.reorder_level_micro,
                    par_level_micro: item.par_level_micro,
                    storage_location: item.storage_location.clone(),
                    is_active: item.is_active,
                    yield_factor_ppm: item.yield_factor_ppm,
                    config_version: item.config_version,
                },
            )?;
        }
        for c in &bundle.item_unit_conversions {
            repo::upsert_item_unit_conversion(
                conn,
                &model::ItemUnitConversion {
                    id: c.id.clone(),
                    inventory_item_id: c.inventory_item_id.clone(),
                    pack_unit_label: c.pack_unit_label.clone(),
                    source_dimension: c.source_dimension.clone(),
                    numerator: c.numerator,
                    denominator: c.denominator,
                    config_version: c.config_version,
                },
            )?;
        }
        for r in &bundle.recipes {
            repo::upsert_recipe(
                conn,
                &model::Recipe {
                    id: r.id.clone(),
                    menu_item_variant_id: r.menu_item_variant_id.clone(),
                    name: r.name.clone(),
                    recipe_version: r.recipe_version,
                    output_dimension: r.output_dimension.clone(),
                    output_quantity_micro: r.output_quantity_micro,
                    config_version: r.config_version,
                },
            )?;
        }
        for i in &bundle.recipe_ingredients {
            repo::upsert_recipe_ingredient(
                conn,
                &model::RecipeIngredient {
                    id: i.id.clone(),
                    recipe_id: i.recipe_id.clone(),
                    component_kind: i.component_kind.clone(),
                    inventory_item_id: i.inventory_item_id.clone(),
                    sub_recipe_id: i.sub_recipe_id.clone(),
                    quantity_micro: i.quantity_micro,
                    quantity_dimension: i.quantity_dimension.clone(),
                    yield_factor_ppm: i.yield_factor_ppm,
                    sort_order: i.sort_order,
                    config_version: i.config_version,
                },
            )?;
        }
        for d in &bundle.modifier_ingredient_deltas {
            repo::upsert_modifier_ingredient_delta(
                conn,
                &model::ModifierIngredientDelta {
                    id: d.id.clone(),
                    menu_item_modifier_id: d.menu_item_modifier_id.clone(),
                    inventory_item_id: d.inventory_item_id.clone(),
                    quantity_micro: d.quantity_micro,
                    config_version: d.config_version,
                },
            )?;
        }
        Ok(())
    })();

    if let Err(e) = result {
        exec_batch(conn, "ROLLBACK")?;
        return Err(e);
    }

    let now = Utc::now().to_rfc3339();
    let existing = repo::get_sync_state(conn, outlet_id)?;
    repo::update_sync_cursor(
        conn,
        outlet_id,
        existing
            .as_ref()
            .and_then(|s| s.last_pushed_outbox_id.clone())
            .as_deref(),
        bundle.config_version,
        Some(&now),
        Some(&now),
        true,
    )?;

    exec_batch(conn, "COMMIT")?;
    Ok(true)
}

/// `rusqlite::Connection::execute_batch` returns `rusqlite::Error`, and this
/// crate deliberately does not depend on `rusqlite` directly (all SQLite
/// access is meant to go through `holler_edge_database`'s repositories) —
/// `BEGIN`/`ROLLBACK`/`COMMIT` around the multi-statement bundle apply are
/// the one exception, since `edge/database` exposes no transaction spanning
/// several repo calls made from outside the crate. This wraps the error
/// through `DbError::Sqlite`, whose type is public, without naming
/// `rusqlite::Error` in this crate's own source.
fn exec_batch(conn: &rusqlite::Connection, sql: &str) -> SyncResult<()> {
    conn.execute_batch(sql)
        .map_err(holler_edge_database::DbError::Sqlite)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ADR-011: proves password_hash never leaks through any error surface
    /// this module can produce. `WireAppUser` has no `Debug` impl (a `{:?}`
    /// would be a compile error), and this test additionally checks that a
    /// malformed-bundle JSON error (the one error path that touches
    /// arbitrary bundle content) never contains a hash value even when one
    /// is present in the source JSON.
    #[test]
    fn malformed_bundle_error_never_contains_password_hash() {
        let raw = r#"{"config_version":1,"users":[{"id":"u1","tenant_id":"t1","outlet_id":"o1","email":"a@b.com","full_name":"A","password_hash":"argon2id$super-secret-hash","is_active":true,"permissions":[],"config_version":1}],"roles":[],"tables":[],"categories":[],"items":"not-an-array"}"#;
        let result = serde_json::from_str::<ConfigBundle>(raw);
        assert!(result.is_err(), "malformed items array must fail to parse");
        let msg = result.err().expect("checked is_err above").to_string();
        assert!(!msg.contains("super-secret-hash"));
    }

    /// ADR-017 "Consequences": an empty `users` array on a bundle that would
    /// otherwise apply must be a hard error, and — proving the guard is not
    /// vacuous — nothing else in the bundle gets applied either: the table
    /// carried alongside the empty `users` array must NOT land in SQLite.
    /// Falsified by temporarily deleting the `bundle.users.is_empty()` guard
    /// in `apply_bundle`: with it removed, this test fails because
    /// `apply_bundle` returns `Ok(true)` and the table row is written.
    #[test]
    fn empty_users_array_on_a_newer_bundle_is_rejected_and_nothing_applies() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        repo::upsert_outlet(
            db.connection(),
            &model::Outlet {
                id: "outlet-1".to_string(),
                brand_id: "brand-1".to_string(),
                name: "Test Outlet".to_string(),
                timezone: "Asia/Kolkata".to_string(),
                config_version: 1,
                created_at: "2026-08-07T00:00:00Z".to_string(),
                updated_at: "2026-08-07T00:00:00Z".to_string(),
            },
        )
        .expect("seed outlet");

        let bundle = ConfigBundle {
            config_version: 2,
            users: vec![],
            roles: vec![],
            tables: vec![WireRestaurantTable {
                id: "table-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                section: "Main".to_string(),
                label: "T1".to_string(),
                seat_count: 4,
                is_active: true,
                config_version: 2,
            }],
            categories: vec![],
            items: vec![],
            device_credentials: vec![],
            day_start_time: None,
            printer_roles: vec![],
            menu_item_variants: vec![],
            menu_item_modifiers: vec![],
            inventory_items: vec![],
            item_unit_conversions: vec![],
            recipes: vec![],
            recipe_ingredients: vec![],
            modifier_ingredient_deltas: vec![],
        };

        let err = apply_bundle(&mut db, "outlet-1", 0, bundle).expect_err("empty users must error");
        assert!(matches!(
            err,
            crate::error::SyncError::EmptyUserCache { config_version: 2 }
        ));

        let tables =
            repo::list_restaurant_tables(db.connection(), "outlet-1").expect("list tables");
        assert!(
            tables.is_empty(),
            "no part of a rejected bundle may apply, including the table carried alongside the empty users array"
        );
    }

    /// The point of the whole ADR-017 amendment: a device credential shipped
    /// on `/sync/config` must land in the encrypted-at-rest
    /// `device_credential_cache` table, exactly as `users` lands in
    /// `app_user` — this is what makes the offline LAN handshake possible at
    /// all. Falsifiable by deleting the `for c in &bundle.device_credentials`
    /// loop in `apply_bundle`: with it removed, this test fails because the
    /// lookup below returns `None`.
    #[test]
    fn device_credential_in_bundle_lands_in_local_cache() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        repo::upsert_outlet(
            db.connection(),
            &model::Outlet {
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

        let bundle = ConfigBundle {
            config_version: 2,
            users: vec![WireAppUser {
                id: "u1".to_string(),
                tenant_id: "t1".to_string(),
                outlet_id: "outlet-1".to_string(),
                email: "a@b.com".to_string(),
                full_name: "A".to_string(),
                password_hash: "argon2id$fake".to_string(),
                pin_hash: None,
                is_active: true,
                permissions: vec![],
                config_version: 2,
            }],
            roles: vec![],
            tables: vec![],
            categories: vec![],
            items: vec![],
            device_credentials: vec![WireDeviceCredential {
                credential_id: "cred-1".to_string(),
                device_id: "device-1".to_string(),
                tenant_id: "t1".to_string(),
                outlet_id: "outlet-1".to_string(),
                credential_hash: "argon2id$device-verifier".to_string(),
                device_kind: "KDS".to_string(),
                revoked_at: None,
                expires_at: None,
                config_version: 2,
            }],
            day_start_time: None,
            printer_roles: vec![],
            menu_item_variants: vec![],
            menu_item_modifiers: vec![],
            inventory_items: vec![],
            item_unit_conversions: vec![],
            recipes: vec![],
            recipe_ingredients: vec![],
            modifier_ingredient_deltas: vec![],
        };

        let applied = apply_bundle(&mut db, "outlet-1", 0, bundle).expect("apply must succeed");
        assert!(applied);

        let cached = repo::get_device_credential_cache_by_id(db.connection(), "cred-1")
            .expect("lookup")
            .expect("credential must be cached after config apply");
        assert_eq!(cached.device_id, "device-1");
        assert_eq!(cached.device_kind, "KDS");
        assert_eq!(cached.credential_hash, "argon2id$device-verifier");
    }

    /// The point of the whole fix: a menu item shipped on `/sync/config`
    /// with a real `tax_profile_id` and `hsn_sac` must land in the local
    /// menu item cache with BOTH values intact, not silently dropped to
    /// `None`. `edge/database`'s `invoice::assemble::build_invoice` reads
    /// these at billing time (see `model::MenuItem`'s doc comments), so a
    /// `None` written here where the cloud sent a real code means the edge
    /// bills without it.
    ///
    /// Falsifiable: hardcode `tax_profile_id: None` (or `hsn_sac: None`) in
    /// the `model::MenuItem` literal inside `apply_bundle`'s items loop,
    /// leaving the wire value unused — this test goes red because the
    /// cached row no longer matches what was sent. Restoring
    /// `i.tax_profile_id.clone()` / `i.hsn_sac.clone()` turns it green
    /// again.
    #[test]
    fn menu_item_tax_profile_id_and_hsn_sac_survive_the_config_cache() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        repo::upsert_outlet(
            db.connection(),
            &model::Outlet {
                id: "outlet-1".to_string(),
                brand_id: "brand-1".to_string(),
                name: "Test Outlet".to_string(),
                timezone: "Asia/Kolkata".to_string(),
                config_version: 1,
                created_at: "2026-08-15T00:00:00Z".to_string(),
                updated_at: "2026-08-15T00:00:00Z".to_string(),
            },
        )
        .expect("seed outlet");
        repo::upsert_menu_category(
            db.connection(),
            &model::MenuCategory {
                id: "cat-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                name: "Starters".to_string(),
                sort_order: 1,
                config_version: 1,
            },
        )
        .expect("seed category");
        repo::upsert_tax_profile(
            db.connection(),
            &model::TaxProfile {
                id: "tax-profile-5pct".to_string(),
                outlet_id: "outlet-1".to_string(),
                code: "GST5".to_string(),
                name: "GST 5%".to_string(),
                pricing_mode: "INCLUSIVE".to_string(),
                is_default: true,
                is_active: true,
                config_version: 1,
            },
        )
        .expect("seed tax profile");

        let raw = r#"{
            "config_version": 2,
            "users": [{
                "id": "u1", "tenant_id": "t1", "outlet_id": "outlet-1",
                "email": "a@b.com", "full_name": "A",
                "password_hash": "argon2id$fake", "is_active": true,
                "permissions": [], "config_version": 2
            }],
            "roles": [],
            "tables": [],
            "categories": [],
            "items": [{
                "id": "item-1", "outlet_id": "outlet-1",
                "category_id": "cat-1", "name": "Paneer Tikka",
                "base_price_paise": 25000, "is_available": true,
                "tax_profile_id": "tax-profile-5pct",
                "hsn_sac": "9963",
                "config_version": 2
            }],
            "device_credentials": []
        }"#;
        let bundle: ConfigBundle = serde_json::from_str(raw).expect("parse bundle");

        let applied = apply_bundle(&mut db, "outlet-1", 0, bundle).expect("apply must succeed");
        assert!(applied);

        let items =
            repo::list_menu_items_for_outlet(db.connection(), "outlet-1").expect("list items");
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].tax_profile_id.as_deref(),
            Some("tax-profile-5pct"),
            "tax_profile_id sent by the cloud must survive into the cache, not be defaulted to None"
        );
        assert_eq!(
            items[0].hsn_sac.as_deref(),
            Some("9963"),
            "hsn_sac sent by the cloud must survive into the cache, not be defaulted to None"
        );
    }

    // ---------------------------------------------------------- M4 (T4b) --

    fn seed_outlet(db: &mut Db, outlet_id: &str) {
        repo::upsert_outlet(
            db.connection(),
            &model::Outlet {
                id: outlet_id.to_string(),
                brand_id: "brand-1".to_string(),
                name: "Test Outlet".to_string(),
                timezone: "Asia/Kolkata".to_string(),
                config_version: 1,
                created_at: "2026-08-21T00:00:00Z".to_string(),
                updated_at: "2026-08-21T00:00:00Z".to_string(),
            },
        )
        .expect("seed outlet");
    }

    /// A bundle with a non-empty `users` array (so it clears the ADR-017
    /// empty-users guard) and every M4 field empty/`None` — the baseline
    /// individual tests customize.
    fn empty_bundle(config_version: i64) -> ConfigBundle {
        ConfigBundle {
            config_version,
            users: vec![WireAppUser {
                id: "u1".to_string(),
                tenant_id: "t1".to_string(),
                outlet_id: "outlet-1".to_string(),
                email: "a@b.com".to_string(),
                full_name: "A".to_string(),
                password_hash: "argon2id$fake".to_string(),
                pin_hash: None,
                is_active: true,
                permissions: vec![],
                config_version,
            }],
            roles: vec![],
            tables: vec![],
            categories: vec![],
            items: vec![],
            device_credentials: vec![],
            day_start_time: None,
            printer_roles: vec![],
            menu_item_variants: vec![],
            menu_item_modifiers: vec![],
            inventory_items: vec![],
            item_unit_conversions: vec![],
            recipes: vec![],
            recipe_ingredients: vec![],
            modifier_ingredient_deltas: vec![],
        }
    }

    /// The write path `DayStartTime`'s own doc comment named as missing:
    /// a valid `day_start_time` in the bundle must land on the outlet row.
    #[test]
    fn day_start_time_applies_to_the_outlet_row() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(&mut db, "outlet-1");

        let mut bundle = empty_bundle(2);
        bundle.day_start_time = Some("04:00".to_string());

        let applied = apply_bundle(&mut db, "outlet-1", 0, bundle).expect("apply must succeed");
        assert!(applied);

        let stored = repo::get_outlet_day_start_time(db.connection(), "outlet-1")
            .expect("read day_start_time")
            .expect("outlet row must exist after seeding");
        assert_eq!(stored, "04:00");
    }

    /// FALSIFICATION target: an unparseable `day_start_time` must reject the
    /// WHOLE bundle apply, not just that one field — and the previous value
    /// (the migration's `DEFAULT '00:00'`, here) must survive untouched.
    /// Falsifiable by removing the `DayStartTime::parse` call from
    /// `repo::upsert_outlet_day_start_time`: with it removed, this test
    /// fails because `apply_bundle` returns `Ok(true)` and `garbage` lands
    /// in the column.
    #[test]
    fn an_unparseable_day_start_time_rejects_the_whole_apply_and_the_previous_value_survives() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(&mut db, "outlet-1");

        let mut bundle = empty_bundle(2);
        bundle.day_start_time = Some("garbage".to_string());
        // Carried alongside the bad value, same proof shape as the
        // empty-users test above: nothing in a rejected bundle may apply.
        bundle.tables = vec![WireRestaurantTable {
            id: "table-1".to_string(),
            outlet_id: "outlet-1".to_string(),
            section: "Main".to_string(),
            label: "T1".to_string(),
            seat_count: 4,
            is_active: true,
            config_version: 2,
        }];

        let err = apply_bundle(&mut db, "outlet-1", 0, bundle)
            .expect_err("an unparseable day_start_time must reject the whole apply");
        assert!(
            matches!(
                err,
                crate::error::SyncError::Db(holler_edge_database::DbError::InvalidInput(_))
            ),
            "must propagate as a typed DbError::InvalidInput, not a silent substitution: {err:?}"
        );

        let stored = repo::get_outlet_day_start_time(db.connection(), "outlet-1")
            .expect("read day_start_time")
            .expect("outlet row must still exist");
        assert_eq!(
            stored, "00:00",
            "the previous valid value must survive a rejected apply"
        );

        let tables = repo::list_restaurant_tables(db.connection(), "outlet-1")
            .expect("list tables");
        assert!(
            tables.is_empty(),
            "no part of a rejected bundle may apply, including the table carried \
             alongside the bad day_start_time"
        );
    }

    /// `printer_role` lands via the per-row upsert
    /// (`repo::upsert_printer_role`), never the wholesale-replace helper —
    /// see that function's own doc comment for why. Requires the referenced
    /// `printer` row to exist locally first (FK), seeded directly here since
    /// this crate has no config-apply path for `printer` itself yet (a
    /// pre-existing gap reported separately, not introduced by this test).
    #[test]
    fn printer_role_applies_via_per_row_upsert() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(&mut db, "outlet-1");
        repo::upsert_printer(
            db.connection(),
            &model::Printer {
                id: "printer-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                name: "Bill Printer".to_string(),
                connection_kind: "ESCPOS_NETWORK".to_string(),
                address: "192.168.1.50:9100".to_string(),
                paper_width_mm: 80,
                is_active: true,
                config_version: 1,
            },
        )
        .expect("seed printer");

        let mut bundle = empty_bundle(2);
        bundle.printer_roles = vec![WirePrinterRole {
            printer_id: "printer-1".to_string(),
            role: "BILL".to_string(),
            config_version: 2,
        }];

        let applied = apply_bundle(&mut db, "outlet-1", 0, bundle).expect("apply must succeed");
        assert!(applied);

        let roles = repo::list_printer_roles(db.connection(), "printer-1").expect("list roles");
        assert_eq!(roles.len(), 1, "exactly one role row must exist");
        assert_eq!(roles[0].role, "BILL");
    }

    /// The full M4 inventory config chain — `inventory_item` ->
    /// `item_unit_conversion`, `menu_item_variant` -> `recipe` ->
    /// `recipe_ingredient`, `menu_item_modifier` ->
    /// `modifier_ingredient_delta` — applies in one bundle, in FK order,
    /// inside one transaction. Uses the typed micro-unit constructors
    /// (`grams`, `millilitres`, `pieces`), never a raw micro literal.
    #[test]
    fn inventory_config_chain_applies_wholesale_in_fk_order() {
        use holler_edge_database::inventory::{grams, pieces};

        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(&mut db, "outlet-1");

        let mut bundle = empty_bundle(2);
        bundle.categories = vec![WireMenuCategory {
            id: "cat-1".to_string(),
            outlet_id: "outlet-1".to_string(),
            name: "Mains".to_string(),
            sort_order: 1,
            config_version: 2,
        }];
        bundle.items = vec![WireMenuItem {
            id: "item-1".to_string(),
            outlet_id: "outlet-1".to_string(),
            category_id: "cat-1".to_string(),
            name: "Butter Chicken".to_string(),
            base_price_paise: 32000,
            is_available: true,
            tax_profile_id: None,
            hsn_sac: Some("9963".to_string()),
            config_version: 2,
        }];
        bundle.menu_item_variants = vec![WireMenuItemVariant {
            id: "variant-1".to_string(),
            menu_item_id: "item-1".to_string(),
            name: "Regular".to_string(),
            price_delta_paise: 0,
            config_version: 2,
        }];
        bundle.menu_item_modifiers = vec![WireMenuItemModifier {
            id: "modifier-1".to_string(),
            menu_item_id: "item-1".to_string(),
            group_name: "Extras".to_string(),
            option_name: "Extra Paneer".to_string(),
            price_delta_paise: 5000,
            min_selection: 0,
            max_selection: 1,
            config_version: 2,
        }];
        bundle.inventory_items = vec![
            WireInventoryItem {
                id: "inv-chicken".to_string(),
                outlet_id: "outlet-1".to_string(),
                sku: "CHK-001".to_string(),
                name: "Chicken".to_string(),
                category: None,
                dimension: "MASS".to_string(),
                reorder_level_micro: None,
                par_level_micro: None,
                storage_location: None,
                is_active: true,
                yield_factor_ppm: 1_000_000,
                config_version: 2,
            },
            WireInventoryItem {
                id: "inv-paneer".to_string(),
                outlet_id: "outlet-1".to_string(),
                sku: "PNR-001".to_string(),
                name: "Paneer".to_string(),
                category: None,
                dimension: "MASS".to_string(),
                reorder_level_micro: None,
                par_level_micro: None,
                storage_location: None,
                is_active: true,
                yield_factor_ppm: 1_000_000,
                config_version: 2,
            },
        ];
        bundle.item_unit_conversions = vec![WireItemUnitConversion {
            id: "conv-1".to_string(),
            inventory_item_id: "inv-chicken".to_string(),
            pack_unit_label: "tray".to_string(),
            source_dimension: "MASS".to_string(),
            numerator: grams(5_000),
            denominator: pieces(1),
            config_version: 2,
        }];
        bundle.recipes = vec![WireRecipe {
            id: "recipe-1".to_string(),
            menu_item_variant_id: "variant-1".to_string(),
            name: "Butter Chicken".to_string(),
            recipe_version: 1,
            output_dimension: "MASS".to_string(),
            output_quantity_micro: grams(300),
            config_version: 2,
        }];
        bundle.recipe_ingredients = vec![WireRecipeIngredient {
            id: "ri-1".to_string(),
            recipe_id: "recipe-1".to_string(),
            component_kind: "ITEM".to_string(),
            inventory_item_id: Some("inv-chicken".to_string()),
            sub_recipe_id: None,
            quantity_micro: grams(220),
            quantity_dimension: "MASS".to_string(),
            yield_factor_ppm: 1_000_000,
            sort_order: 1,
            config_version: 2,
        }];
        bundle.modifier_ingredient_deltas = vec![WireModifierIngredientDelta {
            id: "mid-1".to_string(),
            menu_item_modifier_id: "modifier-1".to_string(),
            inventory_item_id: "inv-paneer".to_string(),
            quantity_micro: grams(50),
            config_version: 2,
        }];

        let applied = apply_bundle(&mut db, "outlet-1", 0, bundle).expect("apply must succeed");
        assert!(applied);

        let item = repo::get_inventory_item(db.connection(), "inv-chicken")
            .expect("lookup")
            .expect("inventory_item must exist after apply");
        assert_eq!(item.name, "Chicken");
        assert_eq!(item.dimension, "MASS");

        let conversion = repo::get_item_unit_conversion(db.connection(), "conv-1")
            .expect("lookup")
            .expect("item_unit_conversion must exist after apply");
        assert_eq!(conversion.numerator, grams(5_000));

        let recipe = repo::get_recipe(db.connection(), "recipe-1")
            .expect("lookup")
            .expect("recipe must exist after apply");
        assert_eq!(recipe.menu_item_variant_id, "variant-1");
        assert_eq!(recipe.output_quantity_micro, grams(300));

        let ingredient = repo::get_recipe_ingredient(db.connection(), "ri-1")
            .expect("lookup")
            .expect("recipe_ingredient must exist after apply");
        assert_eq!(ingredient.quantity_micro, grams(220));

        let delta = repo::get_modifier_ingredient_delta(db.connection(), "mid-1")
            .expect("lookup")
            .expect("modifier_ingredient_delta must exist after apply");
        assert_eq!(delta.quantity_micro, grams(50));
    }

    /// Re-applying the identical bundle (same rows, same `since_version`
    /// passed explicitly rather than advanced from the cursor, so the
    /// re-apply is not short-circuited by the `config_version <=
    /// since_version` guard) must be a no-op on the stored state: every
    /// upsert here is `ON CONFLICT ... WHERE excluded.config_version >=
    /// existing.config_version`, so a second identical apply overwrites
    /// each row with itself rather than erroring or duplicating.
    #[test]
    fn reapplying_the_same_bundle_version_is_idempotent() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(&mut db, "outlet-1");
        repo::upsert_printer(
            db.connection(),
            &model::Printer {
                id: "printer-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                name: "Bill Printer".to_string(),
                connection_kind: "ESCPOS_NETWORK".to_string(),
                address: "192.168.1.50:9100".to_string(),
                paper_width_mm: 80,
                is_active: true,
                config_version: 1,
            },
        )
        .expect("seed printer");

        let build = || {
            let mut bundle = empty_bundle(2);
            bundle.day_start_time = Some("04:00".to_string());
            bundle.printer_roles = vec![WirePrinterRole {
                printer_id: "printer-1".to_string(),
                role: "BILL".to_string(),
                config_version: 2,
            }];
            bundle.inventory_items = vec![WireInventoryItem {
                id: "inv-chicken".to_string(),
                outlet_id: "outlet-1".to_string(),
                sku: "CHK-001".to_string(),
                name: "Chicken".to_string(),
                category: None,
                dimension: "MASS".to_string(),
                reorder_level_micro: None,
                par_level_micro: None,
                storage_location: None,
                is_active: true,
                yield_factor_ppm: 1_000_000,
                config_version: 2,
            }];
            bundle
        };

        apply_bundle(&mut db, "outlet-1", 0, build()).expect("first apply must succeed");
        apply_bundle(&mut db, "outlet-1", 0, build()).expect("second identical apply must succeed");

        let stored_day_start = repo::get_outlet_day_start_time(db.connection(), "outlet-1")
            .expect("read day_start_time")
            .expect("outlet row must exist");
        assert_eq!(stored_day_start, "04:00");

        let roles = repo::list_printer_roles(db.connection(), "printer-1").expect("list roles");
        assert_eq!(roles.len(), 1, "re-applying must not duplicate the role row");

        let item = repo::get_inventory_item(db.connection(), "inv-chicken")
            .expect("lookup")
            .expect("inventory_item must exist after re-apply");
        assert_eq!(item.name, "Chicken");
        assert_eq!(item.reorder_level_micro, None);
    }
}
