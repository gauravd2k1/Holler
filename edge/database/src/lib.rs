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
use crate::model::{
    NewOrder, NewOrderItem, NewOutboxEntry, NewTableSession, Order, OrderItemAddedMeta,
    OrderItemModifier, OrderItemRemovedMeta, TableSession,
};

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

    /// Adds a line item — and its modifier selections, in the same
    /// transaction — to an already-persisted order, writing the
    /// `order_item` row, the `order_item_modifier` rows, its `local_outbox`
    /// event, and the recomputed order totals all in the *same* SQLite
    /// transaction (ADR-007) — the same guarantee as
    /// [`Db::create_order_with_outbox`]. Rejects the write with
    /// `DbError::OrderNotAmendable` if the order is not `DRAFT` (amendment
    /// is only legal pre-confirmation); rolls back entirely on any failure,
    /// leaving neither the line, its modifiers, nor the outbox row.
    ///
    /// `item.line_total_paise` is **not trusted** — this crate recomputes
    /// it from `item.unit_price_paise`, `item.quantity` and `modifiers` per
    /// the money invariant in `0003_order_item_modifiers.sql`
    /// (`line_total_paise = (unit_price_paise + SUM(price_delta_paise)) *
    /// quantity`) and persists the recomputed value, so a caller cannot
    /// desync the stored total from the snapshot prices that produced it.
    ///
    /// Unlike the general-purpose outbox writers on this type, the caller
    /// does not describe the event: `event_type` (the frozen `ItemAdded`
    /// string) and `payload_json` (the added line's own fields, including
    /// its real modifiers) are built by this crate from the rows it is
    /// writing, so a caller cannot commit a write with a mismatched or
    /// misleading event — see `build_item_added_payload` in `src/repo.rs`.
    /// `meta` supplies only what this crate cannot derive: the outbox row's
    /// own id and the moment the event occurred.
    pub fn add_order_item_with_outbox(
        &mut self,
        item: &NewOrderItem,
        modifiers: &[OrderItemModifier],
        meta: &OrderItemAddedMeta,
    ) -> DbResult<()> {
        let tx = self.conn.transaction()?;
        let outlet_id = repo::require_draft_order(&tx, &item.order_id)?;

        let line_total_paise =
            repo::compute_line_total_paise(item.unit_price_paise, item.quantity, modifiers);
        let stored_item = NewOrderItem {
            line_total_paise,
            ..item.clone()
        };

        repo::insert_order_item(&tx, &stored_item)?;
        for modifier in modifiers {
            repo::insert_order_item_modifier(&tx, modifier)?;
        }
        repo::insert_item_added_outbox(
            &tx,
            &outlet_id,
            &item.order_id,
            &stored_item,
            modifiers,
            meta,
        )?;
        repo::recompute_and_persist_order_totals(&tx, &item.order_id, &item.created_at)?;
        tx.commit()?;
        Ok(())
    }

    /// Removes a line item — and its modifier selections, read before the
    /// delete since `order_item_modifier` cascades — from an
    /// already-persisted order, writing the deletion, the recomputed order
    /// totals, and the `local_outbox` row in the *same* transaction. Per
    /// sync.md §51 financial lines are append-only on the *cloud*; the edge
    /// is where a cashier legitimately removes a line before confirmation,
    /// so the row is actually deleted here, but the emitted `ItemRemoved`
    /// event is what lets the cloud replay the removal — history is
    /// preserved in the outbox event stream, not in this table. Rejects
    /// the write with `DbError::OrderNotAmendable` if the order is not
    /// `DRAFT`.
    ///
    /// Like [`Db::add_order_item_with_outbox`], the caller does not
    /// describe the event: `event_type` (the frozen `ItemRemoved` string)
    /// and `payload_json` (the full removed line, including its modifiers)
    /// are built by this crate from the row it is about to delete, so a
    /// caller cannot commit a mismatched removal record — once the row is
    /// gone there is no local way to recover what it actually was.
    pub fn remove_order_item_with_outbox(
        &mut self,
        order_item_id: &str,
        meta: &OrderItemRemovedMeta,
    ) -> DbResult<()> {
        let tx = self.conn.transaction()?;
        let existing = repo::get_order_item_in_tx(&tx, order_item_id)?
            .ok_or(DbError::NotFound("order_item"))?;
        let outlet_id = repo::require_draft_order(&tx, &existing.order_id)?;
        let modifiers = repo::list_order_item_modifiers_in_tx(&tx, order_item_id)?;

        repo::insert_item_removed_outbox(
            &tx,
            &outlet_id,
            &existing.order_id,
            &existing,
            &modifiers,
            meta,
        )?;
        // order_item_modifier rows for this line cascade-delete with it
        // (ON DELETE CASCADE, foreign_keys pragma ON) — no separate delete
        // needed, and their snapshot has already been captured above.
        repo::delete_order_item(&tx, order_item_id)?;
        repo::recompute_and_persist_order_totals(&tx, &existing.order_id, &meta.occurred_at)?;
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

    fn seed_menu(db: &Db, outlet_id: &str) -> (String, String, String) {
        let category_id = "category-1".to_string();
        let item_id = "item-1".to_string();
        let variant_id = "variant-1".to_string();

        repo::upsert_menu_category(
            db.connection(),
            &model::MenuCategory {
                id: category_id.clone(),
                outlet_id: outlet_id.to_string(),
                name: "Mains".to_string(),
                sort_order: 1,
                config_version: 1,
            },
        )
        .expect("seed category");
        repo::upsert_menu_item(
            db.connection(),
            &model::MenuItem {
                id: item_id.clone(),
                outlet_id: outlet_id.to_string(),
                category_id: category_id.clone(),
                name: "Burger".to_string(),
                base_price_paise: 25000,
                is_available: true,
                config_version: 1,
            },
        )
        .expect("seed menu item");
        repo::upsert_menu_item_variant(
            db.connection(),
            &model::MenuItemVariant {
                id: variant_id.clone(),
                menu_item_id: item_id.clone(),
                name: "Large".to_string(),
                price_delta_paise: 5000,
                config_version: 1,
            },
        )
        .expect("seed variant");
        repo::upsert_menu_item_modifier(
            db.connection(),
            &model::MenuItemModifier {
                id: "modifier-1".to_string(),
                menu_item_id: item_id.clone(),
                group_name: "Cheese".to_string(),
                option_name: "Extra Paneer".to_string(),
                price_delta_paise: 3000,
                min_selection: 0,
                max_selection: 1,
                config_version: 1,
            },
        )
        .expect("seed modifier");

        (category_id, item_id, variant_id)
    }

    fn sample_order_item(
        id: &str,
        order_id: &str,
        menu_item_id: &str,
        line_total_paise: i64,
    ) -> NewOrderItem {
        NewOrderItem {
            id: id.to_string(),
            order_id: order_id.to_string(),
            menu_item_id: menu_item_id.to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: line_total_paise,
            line_total_paise,
            notes: None,
            created_at: "2026-08-07T10:05:00Z".to_string(),
        }
    }

    fn item_added_meta(order_item_id: &str) -> OrderItemAddedMeta {
        OrderItemAddedMeta {
            outbox_id: format!("outbox-{order_item_id}"),
            occurred_at: "2026-08-07T10:05:00Z".to_string(),
        }
    }

    fn item_removed_meta(order_item_id: &str, occurred_at: &str) -> OrderItemRemovedMeta {
        OrderItemRemovedMeta {
            outbox_id: format!("outbox-removed-{order_item_id}"),
            occurred_at: occurred_at.to_string(),
        }
    }

    fn sample_modifier(id: &str, order_item_id: &str, price_delta_paise: i64) -> OrderItemModifier {
        OrderItemModifier {
            id: id.to_string(),
            order_item_id: order_item_id.to_string(),
            modifier_id: "modifier-1".to_string(),
            group_name: "Cheese".to_string(),
            option_name: "Extra Paneer".to_string(),
            price_delta_paise,
            created_at: "2026-08-07T10:05:00Z".to_string(),
        }
    }

    #[test]
    fn add_order_item_writes_outbox_and_recomputes_totals_atomically() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");

        let order = sample_order("order-add", "outlet-1", "device-1");
        db.create_order_with_outbox(&order, &[], &sample_outbox("order-add"))
            .expect("create draft order");

        let item = sample_order_item("item-add-1", "order-add", &menu_item_id, 25000);
        db.add_order_item_with_outbox(&item, &[], &item_added_meta("item-add-1"))
            .expect("add item");

        let stored_order = db.get_order("order-add").unwrap().expect("order exists");
        assert_eq!(stored_order.subtotal_paise, 25000);
        assert_eq!(stored_order.total_paise, 25000);
        assert_eq!(stored_order.version, 2, "version must bump on amendment");

        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        assert!(pending.iter().any(|e| e.aggregate_id == "order-add"));
    }

    /// TASK 1: modifiers persist in the same transaction as the line, and
    /// the stored `line_total_paise` obeys the money invariant from
    /// `0003_order_item_modifiers.sql`:
    ///     line_total_paise = (unit_price_paise + SUM(price_delta_paise)) * quantity
    /// — even when the caller's `NewOrderItem.line_total_paise` disagrees
    /// with it (the crate does not trust that field).
    #[test]
    fn add_order_item_persists_modifiers_and_enforces_money_invariant() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");

        let order = sample_order("order-inv", "outlet-1", "device-1");
        db.create_order_with_outbox(&order, &[], &sample_outbox("order-inv"))
            .expect("create draft order");

        // unit_price_paise 41000, quantity 2, two modifiers (+3000, -500):
        // line_total_paise must be (41000 + 3000 - 500) * 2 = 87000,
        // regardless of the wrong 999 the caller put in line_total_paise.
        let mut item = sample_order_item("item-inv-1", "order-inv", &menu_item_id, 999);
        item.unit_price_paise = 41000;
        item.quantity = 2;
        let modifiers = vec![
            sample_modifier("mod-inv-a", "item-inv-1", 3000),
            OrderItemModifier {
                price_delta_paise: -500,
                ..sample_modifier("mod-inv-b", "item-inv-1", -500)
            },
        ];

        db.add_order_item_with_outbox(&item, &modifiers, &item_added_meta("item-inv-1"))
            .expect("add item with modifiers");

        let stored_items = repo::list_order_items(db.connection(), "order-inv").unwrap();
        assert_eq!(stored_items.len(), 1);
        assert_eq!(
            stored_items[0].line_total_paise, 87000,
            "stored line_total_paise must obey the money invariant, not the caller's value"
        );

        let stored_modifiers =
            repo::list_order_item_modifiers(db.connection(), "item-inv-1").unwrap();
        assert_eq!(stored_modifiers.len(), 2);
        let delta_sum: i64 = stored_modifiers.iter().map(|m| m.price_delta_paise).sum();
        assert_eq!(delta_sum, 2500);

        let stored_order = db.get_order("order-inv").unwrap().expect("order exists");
        assert_eq!(
            stored_order.subtotal_paise, 87000,
            "order subtotal must be driven by the invariant-computed line total"
        );
    }

    /// The add-path rollback guarantee must cover modifier rows too: a
    /// failure after modifiers are inserted (colliding outbox id) must
    /// leave neither the line, its modifiers, nor the outbox row.
    #[test]
    fn failed_add_order_item_transaction_leaves_no_modifier_rows_either() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");

        let order = sample_order("order-fail-mod", "outlet-1", "device-1");
        let colliding_id = "colliding-outbox-mod".to_string();
        let create_outbox = NewOutboxEntry {
            id: colliding_id.clone(),
            ..sample_outbox("order-fail-mod")
        };
        db.create_order_with_outbox(&order, &[], &create_outbox)
            .expect("create draft order");

        let item = sample_order_item("item-fail-mod", "order-fail-mod", &menu_item_id, 10000);
        let modifiers = vec![sample_modifier("mod-fail-1", "item-fail-mod", 1000)];
        let colliding_meta = OrderItemAddedMeta {
            outbox_id: colliding_id,
            occurred_at: "2026-08-07T10:05:00Z".to_string(),
        };

        let result = db.add_order_item_with_outbox(&item, &modifiers, &colliding_meta);
        assert!(
            result.is_err(),
            "colliding outbox id must fail the transaction"
        );

        let items = repo::list_order_items(db.connection(), "order-fail-mod").unwrap();
        assert!(items.is_empty(), "no line must be committed");
        let stored_modifiers =
            repo::list_order_item_modifiers(db.connection(), "item-fail-mod").unwrap();
        assert!(
            stored_modifiers.is_empty(),
            "no modifier row must be committed either"
        );
    }

    /// Finding 1: the caller supplies only ids/timestamps for an added
    /// line, never the event description. This proves the emitted event is
    /// truthful even when a hostile/buggy caller supplies nothing about the
    /// item's content — the payload must still describe the exact row that
    /// was written, because there is no field on `OrderItemAddedMeta` a
    /// caller could use to lie about it.
    #[test]
    fn add_order_item_outbox_payload_is_derived_and_cannot_be_spoofed() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");

        let order = sample_order("order-payload", "outlet-1", "device-1");
        db.create_order_with_outbox(&order, &[], &sample_outbox("order-payload"))
            .expect("create draft order");

        let item = sample_order_item("item-payload-1", "order-payload", &menu_item_id, 42_00);
        db.add_order_item_with_outbox(&item, &[], &item_added_meta("item-payload-1"))
            .expect("add item");

        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        let event = pending
            .iter()
            .find(|e| e.aggregate_id == "order-payload" && e.event_type == "ItemAdded")
            .expect("ItemAdded event must exist");

        // event_type is the frozen contract string, chosen by the crate —
        // not whatever a caller might have passed.
        assert_eq!(event.event_type, "ItemAdded");

        let payload: serde_json::Value = serde_json::from_str(&event.payload_json).unwrap();
        assert_eq!(payload["event_type"], "ItemAdded");
        assert_eq!(payload["outlet_id"], "outlet-1");
        assert_eq!(payload["schema_version"], 1);
        assert_eq!(payload["data"]["order_id"], "order-payload");
        assert_eq!(payload["data"]["item"]["id"], "item-payload-1");
        assert_eq!(payload["data"]["item"]["menu_item_id"], menu_item_id);
        assert_eq!(payload["data"]["item"]["quantity"], 1);
        // Money must be i64 paise in the payload too, never float.
        assert_eq!(payload["data"]["item"]["unit_price_paise"], 42_00);
        assert_eq!(payload["data"]["item"]["line_total_paise"], 42_00);
    }

    #[test]
    fn add_order_item_rejects_non_draft_order_and_writes_nothing() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");

        let mut order = sample_order("order-confirmed", "outlet-1", "device-1");
        order.status = "CONFIRMED".to_string();
        db.create_order_with_outbox(&order, &[], &sample_outbox("order-confirmed"))
            .expect("create confirmed order");

        let item = sample_order_item("item-reject-1", "order-confirmed", &menu_item_id, 25000);
        let result = db.add_order_item_with_outbox(&item, &[], &item_added_meta("item-reject-1"));

        assert!(matches!(result, Err(DbError::OrderNotAmendable { .. })));

        let items = repo::list_order_items(db.connection(), "order-confirmed").unwrap();
        assert!(items.is_empty(), "rejected item must not be persisted");
        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        assert!(pending
            .iter()
            .all(|e| e.aggregate_id != "order-confirmed" || e.event_type != "ItemAdded"));
    }

    /// Proves the same rollback guarantee as
    /// `failed_order_transaction_leaves_neither_order_nor_outbox_row`, for
    /// the amendment path: a failure partway through (here, a duplicate
    /// primary key on the second insert attempt) must leave neither a
    /// second copy of the line nor its outbox row, and must not have
    /// recomputed totals off a half-applied change.
    #[test]
    fn failed_add_order_item_transaction_leaves_neither_item_nor_outbox_row() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");

        let order = sample_order("order-fail-add", "outlet-1", "device-1");
        db.create_order_with_outbox(&order, &[], &sample_outbox("order-fail-add"))
            .expect("create draft order");

        let item = sample_order_item("item-dup", "order-fail-add", &menu_item_id, 10000);
        db.add_order_item_with_outbox(&item, &[], &item_added_meta("item-dup"))
            .expect("first add succeeds");

        let before = repo::list_unpublished_outbox(db.connection(), 100)
            .unwrap()
            .len();

        // Same order_item id again -> PRIMARY KEY collision partway through
        // the transaction (after require_draft_order passed, before totals
        // are recomputed), even though this second outbox row's own id
        // differs.
        let second_meta = OrderItemAddedMeta {
            outbox_id: "outbox-item-dup-2".to_string(),
            occurred_at: "2026-08-07T10:06:00Z".to_string(),
        };
        let result = db.add_order_item_with_outbox(&item, &[], &second_meta);
        assert!(result.is_err(), "duplicate id must fail the transaction");

        let items = repo::list_order_items(db.connection(), "order-fail-add").unwrap();
        assert_eq!(items.len(), 1, "no duplicate line must be committed");
        let after = repo::list_unpublished_outbox(db.connection(), 100)
            .unwrap()
            .len();
        assert_eq!(after, before, "no extra outbox row must be committed");

        let stored_order = db
            .get_order("order-fail-add")
            .unwrap()
            .expect("order exists");
        assert_eq!(
            stored_order.subtotal_paise, 10000,
            "totals must reflect only the successful add, not a half-applied second one"
        );
    }

    #[test]
    fn remove_order_item_writes_outbox_and_recomputes_totals_atomically() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");

        let order = sample_order("order-remove", "outlet-1", "device-1");
        let item_a = sample_order_item("item-remove-a", "order-remove", &menu_item_id, 25000);
        let item_b = sample_order_item("item-remove-b", "order-remove", &menu_item_id, 15000);
        db.create_order_with_outbox(&order, &[item_a, item_b], &sample_outbox("order-remove"))
            .expect("create order with two lines");

        db.remove_order_item_with_outbox(
            "item-remove-a",
            &item_removed_meta("item-remove-a", "2026-08-07T10:10:00Z"),
        )
        .expect("remove line a");

        let remaining = repo::list_order_items(db.connection(), "order-remove").unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "item-remove-b");

        let stored_order = db.get_order("order-remove").unwrap().expect("order exists");
        assert_eq!(
            stored_order.subtotal_paise, 15000,
            "totals must be recomputed from remaining snapshot prices"
        );

        // The removal must be observable as its own outbox event, not just
        // a smaller order — the cloud replays what happened.
        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        let removal_event = pending
            .iter()
            .find(|e| e.aggregate_id == "order-remove" && e.event_type == "ItemRemoved")
            .expect("removal outbox event must exist");
        assert_eq!(removal_event.event_type, "ItemRemoved");

        let payload: serde_json::Value = serde_json::from_str(&removal_event.payload_json).unwrap();
        assert_eq!(payload["data"]["order_id"], "order-remove");
        assert_eq!(payload["data"]["item"]["id"], "item-remove-a");
        assert_eq!(payload["data"]["item"]["line_total_paise"], 25000);
    }

    /// Finding 1 for the removal path: the caller supplies only the id being
    /// removed and a timestamp, never the event description — the emitted
    /// `ItemRemoved` payload, including modifiers, is derived from the row
    /// read just before the delete, so a caller cannot describe a
    /// mismatched removal.
    #[test]
    fn remove_order_item_outbox_payload_includes_real_modifiers_and_cannot_be_spoofed() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");

        let order = sample_order("order-remove-mod", "outlet-1", "device-1");
        db.create_order_with_outbox(&order, &[], &sample_outbox("order-remove-mod"))
            .expect("create draft order");

        let item = sample_order_item("item-remove-mod", "order-remove-mod", &menu_item_id, 0);
        let modifier = sample_modifier("mod-remove-1", "item-remove-mod", 3000);
        db.add_order_item_with_outbox(
            &item,
            std::slice::from_ref(&modifier),
            &item_added_meta("item-remove-mod"),
        )
        .expect("add item with modifier");

        db.remove_order_item_with_outbox(
            "item-remove-mod",
            &item_removed_meta("item-remove-mod", "2026-08-07T10:11:00Z"),
        )
        .expect("remove item with modifier");

        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        let removal_event = pending
            .iter()
            .find(|e| e.aggregate_id == "order-remove-mod" && e.event_type == "ItemRemoved")
            .expect("removal outbox event must exist");

        let payload: serde_json::Value = serde_json::from_str(&removal_event.payload_json).unwrap();
        let modifiers = payload["data"]["item"]["modifiers"]
            .as_array()
            .expect("modifiers array");
        assert_eq!(
            modifiers.len(),
            1,
            "the removed item's real modifier must be in the payload"
        );
        assert_eq!(modifiers[0]["modifier_id"], "modifier-1");
        assert_eq!(modifiers[0]["price_delta_paise"], 3000);
        // unit_price_paise = 0, quantity = 1, one modifier at +3000 paise:
        // line_total_paise = (0 + 3000) * 1 = 3000.
        assert_eq!(payload["data"]["item"]["line_total_paise"], 3000);

        // Once deleted, there is no local way to reconstruct this line, so
        // there is no residual order_item_modifier row either (cascade).
        assert!(db.get_order("order-remove-mod").unwrap().is_some());
        let remaining = repo::list_order_items(db.connection(), "order-remove-mod").unwrap();
        assert!(remaining.is_empty());
    }

    /// Finding 2 (mirror of `failed_add_order_item_transaction_leaves_neither_item_nor_outbox_row`
    /// for the remove path): forces a failure *after* `delete_order_item`
    /// has run but before commit (a duplicate `local_outbox.id` — the
    /// outbox insert's own PRIMARY KEY collides with an existing row) and
    /// proves the deleted line is still present and no outbox row was
    /// added. The rollback guarantee was previously asserted only by
    /// analogy to the add path; this exercises it directly.
    #[test]
    fn failed_remove_order_item_transaction_leaves_item_and_writes_no_outbox_row() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");

        let order = sample_order("order-fail-remove", "outlet-1", "device-1");
        let item = sample_order_item(
            "item-fail-remove",
            "order-fail-remove",
            &menu_item_id,
            20000,
        );
        // Reuse the same outbox id for both the order-creation event and the
        // removal attempt, so the removal's outbox insert collides on
        // local_outbox's PRIMARY KEY and fails partway through the
        // transaction, after delete_order_item has already run.
        let colliding_id = "colliding-outbox-id".to_string();
        let create_outbox = NewOutboxEntry {
            id: colliding_id.clone(),
            ..sample_outbox("order-fail-remove")
        };
        db.create_order_with_outbox(&order, &[item], &create_outbox)
            .expect("create order with one line");

        let subtotal_before_attempt = db
            .get_order("order-fail-remove")
            .unwrap()
            .expect("order exists")
            .subtotal_paise;
        let before = repo::list_unpublished_outbox(db.connection(), 100)
            .unwrap()
            .len();

        let colliding_meta = OrderItemRemovedMeta {
            outbox_id: colliding_id,
            occurred_at: "2026-08-07T10:10:00Z".to_string(),
        };
        let result = db.remove_order_item_with_outbox("item-fail-remove", &colliding_meta);
        assert!(
            result.is_err(),
            "colliding outbox id must fail the transaction"
        );

        let remaining = repo::list_order_items(db.connection(), "order-fail-remove").unwrap();
        assert_eq!(
            remaining.len(),
            1,
            "the deleted-then-rolled-back line must still be present"
        );
        assert_eq!(remaining[0].id, "item-fail-remove");

        let after = repo::list_unpublished_outbox(db.connection(), 100)
            .unwrap()
            .len();
        assert_eq!(after, before, "no removal outbox row must be committed");

        let stored_order = db
            .get_order("order-fail-remove")
            .unwrap()
            .expect("order exists");
        assert_eq!(
            stored_order.subtotal_paise, subtotal_before_attempt,
            "totals must not have been recomputed off the rolled-back removal"
        );
    }

    #[test]
    fn remove_order_item_rejects_non_draft_order() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");

        let order = sample_order("order-remove-reject", "outlet-1", "device-1");
        let item = sample_order_item(
            "item-remove-reject",
            "order-remove-reject",
            &menu_item_id,
            25000,
        );
        db.create_order_with_outbox(&order, &[item], &sample_outbox("order-remove-reject"))
            .expect("create order with one line");

        // Confirm the order out of DRAFT directly (bypassing this crate's
        // amendment API, which is exactly what a caller would have done
        // via a status-transition path outside this test's concern).
        db.connection()
            .execute(
                "UPDATE \"order\" SET status = 'CONFIRMED' WHERE id = 'order-remove-reject'",
                [],
            )
            .unwrap();

        let result = db.remove_order_item_with_outbox(
            "item-remove-reject",
            &item_removed_meta("item-remove-reject", "2026-08-07T10:10:00Z"),
        );
        assert!(matches!(result, Err(DbError::OrderNotAmendable { .. })));

        let remaining = repo::list_order_items(db.connection(), "order-remove-reject").unwrap();
        assert_eq!(remaining.len(), 1, "line must survive the rejected removal");
    }

    #[test]
    fn menu_list_functions_are_outlet_scoped_and_read_only() {
        let db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (category_id, item_id, variant_id) = seed_menu(&db, "outlet-1");

        let categories =
            repo::list_menu_categories_for_outlet(db.connection(), "outlet-1").unwrap();
        assert_eq!(categories.len(), 1);
        assert_eq!(categories[0].id, category_id);

        let variants =
            repo::list_menu_item_variants_for_outlet(db.connection(), "outlet-1").unwrap();
        assert_eq!(variants.len(), 1);
        assert_eq!(variants[0].id, variant_id);
        assert_eq!(variants[0].menu_item_id, item_id);
        // Money must be i64 paise, never float.
        assert_eq!(variants[0].price_delta_paise, 5000);

        let modifiers =
            repo::list_menu_item_modifiers_for_outlet(db.connection(), "outlet-1").unwrap();
        assert_eq!(modifiers.len(), 1);
        assert_eq!(modifiers[0].option_name, "Extra Paneer");

        // A different outlet must see none of this outlet's menu.
        let other = repo::list_menu_categories_for_outlet(db.connection(), "outlet-2").unwrap();
        assert!(other.is_empty());
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

    /// The wire fixture this crate can genuinely round-trip and prove it
    /// byte-for-byte: an `order_item`'s own `OrderItemSchema` shape,
    /// including its `modifiers`. This is the reference implementation the
    /// orchestrator asked for — deliberately a stronger claim than the
    /// shape round-trips in `packages/contracts`: those prove TypeScript
    /// and Go agree on a shape; this proves the SQLite storage this crate
    /// owns can actually hold what they agreed on, including the
    /// `order_item_modifier` rows added in contracts 0.2.3 (TASK 1).
    ///
    /// SCOPE, STATED HONESTLY RATHER THAN PAPERED OVER: `fixtures/order.json`
    /// is a full `CanonicalOrder`, not just an order line. This crate's
    /// `"order"` table (`0001_init.sql`, frozen, not owned by this crate)
    /// does not have a column for every `CanonicalOrder` field — there is
    /// no home here for `source`, `customer`, `delivery_address`,
    /// `packaging_paise`, `delivery_charge_paise`, `aggregator_discount_paise`,
    /// `merchant_discount_paise`, `payment_status`, `payment_source`,
    /// `preparation_time_minutes`, `rider`, `timestamps.confirmed_at`,
    /// `source_payload`, `schema_version`, or `external_order_id`; and
    /// `taxes_paise` on the wire is `tax_paise` in this table. A full
    /// byte-for-byte `CanonicalOrder` round trip through this crate is
    /// therefore **not currently possible** — that is a genuine finding
    /// about `0001_init.sql`'s `"order"` table, not something this test
    /// tries to hide by comparing only a trimmed-down subset of the order
    /// envelope. What this test proves instead is the piece contracts 0.2.3
    /// actually added storage for: `fixtures/order.json`'s `items[0]`,
    /// including its real modifier, holds its exact shape after a genuine
    /// write through the crate's public API and a genuine read back —
    /// nothing in the comparison is trimmed to force a pass.
    #[test]
    fn order_item_fixture_round_trips_byte_for_byte_through_public_api() {
        let fixture_text = include_str!("../../../packages/contracts/fixtures/order.json");
        let fixture: serde_json::Value =
            serde_json::from_str(fixture_text).expect("fixture must be valid JSON");
        let fixture_item = fixture["items"][0].clone();
        assert!(
            !fixture_item.is_null(),
            "fixture must contain at least one item"
        );

        // Pull every field this test writes straight from the fixture, so
        // the test cannot silently diverge from what the contract actually
        // specifies.
        let item_id = fixture_item["id"].as_str().unwrap().to_string();
        let menu_item_id = fixture_item["menu_item_id"].as_str().unwrap().to_string();
        let variant_id = fixture_item["variant_id"].as_str().map(str::to_string);
        let quantity = fixture_item["quantity"].as_i64().unwrap();
        let unit_price_paise = fixture_item["unit_price_paise"].as_i64().unwrap();
        let fixture_line_total_paise = fixture_item["line_total_paise"].as_i64().unwrap();
        let notes = fixture_item["notes"].as_str().map(str::to_string);
        let fixture_modifiers = fixture_item["modifiers"].as_array().unwrap().clone();

        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");

        // The order envelope this crate CAN persist (§ the doc comment
        // above): enough of it, as DRAFT, to legally call the public
        // amendment API under test. table_id and outlet_id are sourced from
        // the fixture where this schema has a matching column; status is
        // forced to DRAFT because amendment is only legal pre-confirmation
        // — the fixture's own "SENT_TO_KITCHEN" is exactly one of the
        // fields this table cannot currently hold a faithful round trip of.
        let outlet_id = fixture["outlet_id"].as_str().unwrap().to_string();
        let table_id = fixture["table_id"].as_str().map(str::to_string);
        repo::upsert_outlet(
            db.connection(),
            &model::Outlet {
                id: outlet_id.clone(),
                brand_id: "brand-fixture".to_string(),
                name: "Fixture Outlet".to_string(),
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
                id: "device-fixture".to_string(),
                outlet_id: outlet_id.clone(),
                kind: "POS".to_string(),
                name: "Fixture Till".to_string(),
                last_seen_at: None,
                created_at: "2026-08-07T00:00:00Z".to_string(),
            },
        )
        .expect("seed device");

        // order_item.menu_item_id is a real FOREIGN KEY (0001_init.sql), so
        // the catalog row it points at must exist — seed a menu_item under
        // the fixture's own id, matching this crate's usual catalog-is-config
        // pattern rather than reaching around it.
        repo::upsert_menu_category(
            db.connection(),
            &model::MenuCategory {
                id: "category-fixture".to_string(),
                outlet_id: outlet_id.clone(),
                name: "Fixture Category".to_string(),
                sort_order: 1,
                config_version: 1,
            },
        )
        .expect("seed category");
        repo::upsert_menu_item(
            db.connection(),
            &model::MenuItem {
                id: menu_item_id.clone(),
                outlet_id: outlet_id.clone(),
                category_id: "category-fixture".to_string(),
                name: "Fixture Item".to_string(),
                base_price_paise: unit_price_paise,
                is_available: true,
                config_version: 1,
            },
        )
        .expect("seed menu item");

        let order_id = fixture["holler_order_id"].as_str().unwrap().to_string();
        let order = NewOrder {
            id: order_id.clone(),
            outlet_id: outlet_id.clone(),
            device_id: "device-fixture".to_string(),
            order_type: fixture["order_type"].as_str().unwrap().to_string(),
            status: "DRAFT".to_string(),
            table_id,
            subtotal_paise: 0,
            discount_paise: 0,
            tax_paise: 0,
            total_paise: 0,
            created_at: "2026-08-07T10:00:00Z".to_string(),
            updated_at: "2026-08-07T10:00:00Z".to_string(),
        };
        db.create_order_with_outbox(&order, &[], &sample_outbox(&order_id))
            .expect("create draft order from fixture envelope");

        let new_item = NewOrderItem {
            id: item_id.clone(),
            order_id: order_id.clone(),
            menu_item_id,
            variant_id,
            quantity,
            unit_price_paise,
            // Deliberately the fixture's own value: the invariant recompute
            // must agree with it (below), not merely accept whatever this
            // test supplies.
            line_total_paise: fixture_line_total_paise,
            notes,
            created_at: "2026-08-07T10:05:00Z".to_string(),
        };
        let modifiers: Vec<OrderItemModifier> = fixture_modifiers
            .iter()
            .enumerate()
            .map(|(i, m)| OrderItemModifier {
                id: format!("fixture-modifier-{i}"),
                order_item_id: item_id.clone(),
                modifier_id: m["modifier_id"].as_str().unwrap().to_string(),
                group_name: m["group_name"].as_str().unwrap().to_string(),
                option_name: m["option_name"].as_str().unwrap().to_string(),
                price_delta_paise: m["price_delta_paise"].as_i64().unwrap(),
                created_at: "2026-08-07T10:05:00Z".to_string(),
            })
            .collect();

        db.add_order_item_with_outbox(&new_item, &modifiers, &item_added_meta(&item_id))
            .expect("persist fixture item through the public API");

        // Read back through the public API only — no reaching around it.
        let stored_items = repo::list_order_items(db.connection(), &order_id).unwrap();
        assert_eq!(stored_items.len(), 1);
        let stored_item = &stored_items[0];
        assert_eq!(
            stored_item.line_total_paise, fixture_line_total_paise,
            "the money invariant must reproduce the fixture's own line_total_paise"
        );
        let stored_modifiers = repo::list_order_item_modifiers(db.connection(), &item_id).unwrap();

        // Re-serialize to the contract's OrderItemSchema shape and compare
        // byte-for-byte (both sides through the same serde_json::Value
        // canonical serialization, so key ordering cannot mask a
        // difference in either direction).
        let reconstructed = repo::item_json(
            &stored_item.id,
            &stored_item.menu_item_id,
            stored_item.variant_id.as_deref(),
            stored_item.quantity,
            stored_item.unit_price_paise,
            stored_item.line_total_paise,
            stored_item.notes.as_deref(),
            &stored_modifiers,
        );

        let reconstructed_bytes = serde_json::to_string(&reconstructed).unwrap();
        let fixture_bytes = serde_json::to_string(&fixture_item).unwrap();
        assert_eq!(
            reconstructed_bytes, fixture_bytes,
            "order_item + order_item_modifier storage must round-trip the fixture's item byte-for-byte"
        );
    }
}
