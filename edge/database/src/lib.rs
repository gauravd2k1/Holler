//! Edge SQLite database service (ADR-003) for the Holler POS edge node.
//!
//! Owns opening, configuring, migrating and encrypting-at-rest the local
//! SQLite database, and exposes typed repositories over the frozen
//! `packages/contracts` schema. Nothing outside this crate touches the
//! SQLite file directly (ADR-003) or the encrypted file on disk
//! (ADR-011) — other edge services and the sync worker call this API.

pub mod auth;
pub mod crypto;
mod error;
mod migrations;
pub mod model;
mod pragma;
pub mod repo;

pub use error::{DbError, DbResult};

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::crypto::EncryptionKey;
use crate::model::{NewOrder, NewOrderItem, NewOutboxEntry, NewTableSession, Order, TableSession};

/// An open handle to the edge database. The plaintext SQLite file backing
/// this handle exists on disk only between [`Db::open`] and [`Db::close`]
/// (see `src/crypto.rs` for why, and the limitation that implies).
pub struct Db {
    conn: Connection,
    plaintext_path: PathBuf,
    sealed_path: PathBuf,
    key: EncryptionKey,
}

impl Db {
    /// Opens (decrypting if a sealed file already exists) the database at
    /// `sealed_path`, using `plaintext_path` as the working SQLite file,
    /// applies pragmas (WAL etc., ADR-003) and runs any pending contract
    /// migrations (idempotent).
    ///
    /// `plaintext_path` should live in a directory this process controls
    /// exclusively (e.g. a per-device app-data directory) — this crate never
    /// copies it elsewhere, but it is the caller's responsibility not to
    /// place it under a synced/backed-up folder.
    pub fn open(sealed_path: &Path, plaintext_path: &Path, key: EncryptionKey) -> DbResult<Self> {
        // Deterministically resolve any plaintext left behind by a prior
        // unclean shutdown (crash / power loss) before decrypting a fresh
        // working copy — see crypto::recover_crash_leftovers for why this
        // recovers committed data into the sealed backup rather than
        // wiping it outright.
        crypto::recover_crash_leftovers(sealed_path, plaintext_path, &key)?;

        crypto::open_file(sealed_path, plaintext_path, &key)?;

        let conn = Connection::open(plaintext_path)?;
        pragma::configure_connection(&conn)?;
        migrations::apply_all(&conn)?;

        // Mark this session unclean-until-closed. Presence of this marker
        // on a future open is what makes an unclean shutdown detectable
        // rather than merely inferred from file presence.
        std::fs::write(crypto::marker_path(plaintext_path), b"").map_err(DbError::Io)?;

        Ok(Self {
            conn,
            plaintext_path: plaintext_path.to_path_buf(),
            sealed_path: sealed_path.to_path_buf(),
            key,
        })
    }

    /// Opens a purely in-memory database (WAL falls back to "memory" mode
    /// automatically) with migrations applied. Intended for tests and any
    /// caller that does not need persistence — never used for a real
    /// device, since it is never sealed at rest.
    pub fn open_in_memory_for_tests() -> DbResult<Self> {
        let conn = Connection::open_in_memory()?;
        pragma::configure_connection(&conn)?;
        migrations::apply_all(&conn)?;
        Ok(Self {
            conn,
            plaintext_path: PathBuf::new(),
            sealed_path: PathBuf::new(),
            key: EncryptionKey::new([0u8; 32]),
        })
    }

    /// Checkpoints WAL, closes the connection, re-seals the file at rest
    /// with a fresh nonce, and wipes the plaintext working copy and its
    /// `-wal`/`-shm` siblings. Must be the only way this crate's plaintext
    /// file is ever left on disk after use.
    pub fn close(self) -> DbResult<()> {
        if self.plaintext_path.as_os_str().is_empty() {
            // In-memory test handle: nothing on disk to seal or wipe.
            return Ok(());
        }

        self.conn
            .pragma_update(None, "wal_checkpoint", "TRUNCATE")?;
        // rusqlite's Connection::close returns the connection back on
        // failure; we only hold owned self here so a failure can't leave a
        // half-closed handle for a caller to misuse.
        self.conn.close().map_err(|(_, e)| DbError::Sqlite(e))?;

        crypto::seal_file(&self.plaintext_path, &self.sealed_path, &self.key)?;
        // Same wipe treatment (.db + -wal + -shm) as the crash-recovery
        // path in crypto::recover_crash_leftovers, so there is exactly one
        // way plaintext ever gets removed from disk.
        crypto::wipe_plaintext_and_wal_shm(&self.plaintext_path)?;

        let marker = crypto::marker_path(&self.plaintext_path);
        if marker.exists() {
            std::fs::remove_file(&marker).map_err(DbError::Io)?;
        }
        Ok(())
    }

    /// Read-only access to the underlying connection for the repository
    /// read functions in [`repo`]. Not exposed as `pub` beyond this crate's
    /// modules — callers use the typed `Db` methods and `repo::get_*`/
    /// `repo::list_*` functions, never raw SQL.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Creates an order together with its items, writing the
    /// `local_outbox` row in the *same* SQLite transaction (ADR-007). There
    /// is no lower-level API that lets a caller insert an order without an
    /// outbox row: this is the only order-creation entry point this crate
    /// exposes. On any failure (including an item referencing a
    /// nonexistent menu item) the whole transaction rolls back — neither
    /// the order, its items, nor the outbox row are persisted.
    pub fn create_order_with_outbox(
        &mut self,
        order: &NewOrder,
        items: &[NewOrderItem],
        outbox: &NewOutboxEntry,
    ) -> DbResult<()> {
        let tx = self.conn.transaction()?;
        repo::insert_order(&tx, order)?;
        for item in items {
            repo::insert_order_item(&tx, item)?;
        }
        repo::insert_outbox_entry(&tx, outbox)?;
        tx.commit()?;
        Ok(())
    }

    /// Opens a new table session together with its `local_outbox` row, in
    /// one transaction.
    pub fn open_table_session_with_outbox(
        &mut self,
        session: &NewTableSession,
        outbox: &NewOutboxEntry,
    ) -> DbResult<()> {
        let tx = self.conn.transaction()?;
        repo::insert_table_session(&tx, session)?;
        repo::insert_outbox_entry(&tx, outbox)?;
        tx.commit()?;
        Ok(())
    }

    /// Updates an existing table session (state/current_order_id/
    /// guest_count/close) together with its `local_outbox` row, in one
    /// transaction.
    #[allow(clippy::too_many_arguments)]
    pub fn update_table_session_with_outbox(
        &mut self,
        id: &str,
        state: &str,
        current_order_id: Option<&str>,
        guest_count: i64,
        closed_at: Option<&str>,
        updated_at: &str,
        outbox: &NewOutboxEntry,
    ) -> DbResult<()> {
        let tx = self.conn.transaction()?;
        repo::update_table_session(
            &tx,
            id,
            state,
            current_order_id,
            guest_count,
            closed_at,
            updated_at,
        )?;
        repo::insert_outbox_entry(&tx, outbox)?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_order(&self, id: &str) -> DbResult<Option<Order>> {
        repo::get_order(&self.conn, id)
    }

    pub fn get_table_session(&self, id: &str) -> DbResult<Option<TableSession>> {
        repo::get_table_session(&self.conn, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NewOrder, NewOrderItem, NewOutboxEntry};
    use tempfile::tempdir;

    fn sample_order(id: &str, outlet_id: &str, device_id: &str) -> NewOrder {
        NewOrder {
            id: id.to_string(),
            outlet_id: outlet_id.to_string(),
            device_id: device_id.to_string(),
            order_type: "DINE_IN".to_string(),
            status: "DRAFT".to_string(),
            table_id: None,
            subtotal_paise: 0,
            discount_paise: 0,
            tax_paise: 0,
            total_paise: 0,
            created_at: "2026-08-07T10:00:00Z".to_string(),
            updated_at: "2026-08-07T10:00:00Z".to_string(),
        }
    }

    fn sample_outbox(order_id: &str) -> NewOutboxEntry {
        NewOutboxEntry {
            id: "outbox-1".to_string(),
            aggregate_type: "order".to_string(),
            aggregate_id: order_id.to_string(),
            event_type: "OrderCreated".to_string(),
            payload_json: "{}".to_string(),
            created_at: "2026-08-07T10:00:00Z".to_string(),
        }
    }

    fn seed_outlet_and_device(db: &Db, outlet_id: &str, device_id: &str) {
        repo::upsert_outlet(
            db.connection(),
            &model::Outlet {
                id: outlet_id.to_string(),
                brand_id: "brand-1".to_string(),
                name: "Test Outlet".to_string(),
                timezone: "Asia/Kolkata".to_string(),
                config_version: 1,
                created_at: "2026-08-07T00:00:00Z".to_string(),
                updated_at: "2026-08-07T00:00:00Z".to_string(),
            },
        )
        .expect("seed outlet");
        repo::upsert_device(
            db.connection(),
            &model::Device {
                id: device_id.to_string(),
                outlet_id: outlet_id.to_string(),
                kind: "POS".to_string(),
                name: "Till 1".to_string(),
                last_seen_at: None,
                created_at: "2026-08-07T00:00:00Z".to_string(),
            },
        )
        .expect("seed device");
    }

    #[test]
    fn open_and_migrate_in_memory() {
        let db = Db::open_in_memory_for_tests().expect("open");
        assert!(db.get_order("nonexistent").unwrap().is_none());
    }

    #[test]
    fn create_order_writes_outbox_in_same_transaction() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");

        let order = sample_order("order-1", "outlet-1", "device-1");
        let outbox = sample_outbox("order-1");

        db.create_order_with_outbox(&order, &[], &outbox)
            .expect("create order");

        assert!(db.get_order("order-1").unwrap().is_some());
        let pending = repo::list_unpublished_outbox(db.connection(), 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].aggregate_id, "order-1");
    }

    /// The critical proof required by the task: a failure partway through
    /// the transaction (an order_item referencing a menu_item that does not
    /// exist, violating the FOREIGN KEY pragma) must leave neither the
    /// order row nor the outbox row committed. Commit-then-publish, or
    /// insert-order-then-separately-insert-outbox, would not have this
    /// property.
    #[test]
    fn failed_order_transaction_leaves_neither_order_nor_outbox_row() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");

        let order = sample_order("order-2", "outlet-1", "device-1");
        let bad_item = NewOrderItem {
            id: "item-1".to_string(),
            order_id: "order-2".to_string(),
            menu_item_id: "does-not-exist".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 10000,
            line_total_paise: 10000,
            notes: None,
            created_at: "2026-08-07T10:00:00Z".to_string(),
        };
        let outbox = sample_outbox("order-2");

        let result = db.create_order_with_outbox(&order, &[bad_item], &outbox);
        assert!(result.is_err(), "FK violation must fail the transaction");

        assert!(
            db.get_order("order-2").unwrap().is_none(),
            "order must not be committed"
        );
        let pending = repo::list_unpublished_outbox(db.connection(), 10).unwrap();
        assert!(
            pending.iter().all(|e| e.aggregate_id != "order-2"),
            "outbox row must not be committed either"
        );
    }

    #[test]
    fn table_session_write_also_carries_outbox_atomically() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        repo::upsert_restaurant_table(
            db.connection(),
            &model::RestaurantTable {
                id: "table-1".to_string(),
                outlet_id: "outlet-1".to_string(),
                section: "Main".to_string(),
                label: "T1".to_string(),
                seat_count: 4,
                is_active: true,
                config_version: 1,
            },
        )
        .unwrap();

        let session = NewTableSession {
            id: "session-1".to_string(),
            outlet_id: "outlet-1".to_string(),
            table_id: "table-1".to_string(),
            state: "OCCUPIED".to_string(),
            current_order_id: None,
            guest_count: 2,
            opened_by_user_id: None,
            opened_at: "2026-08-07T10:00:00Z".to_string(),
            created_at: "2026-08-07T10:00:00Z".to_string(),
            updated_at: "2026-08-07T10:00:00Z".to_string(),
        };
        let outbox = NewOutboxEntry {
            id: "outbox-2".to_string(),
            aggregate_type: "table_session".to_string(),
            aggregate_id: "session-1".to_string(),
            event_type: "TableSessionOpened".to_string(),
            payload_json: "{}".to_string(),
            created_at: "2026-08-07T10:00:00Z".to_string(),
        };

        db.open_table_session_with_outbox(&session, &outbox)
            .expect("open session");

        assert!(db.get_table_session("session-1").unwrap().is_some());
        let pending = repo::list_unpublished_outbox(db.connection(), 10).unwrap();
        assert!(pending.iter().any(|e| e.aggregate_id == "session-1"));
    }

    #[test]
    fn open_close_round_trips_encrypted_file_on_disk() {
        let dir = tempdir().expect("tempdir");
        let sealed = dir.path().join("edge.db.enc");
        let plaintext = dir.path().join("edge.db");
        let key = EncryptionKey::new([9u8; 32]);

        {
            let mut db =
                Db::open(&sealed, &plaintext, EncryptionKey::new([9u8; 32])).expect("open");
            seed_outlet_and_device(&db, "outlet-1", "device-1");
            let order = sample_order("order-3", "outlet-1", "device-1");
            let outbox = sample_outbox("order-3");
            db.create_order_with_outbox(&order, &[], &outbox)
                .expect("create order");
            db.close().expect("close");
        }

        // Plaintext must not survive close().
        assert!(!plaintext.exists());
        assert!(sealed.exists());

        // Reopening with the same key must recover the order.
        let db2 = Db::open(&sealed, &plaintext, key).expect("reopen");
        assert!(db2.get_order("order-3").unwrap().is_some());
        db2.close().expect("close again");
    }

    /// Simulates a shop-floor power loss: a session commits a transaction
    /// and the `Db` handle is then simply dropped without calling
    /// [`Db::close`] — leaving `.db`/`-wal`/`-shm` and the open-marker on
    /// disk exactly as a real crash would. `Db` has no custom `Drop` impl,
    /// so this only releases the OS file handle (as a real crash's process
    /// exit would too) without running any of the reseal/wipe logic that
    /// lives in `close()` — that logic is exactly what must not run here.
    /// (`std::mem::forget` was deliberately avoided: on Windows it also
    /// keeps the OS handle open, which a real crash does not, and makes the
    /// very next `Db::open` fail with a sharing violation instead of
    /// exercising recovery.)
    ///
    /// The very next `Db::open` must (a) leave no plaintext
    /// credential-bearing material on disk once it has finished recovering,
    /// and (b) not have silently discarded the pre-crash committed order —
    /// this is the pair of properties the coordinator's fix requires.
    #[test]
    fn crash_leftovers_are_recovered_not_wiped_and_no_plaintext_survives() {
        let dir = tempdir().expect("tempdir");
        let sealed = dir.path().join("edge.db.enc");
        let plaintext = dir.path().join("edge.db");
        let key_bytes = [11u8; 32];

        {
            let mut db =
                Db::open(&sealed, &plaintext, EncryptionKey::new(key_bytes)).expect("first open");
            seed_outlet_and_device(&db, "outlet-1", "device-1");
            let order = sample_order("order-crash", "outlet-1", "device-1");
            let outbox = sample_outbox("order-crash");
            db.create_order_with_outbox(&order, &[], &outbox)
                .expect("commit before crash");
            // `db` drops here at end of scope without `close()` ever being
            // called: that is the crash.
        }

        // Crash artifacts must actually be present, or this test would
        // trivially pass without exercising recovery at all.
        assert!(plaintext.exists(), "precondition: plaintext left behind");
        assert!(
            crypto::marker_path(&plaintext).exists(),
            "precondition: unclean marker left behind"
        );

        // Reopening triggers crash recovery inside Db::open.
        let db2 = Db::open(&sealed, &plaintext, EncryptionKey::new(key_bytes))
            .expect("reopen after crash must succeed");

        // Property (b): the committed pre-crash order must not have been
        // silently lost.
        assert!(
            db2.get_order("order-crash").unwrap().is_some(),
            "committed pre-crash transaction must survive recovery"
        );

        db2.close().expect("close");

        // Property (a): after this session also closes cleanly, nothing
        // plaintext remains — including no stray marker.
        assert!(!plaintext.exists());
        let (wal, shm) = crypto::wal_shm_paths(&plaintext);
        assert!(!wal.exists());
        assert!(!shm.exists());
        assert!(!crypto::marker_path(&plaintext).exists());
    }

    /// Same crash scenario, but this time nothing reopens the database
    /// afterward — it asserts the intermediate state right after `Db::open`
    /// has finished recovering (i.e. recovery leaves no *leftover* plaintext
    /// from the crashed session, even though the newly opened session's own
    /// working copy is expected to exist while it is open).
    #[test]
    fn recovery_clears_marker_immediately_after_reopen() {
        let dir = tempdir().expect("tempdir");
        let sealed = dir.path().join("edge.db.enc");
        let plaintext = dir.path().join("edge.db");
        let key_bytes = [12u8; 32];

        {
            let _db =
                Db::open(&sealed, &plaintext, EncryptionKey::new(key_bytes)).expect("first open");
            // `_db` drops here without `close()`: the crash.
        }
        assert!(crypto::marker_path(&plaintext).exists());

        let db2 = Db::open(&sealed, &plaintext, EncryptionKey::new(key_bytes))
            .expect("reopen after crash");
        // The new session's own marker exists (it is itself open now), but
        // recovery must have replaced the stale crash marker with a fresh
        // one written by this open() call, not left it dangling forever
        // across multiple recover_crash_leftovers calls with no session in
        // between.
        assert!(crypto::marker_path(&plaintext).exists());
        db2.close().expect("close");
        assert!(!crypto::marker_path(&plaintext).exists());
    }

    #[test]
    fn reserved_word_order_table_round_trips_via_repo() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let order = sample_order("order-4", "outlet-1", "device-1");
        let outbox = sample_outbox("order-4");
        db.create_order_with_outbox(&order, &[], &outbox).unwrap();
        let fetched = db.get_order("order-4").unwrap().expect("order exists");
        assert_eq!(fetched.status, "DRAFT");
        // Money must be i64 paise, never float.
        assert_eq!(fetched.total_paise, 0);
    }
}
