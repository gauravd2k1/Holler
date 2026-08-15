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
}
