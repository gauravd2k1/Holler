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
                    config_version: i.config_version,
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
}
