//! Applies the frozen contract schema files verbatim, in order.
//!
//! `packages/contracts/sqlite/0001_init.sql`, `0002_m1_identity_tables.sql`,
//! `0003_order_item_modifiers.sql` and `0004_order_canonical_fields.sql` are
//! read-only and authoritative (ADR-008, ADR-011). This module never edits,
//! reorders, or adds to their statements
//! — it only decides *whether* to run each file exactly once.
//!
//! Idempotency is tracked with SQLite's built-in `PRAGMA user_version`
//! rather than an extra bookkeeping table: this crate must not add a table
//! that is not in the frozen contracts, and `user_version` is a pragma, not
//! a schema object. `user_version == N` means migrations 1..=N have been
//! applied.

use rusqlite::Connection;

use crate::error::{DbError, DbResult};

/// The contract migration files, embedded at compile time so the crate has
/// no runtime dependency on the location of `packages/contracts` on disk.
/// Content is copied verbatim from the frozen files — do not hand-edit.
const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_init.sql",
        include_str!("../../../packages/contracts/sqlite/0001_init.sql"),
    ),
    (
        "0002_m1_identity_tables.sql",
        include_str!("../../../packages/contracts/sqlite/0002_m1_identity_tables.sql"),
    ),
    (
        "0003_order_item_modifiers.sql",
        include_str!("../../../packages/contracts/sqlite/0003_order_item_modifiers.sql"),
    ),
    (
        "0004_order_canonical_fields.sql",
        include_str!("../../../packages/contracts/sqlite/0004_order_canonical_fields.sql"),
    ),
];

/// Applies any migrations not yet reflected in `PRAGMA user_version`. Safe
/// to call on every startup (idempotent): a database already at the latest
/// version is a no-op.
pub fn apply_all(conn: &Connection) -> DbResult<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let current = usize::try_from(current)
        .map_err(|_| DbError::Migration("negative user_version".to_string()))?;

    if current > MIGRATIONS.len() {
        return Err(DbError::Migration(format!(
            "database user_version {current} is ahead of the {} migrations this crate knows about",
            MIGRATIONS.len()
        )));
    }

    for (name, sql) in MIGRATIONS.iter().skip(current) {
        conn.execute_batch(sql)
            .map_err(|e| DbError::Migration(format!("applying {name}: {e}")))?;
        let applied = MIGRATIONS
            .iter()
            .position(|(n, _)| n == name)
            .expect("name is from MIGRATIONS")
            + 1;
        conn.pragma_update(None, "user_version", applied as i64)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pragma::configure_connection;

    #[test]
    fn migrations_are_idempotent() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");

        apply_all(&conn).expect("first apply");
        let version_after_first: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version_after_first, MIGRATIONS.len() as i64);

        // Re-running must not error (e.g. "table already exists") and must
        // leave the version unchanged.
        apply_all(&conn).expect("second apply is a no-op");
        let version_after_second: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version_after_second, version_after_first);
    }

    #[test]
    fn reserved_word_order_table_is_created_and_queryable() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");
        apply_all(&conn).expect("apply");

        // "order" is a reserved word; the migration quotes it and so must
        // every caller. This proves the table exists under that exact name.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM \"order\"", [], |row| row.get(0))
            .expect("querying quoted order table");
        assert_eq!(count, 0);
    }

    #[test]
    fn order_table_has_the_0004_canonical_columns() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");
        apply_all(&conn).expect("apply");

        let mut stmt = conn.prepare("PRAGMA table_info(\"order\")").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        // The rename: tax_paise no longer exists, taxes_paise does.
        assert!(
            !columns.iter().any(|c| c == "tax_paise"),
            "tax_paise must be renamed away, not left alongside taxes_paise"
        );
        for expected in [
            "taxes_paise",
            "source",
            "external_order_id",
            "payment_status",
            "payment_source",
            "confirmed_at",
            "source_payload_json",
            "schema_version",
        ] {
            assert!(
                columns.iter().any(|c| c == expected),
                "expected column {expected} on \"order\" after 0004"
            );
        }
    }

    #[test]
    fn all_milestone_1_tables_exist_after_migration() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");
        apply_all(&conn).expect("apply");

        for table in [
            "outlet",
            "device",
            "menu_category",
            "menu_item",
            "menu_item_variant",
            "menu_item_modifier",
            "order_item",
            "order_item_modifier",
            "kot",
            "local_outbox",
            "sync_state",
            "app_user",
            "restaurant_table",
            "table_session",
            "audit_event",
        ] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "expected table {table} to exist");
        }
    }
}
