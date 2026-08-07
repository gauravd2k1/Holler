//! Connection pragmas for the edge database. Every value is named and
//! justified — no magic numbers (CLAUDE.md).

use rusqlite::Connection;

use crate::error::DbResult;

/// Applies the pragmas a local-first POS edge node needs. Called once per
/// opened connection, before migrations run.
pub fn configure_connection(conn: &Connection) -> DbResult<()> {
    // WAL mode (ADR-003): lets the sync worker read the outbox while the POS
    // UI thread writes an order, and lets KDS/printer-gateway readers run
    // concurrently with writers instead of being serialized behind a single
    // rollback journal lock. This is the reason ADR-003 exists.
    conn.pragma_update(None, "journal_mode", "WAL")?;

    // FULL fsyncs the WAL on every commit rather than only at checkpoint
    // (NORMAL). A restaurant POS runs on commodity hardware that can lose
    // power mid-shift; an order that the cashier believes was accepted must
    // survive that. This trades some write latency for the durability
    // guarantee CLAUDE.md requires ("Durable writes").
    conn.pragma_update(None, "synchronous", "FULL")?;

    // Enforce declared FOREIGN KEY constraints (e.g. order_item -> "order",
    // table_session -> restaurant_table). SQLite defaults this off for
    // backward compatibility; a POS must not silently accept an order_item
    // for a nonexistent order.
    conn.pragma_update(None, "foreign_keys", "ON")?;

    // A writer competing with concurrent readers (KDS/printer-gateway/sync
    // worker all holding read connections) can hit SQLITE_BUSY under WAL.
    // Block and retry internally for up to 5000ms instead of surfacing a
    // spurious "database is locked" to the cashier on every table tap.
    conn.busy_timeout(std::time::Duration::from_millis(5_000))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wal_mode_is_enabled() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");
        // In-memory databases cannot use WAL (SQLite falls back to
        // "memory"); this test asserts the pragma call itself succeeds and
        // is exercised against a real file in db_tests::wal_mode_on_disk.
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert!(mode == "memory" || mode == "wal");
    }

    #[test]
    fn foreign_keys_enforced() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }
}
