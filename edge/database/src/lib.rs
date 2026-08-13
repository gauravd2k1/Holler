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
    Kot, KotStatusHistoryEntry, KotTicketItem, KotTransitionMeta, NewOrder, NewOrderItem,
    NewOutboxEntry, NewTableSession, Order, OrderConfirmedMeta, OrderItemAddedMeta,
    OrderItemModifier, OrderItemRemovedMeta, SendToKitchenMeta, Station, TableSession,
};
use std::collections::BTreeMap;

/// An open handle to the edge database. The plaintext SQLite file backing
/// this handle exists on disk only between [`Db::open`] and [`Db::close`]
/// (see `src/crypto.rs` for why, and the limitation that implies).
pub struct Db {
    /// `None` once the handle has been shut down. Optional rather than a bare
    /// `Connection` so that both [`Db::close`] and the [`Drop`] fallback can
    /// take ownership of it: a type implementing `Drop` cannot have fields
    /// moved out of it, and the seal-on-drop guarantee requires exactly that.
    conn: Option<Connection>,
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
            conn: Some(conn),
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
            conn: Some(conn),
            plaintext_path: PathBuf::new(),
            sealed_path: PathBuf::new(),
            key: EncryptionKey::new([0u8; 32]),
        })
    }

    /// Checkpoints WAL, closes the connection, re-seals the file at rest
    /// with a fresh nonce, and wipes the plaintext working copy and its
    /// `-wal`/`-shm` siblings. Must be the only way this crate's plaintext
    /// file is ever left on disk after use.
    ///
    /// Prefer this over relying on [`Drop`]: it is the only variant that can
    /// report a sealing failure to the caller. Drop reseals as a fallback but
    /// can only log.
    pub fn close(mut self) -> DbResult<()> {
        self.shutdown_in_place()
    }

    /// Seals and wipes without consuming the handle, so a caller holding a
    /// `Db` behind a mutex (as the POS does in its Tauri-managed state) can
    /// shut it down on application exit without moving it out.
    ///
    /// Idempotent: a second call is a no-op, which is what makes the [`Drop`]
    /// fallback safe after an explicit [`Db::close`].
    pub fn shutdown_in_place(&mut self) -> DbResult<()> {
        let conn = match self.conn.take() {
            Some(conn) => conn,
            // Already shut down.
            None => return Ok(()),
        };

        if self.plaintext_path.as_os_str().is_empty() {
            // In-memory test handle: nothing on disk to seal or wipe.
            return Ok(());
        }

        conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")?;
        // rusqlite's Connection::close returns the connection back on
        // failure; we own it here so a failure can't leave a half-closed
        // handle for a caller to misuse.
        conn.close().map_err(|(_, e)| DbError::Sqlite(e))?;

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
    ///
    /// Panics if the handle has already been shut down; that is a caller bug
    /// (using a `Db` after `shutdown_in_place`), not a runtime condition.
    pub fn connection(&self) -> &Connection {
        self.conn
            .as_ref()
            .expect("edge database handle used after shutdown")
    }

    fn connection_mut(&mut self) -> &mut Connection {
        self.conn
            .as_mut()
            .expect("edge database handle used after shutdown")
    }

    /// Whether this handle has already been sealed and wiped. Used by tests
    /// and by callers that want to avoid a redundant shutdown.
    pub fn is_shut_down(&self) -> bool {
        self.conn.is_none()
    }

    /// Test-only: models a process killed mid-session (power loss, SIGKILL,
    /// task-manager End Task) — the plaintext file, its `-wal`/`-shm`
    /// siblings and the unclean marker are all left on disk, with nothing
    /// sealed.
    ///
    /// It drops the SQLite `Connection` first so the OS file handle is
    /// released; `std::mem::forget` would model a crash more literally but
    /// would keep the handle open and make the next `Db::open` fail with a
    /// Windows sharing violation instead of exercising recovery. Taking the
    /// connection also disarms the `Drop` seal, which is the behaviour under
    /// test here.
    #[cfg(test)]
    fn simulate_crash_for_tests(&mut self) {
        drop(self.conn.take());
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
        let tx = self.connection_mut().transaction()?;
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
        let tx = self.connection_mut().transaction()?;
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
        let tx = self.connection_mut().transaction()?;
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

    /// Confirms a `DRAFT` order — the cashier's DRAFT -> CONFIRMED
    /// transition — stamping `status = 'CONFIRMED'` and `confirmed_at`,
    /// bumping `version`/`updated_at`, and writing the `OrderConfirmed`
    /// `local_outbox` row, all in the *same* SQLite transaction (ADR-007).
    /// Rejects the write with `DbError::OrderNotConfirmable` if the order is
    /// not `DRAFT` (checked and mutated inside one transaction, so this
    /// cannot race with a concurrent amendment or a second confirm); rolls
    /// back entirely on any failure, leaving neither the status change nor
    /// the outbox row.
    ///
    /// Milestone 1 scope: this is the DRAFT -> CONFIRMED transition only —
    /// no KOT generation (Milestone 2), no tax/discount computation
    /// (Milestone 3), no payment capture.
    ///
    /// Like [`Db::add_order_item_with_outbox`], the caller does not
    /// describe the event: `event_type` (the frozen `OrderConfirmed`
    /// string) and `payload_json` (`{ order_id, confirmed_at }`) are built
    /// by this crate from the row it is writing, so a caller cannot commit
    /// a mismatched or misleading confirmation event. `meta` supplies only
    /// what this crate cannot derive: the outbox row's own id, the moment
    /// the event occurred, and `confirmed_at` — the moment the *edge*
    /// recorded the confirmation (sync.md §50.1); this crate never lets a
    /// cloud-supplied clock stamp it.
    pub fn confirm_order_with_outbox(
        &mut self,
        order_id: &str,
        meta: &OrderConfirmedMeta,
    ) -> DbResult<()> {
        let tx = self.connection_mut().transaction()?;
        let outlet_id = repo::require_draft_order_for_confirm(&tx, order_id)?;
        repo::stamp_order_confirmed(&tx, order_id, &meta.confirmed_at, &meta.occurred_at)?;
        repo::insert_order_confirmed_outbox(&tx, &outlet_id, order_id, meta)?;
        tx.commit()?;
        Ok(())
    }

    /// Sets `order_type`/`table_id` on a `DRAFT` order — the cashier
    /// correcting the order's shape before Send. This is the fix for the M2
    /// P0 regression (docs/retro.md): a DRAFT order is created on the first
    /// cart line for crash durability, but its shape must stay editable for
    /// the order's *entire* DRAFT lifetime, not just before it existed.
    /// Rejects with `DbError::OrderNotAmendable` once the order has left
    /// DRAFT — same enforcement, and the same transaction-scoped check, as
    /// [`Db::add_order_item_with_outbox`]/[`Db::remove_order_item_with_outbox`]:
    /// once a ticket is with the kitchen the shape is history, and that
    /// must be a rejection, not a silent no-op.
    ///
    /// **No new `local_outbox` row is written for this transition.** The
    /// frozen event catalog (`packages/contracts/src/types/events.ts`,
    /// `OUTBOX_EVENT_TYPES`) has no "order shape changed"/"order amended"
    /// event, and this crate never originates a wire event outside that
    /// catalog (ADR-008) — contracts are read-only to builder agents, and
    /// adding one requires an ADR the orchestrator session owns, not this
    /// task. `order_type`/`table_id` are already part of the `OrderCreated`
    /// snapshot `Db::create_order_with_outbox` queued when the order was
    /// first persisted, so this is a correction of that same
    /// not-yet-observed-by-the-cloud fact, not a second fact needing a
    /// second event: `corrected_order_created_payload_json`, when supplied,
    /// overwrites that queued row's `payload_json` in place *only* while it
    /// is still unpublished (`repo::update_pending_order_created_payload`).
    /// Nothing has left the device for that event yet, so this is not
    /// rewriting delivered history.
    ///
    /// Residual gap, called out rather than hidden: if the outlet is online
    /// and the sync worker has already published this order's
    /// `OrderCreated` event by the time the cashier fixes the shape, the
    /// cloud's copy stays stale until a future milestone adds a proper
    /// `OrderShapeChanged`-style event (needs a contract ADR). The realistic
    /// window is small — this only matters between "first item tapped" and
    /// "shape corrected", normally seconds — but it is a real desync, not
    /// merely a theoretical one, and is out of this crate's authority to
    /// close today.
    pub fn update_order_shape_with_outbox(
        &mut self,
        order_id: &str,
        order_type: &str,
        table_id: Option<&str>,
        updated_at: &str,
        corrected_order_created_payload_json: Option<&str>,
    ) -> DbResult<()> {
        let tx = self.connection_mut().transaction()?;
        repo::require_draft_order(&tx, order_id)?;
        repo::update_order_shape(&tx, order_id, order_type, table_id, updated_at)?;
        if let Some(payload) = corrected_order_created_payload_json {
            repo::update_pending_order_created_payload(&tx, order_id, payload)?;
        }
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
        let tx = self.connection_mut().transaction()?;
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
        let tx = self.connection_mut().transaction()?;
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
        repo::get_order(self.connection(), id)
    }

    pub fn get_table_session(&self, id: &str) -> DbResult<Option<TableSession>> {
        repo::get_table_session(self.connection(), id)
    }

    /// Generates KOTs for an order's send-to-kitchen moment: resolves each
    /// unticketed order line to its station(s) via `menu_item_station`
    /// (docs/spec/kitchen.md — an item may route to more than one station,
    /// e.g. a thali hits MAIN_KITCHEN and TANDOOR), produces exactly one
    /// `kot` row per affected station (never one ticket for the whole
    /// order), and writes every `kot` row plus its `KOTCreated` and the
    /// order's `SentToKitchen` `local_outbox` rows in the *same* SQLite
    /// transaction (ADR-007).
    ///
    /// Idempotent-by-delta across repeated calls on the same order: an
    /// order line already present on some earlier KOT (any status) is never
    /// re-ticketed. All tickets produced by one call share the next
    /// `sequence` number for the order — a later call (e.g. items added
    /// after the first send) produces brand-new tickets carrying the next
    /// sequence, never a mutation of an earlier one, matching the #132 ->
    /// #132-A change history in docs/spec/kitchen.md.
    ///
    /// Returns `DbError::UnroutedKitchenItems` — naming every affected line
    /// — if the unticketed lines are a *mix* of routed and unrouted (at
    /// least one of each). Nothing in `packages/contracts` marks a menu item
    /// as deliberately non-production, so an unrouted line cannot be
    /// distinguished from a routing config gap; the whole call is rejected
    /// and **no `kot` row is written for any line, routed or not**, so a
    /// cashier is never told an order reached the kitchen when one of its
    /// dishes did not (docs/backlog-m2.md Track A). Returns
    /// `DbError::NothingToSendToKitchen` if there were zero unticketed lines
    /// at all, or if every unticketed line is unrouted — both are "nothing
    /// legitimately goes to any kitchen from this call" and keep the
    /// pre-existing, already-correct outcome; only the *mixed* case changed.
    /// Returns `DbError::OrderNotSendableToKitchen` if the order is not in a
    /// status that can produce KOTs (DRAFT, or already
    /// SERVED/BILLED/PAID/CLOSED/CANCELLED).
    pub fn send_order_to_kitchen_with_outbox(
        &mut self,
        order_id: &str,
        meta: &SendToKitchenMeta,
    ) -> DbResult<Vec<Kot>> {
        self.send_order_to_kitchen_with_outbox_inner(order_id, meta, None)
    }

    /// Test-only seam for [`Db::send_order_to_kitchen_with_outbox`]: lets a
    /// test force the (kot_id, kot_outbox_id) pair used for each station
    /// ticket, in the same order `by_station`'s `BTreeMap` iterates
    /// (alphabetical by station code), instead of the crate's own random
    /// UUIDv7s. This exists solely so a test can force a deterministic
    /// mid-transaction collision (e.g. a duplicate `local_outbox` id on the
    /// second station's ticket, after the first station's `kot` row has
    /// already been written) and prove the whole transaction rolls back —
    /// something that cannot be exercised against real random ids.
    #[cfg(test)]
    pub(crate) fn send_order_to_kitchen_with_outbox_with_forced_ids(
        &mut self,
        order_id: &str,
        meta: &SendToKitchenMeta,
        forced_ids: Vec<(String, String)>,
    ) -> DbResult<Vec<Kot>> {
        self.send_order_to_kitchen_with_outbox_inner(order_id, meta, Some(forced_ids))
    }

    fn send_order_to_kitchen_with_outbox_inner(
        &mut self,
        order_id: &str,
        meta: &SendToKitchenMeta,
        forced_ids: Option<Vec<(String, String)>>,
    ) -> DbResult<Vec<Kot>> {
        let tx = self.connection_mut().transaction()?;
        let outlet_id = repo::require_sendable_order(&tx, order_id)?;

        let already_ticketed = repo::already_ticketed_order_item_ids(&tx, order_id)?;
        let unticketed_items: Vec<_> = repo::list_order_items_in_tx(&tx, order_id)?
            .into_iter()
            .filter(|item| !already_ticketed.contains(&item.id))
            .collect();

        // Group ticket lines by station code; an item present in more than
        // one station's routing appears on more than one group. Also track
        // any line that resolves to zero stations — see
        // `DbError::UnroutedKitchenItems` for why that rejects the whole
        // call rather than being skipped.
        let mut by_station: BTreeMap<String, Vec<KotTicketItem>> = BTreeMap::new();
        let mut unrouted: Vec<crate::model::UnroutedKitchenItem> = Vec::new();
        for item in &unticketed_items {
            let stations = repo::list_stations_for_menu_item(&tx, &item.menu_item_id)?;
            let ticket_item = repo::build_kot_ticket_item(&tx, item)?;
            if stations.is_empty() {
                unrouted.push(crate::model::UnroutedKitchenItem {
                    order_item_id: item.id.clone(),
                    name: ticket_item.name.clone(),
                });
                continue;
            }
            for station in stations {
                by_station
                    .entry(station.code)
                    .or_default()
                    .push(ticket_item.clone());
            }
        }

        // A *mixed* call — some lines routed, some not — is the defect this
        // guards against, and is rejected outright (see
        // `DbError::UnroutedKitchenItems`). An *all-unrouted* call (or a
        // call with zero unticketed lines at all) keeps its pre-existing,
        // already-correct `NothingToSendToKitchen` outcome below — this
        // fix narrows the gap the all-unrouted case never had, it does not
        // change that case's wire behaviour.
        if !unrouted.is_empty() && !by_station.is_empty() {
            return Err(DbError::UnroutedKitchenItems {
                order_id: order_id.to_string(),
                items: unrouted,
            });
        }

        if by_station.is_empty() {
            return Err(DbError::NothingToSendToKitchen {
                order_id: order_id.to_string(),
            });
        }

        if let Some(ids) = &forced_ids {
            assert_eq!(
                ids.len(),
                by_station.len(),
                "test bug: forced_ids must supply exactly one (kot_id, outbox_id) pair per station"
            );
        }

        let sequence = repo::next_kot_sequence(&tx, order_id)?;
        let mut created = Vec::with_capacity(by_station.len());
        for (i, (station_code, items)) in by_station.into_iter().enumerate() {
            // The number of stations affected is not knowable to the
            // caller ahead of routing resolution, so this crate mints
            // these ids itself (model::SendToKitchenMeta doc comment) —
            // unless a test forced deterministic ones (see
            // `send_order_to_kitchen_with_outbox_with_forced_ids`).
            let (kot_id, kot_outbox_id) = match &forced_ids {
                Some(ids) => ids[i].clone(),
                None => (
                    uuid::Uuid::now_v7().to_string(),
                    uuid::Uuid::now_v7().to_string(),
                ),
            };
            let kot = Kot {
                id: kot_id,
                order_id: order_id.to_string(),
                station: station_code,
                sequence,
                status: "NEW".to_string(),
                items_json: repo::kot_ticket_items_json(&items),
                created_by_device_id: meta.device_id.clone(),
                created_at: meta.occurred_at.clone(),
                updated_at: meta.occurred_at.clone(),
            };
            repo::insert_kot_in_tx(&tx, &kot)?;
            repo::insert_kot_created_outbox(
                &tx,
                &outlet_id,
                &kot,
                &items,
                &kot_outbox_id,
                &meta.occurred_at,
            )?;
            created.push(kot);
        }

        let sent_outbox_id = uuid::Uuid::now_v7().to_string();
        repo::insert_sent_to_kitchen_outbox(
            &tx,
            &outlet_id,
            order_id,
            &sent_outbox_id,
            &meta.occurred_at,
        )?;
        repo::stamp_order_sent_to_kitchen_if_earlier(&tx, order_id, &meta.occurred_at)?;

        tx.commit()?;
        Ok(created)
    }

    /// Announces the cancellation of already-ticketed order lines to the
    /// kitchen — the `#132` -> `#132-C` step of docs/spec/kitchen.md's
    /// change history, the cancellation counterpart to the `#132-A`
    /// addition path in [`Db::send_order_to_kitchen_with_outbox`].
    ///
    /// A `CANCELLED`-status flag on the *existing* ticket is not this: a
    /// cook working from a printed ticket has no way to observe a status
    /// column changing in SQLite, which is exactly why the spec calls for
    /// a new ticket. So this produces one brand-new `kot` row per station
    /// that had one of the cancelled lines on it — grouped by station, at
    /// the next `sequence` for the order, created directly with
    /// `status = 'CANCELLED'` (never transitioned into it, since nothing
    /// on this new ticket was ever being prepared) — each carrying only
    /// the cancelled lines that were actually on that station's ticket.
    ///
    /// Assumption (stated per the coordinator's instruction to proceed
    /// rather than stop on ambiguity): a line can only be cancelled once
    /// it has actually been sent to the kitchen (present on some earlier
    /// KOT for the order) and only once — this method does not re-derive
    /// order-line deletion or partial-quantity cancellation, neither of
    /// which docs/spec/kitchen.md's one-line example addresses.
    ///
    /// Writes every new `kot` row and its `KOTCreated` `local_outbox` row
    /// in the *same* SQLite transaction (ADR-007), exactly like the
    /// addition path. Returns `DbError::NothingToSendToKitchen` if
    /// `order_item_ids` is empty. Returns `DbError::NotFound("order_item")`
    /// if a requested id was never ticketed for this order (or its
    /// `order_item` row is gone) — this method never silently skips a
    /// cancellation it was explicitly asked to announce.
    pub fn cancel_kitchen_items_with_outbox(
        &mut self,
        order_id: &str,
        order_item_ids: &[String],
        meta: &SendToKitchenMeta,
    ) -> DbResult<Vec<Kot>> {
        if order_item_ids.is_empty() {
            return Err(DbError::NothingToSendToKitchen {
                order_id: order_id.to_string(),
            });
        }

        let tx = self.connection_mut().transaction()?;
        let outlet_id = repo::require_sendable_order(&tx, order_id)?;

        let mut by_station: BTreeMap<String, Vec<KotTicketItem>> = BTreeMap::new();
        for order_item_id in order_item_ids {
            let station_code =
                repo::find_ticketed_station_for_order_item(&tx, order_id, order_item_id)?
                    .ok_or(DbError::NotFound("order_item"))?;
            let item = repo::get_order_item_in_tx(&tx, order_item_id)?
                .ok_or(DbError::NotFound("order_item"))?;
            let ticket_item = repo::build_kot_ticket_item(&tx, &item)?;
            by_station.entry(station_code).or_default().push(ticket_item);
        }

        let sequence = repo::next_kot_sequence(&tx, order_id)?;
        let mut created = Vec::with_capacity(by_station.len());
        for (station_code, items) in by_station {
            let kot_id = uuid::Uuid::now_v7().to_string();
            let kot_outbox_id = uuid::Uuid::now_v7().to_string();
            let kot = Kot {
                id: kot_id,
                order_id: order_id.to_string(),
                station: station_code,
                sequence,
                // Created directly as CANCELLED: this ticket is the
                // announcement, not a ticket that was ever being worked.
                status: "CANCELLED".to_string(),
                items_json: repo::kot_ticket_items_json(&items),
                created_by_device_id: meta.device_id.clone(),
                created_at: meta.occurred_at.clone(),
                updated_at: meta.occurred_at.clone(),
            };
            repo::insert_kot_in_tx(&tx, &kot)?;
            repo::insert_kot_created_outbox(
                &tx,
                &outlet_id,
                &kot,
                &items,
                &kot_outbox_id,
                &meta.occurred_at,
            )?;
            created.push(kot);
        }

        // A cancellation round can itself complete the order-ready
        // derivation (e.g. the only still-active ticket was already
        // READY, and this cancellation removes the last blocker) — same
        // derivation as the status-transition path.
        if repo::order_is_kitchen_ready(&tx, order_id)?
            && repo::stamp_order_ready_if_applicable(&tx, order_id, &meta.occurred_at)?
        {
            let ready_outbox_id = uuid::Uuid::now_v7().to_string();
            repo::insert_order_ready_outbox(
                &tx,
                &outlet_id,
                order_id,
                &ready_outbox_id,
                &meta.occurred_at,
            )?;
        }

        tx.commit()?;
        Ok(created)
    }

    /// Transitions one KOT's status (NEW -> ACKNOWLEDGED -> PREPARING ->
    /// READY -> SERVED, plus CANCELLED from any non-terminal status),
    /// writing the `kot.status` update, a `kot_status_history` row and the
    /// `KOTStatusChanged` `local_outbox` row in the *same* SQLite
    /// transaction (ADR-007). `POST /kots/{kotId}/status` on the cloud side
    /// is documented (ADR-014 §4) as the only writer of `kot.status`; this
    /// is the corresponding single edge-side writer.
    ///
    /// Rejects an illegal transition with `DbError::IllegalKotStatusTransition`
    /// — never a silent no-op — and writes nothing.
    ///
    /// If this transition leaves every non-cancelled KOT on the order READY,
    /// also stamps the order `status = 'READY'` (unless it has already moved
    /// past READY) and writes an `OrderReady` `local_outbox` row, in the same
    /// transaction — the order-status derivation stays in this domain layer,
    /// not in a query a caller might forget to run.
    pub fn transition_kot_status_with_outbox(
        &mut self,
        kot_id: &str,
        new_status: &str,
        meta: &KotTransitionMeta,
    ) -> DbResult<()> {
        let tx = self.connection_mut().transaction()?;
        let (order_id, current_status) = repo::get_kot_status_for_transition(&tx, kot_id)?;

        if !repo::is_legal_kot_transition(&current_status, new_status) {
            return Err(DbError::IllegalKotStatusTransition {
                kot_id: kot_id.to_string(),
                from: current_status,
                to: new_status.to_string(),
            });
        }

        let outlet_id = repo::get_order_outlet_id(&tx, &order_id)?;

        repo::stamp_kot_status(&tx, kot_id, new_status, &meta.occurred_at)?;
        repo::insert_kot_status_history(
            &tx,
            &KotStatusHistoryEntry {
                id: meta.status_history_id.clone(),
                kot_id: kot_id.to_string(),
                status: new_status.to_string(),
                changed_by_device_id: meta.changed_by_device_id.clone(),
                // Edge's own clock (§50.1) — the Tauri command layer sources
                // this from the local machine, never from a KDS screen.
                changed_at: meta.occurred_at.clone(),
            },
        )?;
        repo::insert_kot_status_changed_outbox(
            &tx,
            &outlet_id,
            kot_id,
            &order_id,
            new_status,
            &meta.changed_by_device_id,
            &meta.outbox_id,
            &meta.occurred_at,
        )?;

        if repo::order_is_kitchen_ready(&tx, &order_id)?
            && repo::stamp_order_ready_if_applicable(&tx, &order_id, &meta.occurred_at)?
        {
            let ready_outbox_id = uuid::Uuid::now_v7().to_string();
            repo::insert_order_ready_outbox(
                &tx,
                &outlet_id,
                &order_id,
                &ready_outbox_id,
                &meta.occurred_at,
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Stations configured for an outlet (config, cloud→edge — see
    /// `repo::upsert_station`). Read surface for the POS and the LAN
    /// server.
    pub fn list_stations_for_outlet(&self, outlet_id: &str) -> DbResult<Vec<Station>> {
        repo::list_stations_for_outlet(self.connection(), outlet_id)
    }

    /// All KOTs (any status) for one order, oldest sequence first.
    pub fn list_kots_for_order(&self, order_id: &str) -> DbResult<Vec<Kot>> {
        repo::list_kots_for_order(self.connection(), order_id)
    }

    /// KOTs for an outlet, optionally narrowed to one station code — the
    /// query a KDS/expo screen or the LAN server needs to answer "what's on
    /// this station's pass right now".
    pub fn list_kots_for_outlet(&self, outlet_id: &str, station: Option<&str>) -> DbResult<Vec<Kot>> {
        repo::list_kots_for_outlet(self.connection(), outlet_id, station)
    }
}

/// Last-resort seal-on-drop (ADR-011). Without this, any exit path that does
/// not explicitly call [`Db::close`] or [`Db::shutdown_in_place`] — an early
/// `return`, a `?`, a panic unwind, or an application that simply never wires
/// a shutdown hook — leaves the decrypted SQLite file and its `-wal`/`-shm`
/// siblings on disk indefinitely, holding cached credential hashes in the
/// clear.
///
/// This is a safety net, not the intended path: `Drop` cannot return an error,
/// so a sealing failure here can only be logged. Callers that need to know
/// whether the seal succeeded must call [`Db::close`].
impl Drop for Db {
    fn drop(&mut self) {
        // No-op when close()/shutdown_in_place() already ran — that is what
        // makes the explicit path and this fallback safe to combine.
        if self.conn.is_none() {
            return;
        }

        if let Err(e) = self.shutdown_in_place() {
            // Deliberately not a panic: unwinding out of Drop during another
            // unwind aborts the process, which would be a worse outcome than
            // a logged failure. The plaintext left behind is recovered by
            // crypto::recover_crash_leftovers on the next open.
            eprintln!(
                "edge database: sealing on drop failed ({e}); plaintext may remain at {}",
                self.plaintext_path.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NewOrder, NewOrderItem, NewOutboxEntry};
    use tempfile::tempdir;

    /// The plaintext artifacts that must never outlive an open handle.
    fn plaintext_artifacts(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("read app data dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().to_string())
            .filter(|n| n != "edge.db.enc")
            .collect();
        names.sort();
        names
    }

    /// Dropping a handle without calling close() must still seal and wipe.
    /// This is the regression test for the POS leaving a decrypted database
    /// on disk after every exit.
    #[test]
    fn dropping_without_close_seals_and_wipes_plaintext() {
        let dir = tempdir().expect("tempdir");
        let sealed = dir.path().join("edge.db.enc");
        let plaintext = dir.path().join("edge.db");
        {
            let db = Db::open(&sealed, &plaintext, EncryptionKey::new([7u8; 32])).expect("open");
            // Touch the database so there is real committed content to seal.
            db.connection()
                .execute_batch("CREATE TABLE IF NOT EXISTS drop_probe (x INTEGER)")
                .expect("write");
            // No close() — the handle goes out of scope here.
        }

        assert!(sealed.exists(), "sealed file must exist after drop");
        assert_eq!(
            plaintext_artifacts(dir.path()),
            Vec::<String>::new(),
            "drop must leave nothing but edge.db.enc on disk"
        );
    }

    /// Drop after an explicit close must be a harmless no-op, not a second
    /// seal attempt against an already-wiped plaintext file.
    #[test]
    fn close_then_drop_is_safe() {
        let dir = tempdir().expect("tempdir");
        let sealed = dir.path().join("edge.db.enc");
        let plaintext = dir.path().join("edge.db");
        // EncryptionKey is deliberately not Clone (it is key material), so
        // each open constructs the same bytes afresh.
        let db = Db::open(&sealed, &plaintext, EncryptionKey::new([9u8; 32])).expect("open");
        db.close().expect("explicit close must succeed");
        // `db` was consumed by close(); its Drop ran immediately afterwards
        // and must not have errored or resurrected the plaintext file.

        assert!(sealed.exists());
        assert_eq!(plaintext_artifacts(dir.path()), Vec::<String>::new());

        // The sealed file must still be openable, i.e. close+drop did not
        // corrupt it by sealing twice.
        let reopened = Db::open(&sealed, &plaintext, EncryptionKey::new([9u8; 32]))
            .expect("reopen after close+drop");
        reopened.close().expect("second close");
    }

    /// shutdown_in_place is the path the POS exit hook uses; calling it twice
    /// must be safe because Drop will call it again.
    #[test]
    fn shutdown_in_place_is_idempotent() {
        let dir = tempdir().expect("tempdir");
        let sealed = dir.path().join("edge.db.enc");
        let plaintext = dir.path().join("edge.db");
        let key = EncryptionKey::new([3u8; 32]);

        let mut db = Db::open(&sealed, &plaintext, key).expect("open");
        assert!(!db.is_shut_down());

        db.shutdown_in_place().expect("first shutdown");
        assert!(db.is_shut_down());
        db.shutdown_in_place().expect("second shutdown is a no-op");

        assert_eq!(plaintext_artifacts(dir.path()), Vec::<String>::new());
    }

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
            taxes_paise: 0,
            total_paise: 0,
            source: "POS".to_string(),
            external_order_id: None,
            payment_status: "UNPAID".to_string(),
            payment_source: None,
            confirmed_at: None,
            source_payload_json: None,
            schema_version: 1,
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

    fn order_confirmed_meta(order_id: &str) -> OrderConfirmedMeta {
        OrderConfirmedMeta {
            outbox_id: format!("outbox-confirm-{order_id}"),
            occurred_at: "2026-08-08T12:00:00Z".to_string(),
            confirmed_at: "2026-08-08T11:59:59Z".to_string(),
        }
    }

    #[test]
    fn confirm_order_stamps_status_and_writes_outbox_atomically() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");

        let order = sample_order("order-confirm-1", "outlet-1", "device-1");
        db.create_order_with_outbox(&order, &[], &sample_outbox("order-confirm-1"))
            .expect("create draft order");

        let meta = order_confirmed_meta("order-confirm-1");
        db.confirm_order_with_outbox("order-confirm-1", &meta)
            .expect("confirm order");

        let stored = db
            .get_order("order-confirm-1")
            .unwrap()
            .expect("order exists");
        assert_eq!(stored.status, "CONFIRMED");
        assert_eq!(stored.confirmed_at.as_deref(), Some("2026-08-08T11:59:59Z"));
        assert_eq!(stored.version, 2, "version must bump on confirmation");

        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        let event = pending
            .iter()
            .find(|e| e.aggregate_id == "order-confirm-1" && e.event_type == "OrderConfirmed")
            .expect("OrderConfirmed event must exist");
        assert_eq!(event.event_type, "OrderConfirmed");
    }

    /// The caller supplies only ids/timestamps, never the event description
    /// — this proves the emitted payload matches `OrderConfirmedEventSchema`
    /// exactly and is derived, not caller-described.
    #[test]
    fn confirm_order_outbox_payload_is_derived_and_cannot_be_spoofed() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");

        let order = sample_order("order-confirm-payload", "outlet-1", "device-1");
        db.create_order_with_outbox(&order, &[], &sample_outbox("order-confirm-payload"))
            .expect("create draft order");

        let meta = order_confirmed_meta("order-confirm-payload");
        db.confirm_order_with_outbox("order-confirm-payload", &meta)
            .expect("confirm order");

        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        let event = pending
            .iter()
            .find(|e| e.aggregate_id == "order-confirm-payload" && e.event_type == "OrderConfirmed")
            .expect("OrderConfirmed event must exist");

        let payload: serde_json::Value = serde_json::from_str(&event.payload_json).unwrap();
        assert_eq!(payload["event_type"], "OrderConfirmed");
        assert_eq!(payload["outlet_id"], "outlet-1");
        assert_eq!(payload["schema_version"], 1);
        assert_eq!(payload["data"]["order_id"], "order-confirm-payload");
        assert_eq!(payload["data"]["confirmed_at"], "2026-08-08T11:59:59Z");
        // Envelope must contain exactly the frozen shape — no extra fields
        // a caller could have smuggled in.
        assert_eq!(
            payload.as_object().unwrap().len(),
            6,
            "envelope must be exactly event_id/event_type/occurred_at/outlet_id/schema_version/data"
        );
        assert_eq!(
            payload["data"].as_object().unwrap().len(),
            2,
            "data must be exactly order_id/confirmed_at"
        );
    }

    #[test]
    fn confirm_order_rejects_non_draft_order_and_writes_nothing() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");

        let mut order = sample_order("order-confirm-reject", "outlet-1", "device-1");
        order.status = "CONFIRMED".to_string();
        db.create_order_with_outbox(&order, &[], &sample_outbox("order-confirm-reject"))
            .expect("create already-confirmed order");

        let meta = order_confirmed_meta("order-confirm-reject");
        let result = db.confirm_order_with_outbox("order-confirm-reject", &meta);

        assert!(matches!(result, Err(DbError::OrderNotConfirmable { .. })));

        let stored = db
            .get_order("order-confirm-reject")
            .unwrap()
            .expect("order exists");
        assert_eq!(stored.status, "CONFIRMED");
        assert!(
            stored.confirmed_at.is_none(),
            "confirmed_at must be untouched by the rejected attempt"
        );
        assert_eq!(stored.version, 1, "version must not bump on rejection");

        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        assert!(pending
            .iter()
            .all(|e| e.aggregate_id != "order-confirm-reject" || e.event_type != "OrderConfirmed"));
    }

    /// Rollback guarantee: a failure partway through the transaction (a
    /// colliding outbox id, so `insert_order_confirmed_outbox` fails after
    /// `stamp_order_confirmed` has already run) must leave the order row in
    /// DRAFT with no outbox row — the status stamp must not survive without
    /// its outbox row, or vice versa.
    #[test]
    fn failed_confirm_order_transaction_leaves_draft_status_and_no_outbox_row() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");

        let order = sample_order("order-confirm-fail", "outlet-1", "device-1");
        let colliding_id = "colliding-outbox-confirm".to_string();
        let create_outbox = NewOutboxEntry {
            id: colliding_id.clone(),
            ..sample_outbox("order-confirm-fail")
        };
        db.create_order_with_outbox(&order, &[], &create_outbox)
            .expect("create draft order");

        let colliding_meta = OrderConfirmedMeta {
            outbox_id: colliding_id,
            occurred_at: "2026-08-08T12:00:00Z".to_string(),
            confirmed_at: "2026-08-08T11:59:59Z".to_string(),
        };
        let result = db.confirm_order_with_outbox("order-confirm-fail", &colliding_meta);
        assert!(
            result.is_err(),
            "colliding outbox id must fail the transaction"
        );

        let stored = db
            .get_order("order-confirm-fail")
            .unwrap()
            .expect("order exists");
        assert_eq!(
            stored.status, "DRAFT",
            "rolled-back confirm must leave the order in DRAFT"
        );
        assert!(
            stored.confirmed_at.is_none(),
            "confirmed_at must not be stamped by a rolled-back transaction"
        );
        assert_eq!(stored.version, 1, "version must not bump on rollback");

        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        assert!(pending
            .iter()
            .all(|e| e.aggregate_id != "order-confirm-fail" || e.event_type != "OrderConfirmed"));
    }

    /// The regression this crate exists to fix (task T14, docs/retro.md P0):
    /// a DRAFT order's shape must stay editable for its whole DRAFT
    /// lifetime, not just before it existed.
    #[test]
    fn update_order_shape_persists_order_type_and_table_id() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");

        let order = sample_order("order-shape-1", "outlet-1", "device-1");
        db.create_order_with_outbox(&order, &[], &sample_outbox("order-shape-1"))
            .expect("create draft order");

        db.update_order_shape_with_outbox(
            "order-shape-1",
            "TAKEAWAY",
            None,
            "2026-08-11T09:00:00Z",
            None,
        )
        .expect("update shape");

        let stored = db
            .get_order("order-shape-1")
            .unwrap()
            .expect("order exists");
        assert_eq!(stored.order_type, "TAKEAWAY");
        assert_eq!(stored.table_id, None);
        assert_eq!(stored.version, 2, "version must bump on shape change");
        assert_eq!(stored.updated_at, "2026-08-11T09:00:00Z");
    }

    #[test]
    fn update_order_shape_persists_dine_in_table_selection() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");

        let order = sample_order("order-shape-2", "outlet-1", "device-1");
        db.create_order_with_outbox(&order, &[], &sample_outbox("order-shape-2"))
            .expect("create draft order");

        db.update_order_shape_with_outbox(
            "order-shape-2",
            "DINE_IN",
            Some("table-1"),
            "2026-08-11T09:00:00Z",
            None,
        )
        .expect("update shape");

        let stored = db
            .get_order("order-shape-2")
            .unwrap()
            .expect("order exists");
        assert_eq!(stored.order_type, "DINE_IN");
        assert_eq!(stored.table_id.as_deref(), Some("table-1"));
    }

    #[test]
    fn update_order_shape_rejects_non_draft_order_and_writes_nothing() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");

        let mut order = sample_order("order-shape-reject", "outlet-1", "device-1");
        order.status = "CONFIRMED".to_string();
        db.create_order_with_outbox(&order, &[], &sample_outbox("order-shape-reject"))
            .expect("create already-confirmed order");

        let result = db.update_order_shape_with_outbox(
            "order-shape-reject",
            "TAKEAWAY",
            None,
            "2026-08-11T09:00:00Z",
            None,
        );

        assert!(matches!(result, Err(DbError::OrderNotAmendable { .. })));

        let stored = db
            .get_order("order-shape-reject")
            .unwrap()
            .expect("order exists");
        assert_eq!(
            stored.order_type, "DINE_IN",
            "order_type must be untouched by the rejected attempt"
        );
        assert_eq!(stored.version, 1, "version must not bump on rejection");
    }

    #[test]
    fn update_order_shape_rejects_nonexistent_order() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");

        let result = db.update_order_shape_with_outbox(
            "does-not-exist",
            "TAKEAWAY",
            None,
            "2026-08-11T09:00:00Z",
            None,
        );

        assert!(matches!(result, Err(DbError::NotFound("order"))));
    }

    /// When a corrected `OrderCreated` payload is supplied and that event is
    /// still unpublished, the queued row is corrected in place rather than
    /// a second event being written — see the doc comment on
    /// `Db::update_order_shape_with_outbox` for why.
    #[test]
    fn update_order_shape_corrects_unpublished_order_created_payload_in_place() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");

        let order = sample_order("order-shape-payload", "outlet-1", "device-1");
        db.create_order_with_outbox(&order, &[], &sample_outbox("order-shape-payload"))
            .expect("create draft order");

        db.update_order_shape_with_outbox(
            "order-shape-payload",
            "TAKEAWAY",
            None,
            "2026-08-11T09:00:00Z",
            Some(r#"{"event_type":"OrderCreated","data":{"order":{"order_type":"TAKEAWAY"}}}"#),
        )
        .expect("update shape");

        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        let event = pending
            .iter()
            .find(|e| e.aggregate_id == "order-shape-payload" && e.event_type == "OrderCreated")
            .expect("OrderCreated event must still exist");
        assert_eq!(
            event.payload_json,
            r#"{"event_type":"OrderCreated","data":{"order":{"order_type":"TAKEAWAY"}}}"#,
            "unpublished OrderCreated payload must be corrected in place"
        );
        // Still exactly one OrderCreated row — a correction, not a second event.
        assert_eq!(
            pending
                .iter()
                .filter(|e| e.aggregate_id == "order-shape-payload"
                    && e.event_type == "OrderCreated")
                .count(),
            1
        );
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
            // The crash. Note this is NOT the same as letting `db` drop:
            // `Drop` now seals and wipes (ADR-011), so a plain drop would
            // leave no crash artifacts to recover from.
            db.simulate_crash_for_tests();
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
            let mut db =
                Db::open(&sealed, &plaintext, EncryptionKey::new(key_bytes)).expect("first open");
            // The crash — see the note in the test above on why this is not
            // simply a drop.
            db.simulate_crash_for_tests();
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
    /// SCOPE (contracts 0.2.4 update): `0004_order_canonical_fields.sql` gave
    /// the `"order"` table columns for `source`, `external_order_id`,
    /// `payment_status`, `payment_source`, `confirmed_at`,
    /// `source_payload_json` and `schema_version`, and renamed `tax_paise`
    /// to `taxes_paise` to match the wire name exactly. The full
    /// order-level round trip (including those fields) now lives in
    /// [`order_fixture_round_trips_byte_for_byte_through_public_api`] below;
    /// this test remains as the item-level reference case it always was.
    /// `packaging_paise`, `delivery_charge_paise`, `aggregator_discount_paise`,
    /// `merchant_discount_paise`, `customer`, `delivery_address`, `rider`
    /// and `preparation_time_minutes` still have no column — deliberately
    /// deferred to Milestone 2/6 per the ADR-011 0.2.4 addendum — and are
    /// synthesized at a fixed value by the order-level test below.
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
            taxes_paise: 0,
            total_paise: 0,
            source: "POS".to_string(),
            external_order_id: None,
            payment_status: "UNPAID".to_string(),
            payment_source: None,
            confirmed_at: None,
            source_payload_json: None,
            schema_version: 1,
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

    /// The order-level counterpart to
    /// [`order_item_fixture_round_trips_byte_for_byte_through_public_api`],
    /// added for contracts 0.2.4 (`0004_order_canonical_fields.sql`). Writes
    /// `fixtures/order.json` — the order envelope, its one line and that
    /// line's modifier — through this crate's public API, reads it back,
    /// and re-serializes to the `CanonicalOrderSchema` wire shape.
    ///
    /// Eight fields have no column at all as of 0.2.4 (`packaging_paise`,
    /// `delivery_charge_paise`, `aggregator_discount_paise`,
    /// `merchant_discount_paise`, `customer`, `delivery_address`, `rider`,
    /// `preparation_time_minutes` — deferred to Milestone 2/6 per the
    /// ADR-011 0.2.4 addendum). This test pins their synthesized values
    /// exactly rather than merely asserting they are falsy, so a later
    /// milestone that starts persisting one of them fails this test instead
    /// of drifting quietly.
    ///
    /// WHAT THIS TEST CLAIMS, AND WHAT IT DOES NOT. The order and its line go
    /// through the public `Db` API. The line's *modifier* does not: no public
    /// writer reaches `order_item_modifier` for an order in this state, because
    /// `add_order_item_with_outbox` is DRAFT-only and the fixture's order is
    /// `SENT_TO_KITCHEN`. That leg is written through the in-crate `repo::`
    /// path instead. So for modifiers this asserts **storage fidelity — that
    /// the schema can hold and return the contract's shape — and not public-API
    /// coverage.** The API-coverage claim for modifiers belongs to
    /// [`order_item_fixture_round_trips_byte_for_byte_through_public_api`],
    /// which does drive the public DRAFT amendment path. Read together they
    /// cover both; read alone, neither covers both.
    #[test]
    fn order_fixture_round_trips_byte_for_byte_through_public_api() {
        let fixture_text = include_str!("../../../packages/contracts/fixtures/order.json");
        let fixture: serde_json::Value =
            serde_json::from_str(fixture_text).expect("fixture must be valid JSON");
        let fixture_item = fixture["items"][0].clone();

        let mut db = Db::open_in_memory_for_tests().expect("open");

        let outlet_id = fixture["outlet_id"].as_str().unwrap().to_string();
        let table_id = fixture["table_id"].as_str().map(str::to_string);
        let device_id = "device-order-fixture".to_string();
        seed_outlet_and_device(&db, &outlet_id, &device_id);

        let menu_item_id = fixture_item["menu_item_id"].as_str().unwrap().to_string();
        let unit_price_paise = fixture_item["unit_price_paise"].as_i64().unwrap();
        repo::upsert_menu_category(
            db.connection(),
            &model::MenuCategory {
                id: "category-order-fixture".to_string(),
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
                category_id: "category-order-fixture".to_string(),
                name: "Fixture Item".to_string(),
                base_price_paise: unit_price_paise,
                is_available: true,
                config_version: 1,
            },
        )
        .expect("seed menu item");

        let order_id = fixture["holler_order_id"].as_str().unwrap().to_string();
        let item_id = fixture_item["id"].as_str().unwrap().to_string();
        let item_created_at = fixture["timestamps"]["created_at"]
            .as_str()
            .unwrap()
            .to_string();

        let source_payload_json = match &fixture["source_payload"] {
            serde_json::Value::Null => None,
            other => Some(serde_json::to_string(other).unwrap()),
        };

        let new_order = NewOrder {
            id: order_id.clone(),
            outlet_id: outlet_id.clone(),
            device_id: device_id.clone(),
            order_type: fixture["order_type"].as_str().unwrap().to_string(),
            status: fixture["status"].as_str().unwrap().to_string(),
            table_id: table_id.clone(),
            subtotal_paise: fixture["subtotal_paise"].as_i64().unwrap(),
            discount_paise: fixture["discount_paise"].as_i64().unwrap(),
            taxes_paise: fixture["taxes_paise"].as_i64().unwrap(),
            total_paise: fixture["total_paise"].as_i64().unwrap(),
            source: fixture["source"].as_str().unwrap().to_string(),
            external_order_id: fixture["external_order_id"].as_str().map(str::to_string),
            payment_status: fixture["payment_status"].as_str().unwrap().to_string(),
            payment_source: fixture["payment_source"].as_str().map(str::to_string),
            confirmed_at: fixture["timestamps"]["confirmed_at"]
                .as_str()
                .map(str::to_string),
            source_payload_json,
            schema_version: fixture["schema_version"].as_i64().unwrap(),
            created_at: item_created_at.clone(),
            updated_at: fixture["timestamps"]["updated_at"]
                .as_str()
                .unwrap()
                .to_string(),
        };

        let new_item = NewOrderItem {
            id: item_id.clone(),
            order_id: order_id.clone(),
            menu_item_id: menu_item_id.clone(),
            variant_id: fixture_item["variant_id"].as_str().map(str::to_string),
            quantity: fixture_item["quantity"].as_i64().unwrap(),
            unit_price_paise,
            line_total_paise: fixture_item["line_total_paise"].as_i64().unwrap(),
            notes: fixture_item["notes"].as_str().map(str::to_string),
            created_at: item_created_at,
        };

        // create_order_with_outbox is the only entry point that writes the
        // order row itself; it trusts the caller's line_total_paise (unlike
        // add_order_item_with_outbox, which recomputes it) because it is
        // the initial creation of the order, not an amendment.
        db.create_order_with_outbox(&new_order, &[new_item], &sample_outbox(&order_id))
            .expect("persist fixture order + item through the public API");

        // order_item_modifier has no writer on `Db` outside
        // add_order_item_with_outbox (which requires DRAFT and recomputes
        // totals) — the fixture's order is SENT_TO_KITCHEN, so its modifier
        // is written directly through the same in-crate transaction pattern
        // every other writer in this module uses (`repo::` functions are
        // `pub(crate)`, reachable here because `tests` is a descendant
        // module of the crate root that defines `Db`).
        {
            let fixture_modifiers = fixture_item["modifiers"].as_array().unwrap().clone();
            let tx = db.connection_mut().transaction().expect("begin tx");
            for (i, m) in fixture_modifiers.iter().enumerate() {
                repo::insert_order_item_modifier(
                    &tx,
                    &OrderItemModifier {
                        id: format!("fixture-order-modifier-{i}"),
                        order_item_id: item_id.clone(),
                        modifier_id: m["modifier_id"].as_str().unwrap().to_string(),
                        group_name: m["group_name"].as_str().unwrap().to_string(),
                        option_name: m["option_name"].as_str().unwrap().to_string(),
                        price_delta_paise: m["price_delta_paise"].as_i64().unwrap(),
                        created_at: new_order.created_at.clone(),
                    },
                )
                .expect("insert fixture modifier");
            }
            tx.commit().expect("commit modifier tx");
        }

        // Read back through the public API only.
        let stored_order = db.get_order(&order_id).unwrap().expect("order exists");
        let stored_items = repo::list_order_items(db.connection(), &order_id).unwrap();
        assert_eq!(stored_items.len(), 1);
        let stored_item = &stored_items[0];
        let stored_modifiers = repo::list_order_item_modifiers(db.connection(), &item_id).unwrap();

        let reconstructed_item = repo::item_json(
            &stored_item.id,
            &stored_item.menu_item_id,
            stored_item.variant_id.as_deref(),
            stored_item.quantity,
            stored_item.unit_price_paise,
            stored_item.line_total_paise,
            stored_item.notes.as_deref(),
            &stored_modifiers,
        );

        let reconstructed_source_payload: serde_json::Value = match &stored_order
            .source_payload_json
        {
            Some(text) => serde_json::from_str(text).expect("stored source_payload is valid JSON"),
            None => serde_json::Value::Null,
        };

        let reconstructed = serde_json::json!({
            "holler_order_id": stored_order.id,
            "external_order_id": stored_order.external_order_id,
            "source": stored_order.source,
            "outlet_id": stored_order.outlet_id,
            "order_type": stored_order.order_type,
            "status": stored_order.status,
            "table_id": stored_order.table_id,
            // display_number: the column landed at contracts 0.4.0 (ADR-016)
            // but NOTHING MINTS ONE YET. Synthesized as null, and pinned below
            // exactly like the deferred M6 fields, so the Milestone 3 track
            // that adds per-outlet short-number minting fails this assertion
            // instead of drifting quietly past it.
            //
            // Until that lands, a printed KOT still shows the raw UUID — the
            // defect docs/backlog-m2.md raised. The contract is a prerequisite
            // for the fix, not the fix.
            "display_number": serde_json::Value::Null,
            // Deferred to Milestone 6 (ADR-011 0.2.4 addendum) — no column
            // exists yet; synthesized rather than persisted.
            "customer": serde_json::Value::Null,
            "delivery_address": serde_json::Value::Null,
            "items": [reconstructed_item],
            "subtotal_paise": stored_order.subtotal_paise,
            "discount_paise": stored_order.discount_paise,
            "packaging_paise": 0,
            "delivery_charge_paise": 0,
            "taxes_paise": stored_order.taxes_paise,
            "aggregator_discount_paise": 0,
            "merchant_discount_paise": 0,
            "total_paise": stored_order.total_paise,
            "payment_status": stored_order.payment_status,
            "payment_source": stored_order.payment_source,
            // Deferred to Milestone 2.
            "preparation_time_minutes": serde_json::Value::Null,
            // Deferred to Milestone 6.
            "rider": serde_json::Value::Null,
            "timestamps": {
                "created_at": stored_order.created_at,
                "confirmed_at": stored_order.confirmed_at,
                "updated_at": stored_order.updated_at,
            },
            "source_payload": reconstructed_source_payload,
            "schema_version": stored_order.schema_version,
        });

        // Pin the eight deferred fields' synthesized values exactly, so a
        // later milestone that starts persisting one of them fails this
        // assertion instead of drifting quietly (per the task's explicit
        // instruction not to weaken this to "absent or falsy").
        assert_eq!(reconstructed["packaging_paise"], serde_json::json!(0));
        assert_eq!(reconstructed["delivery_charge_paise"], serde_json::json!(0));
        assert_eq!(
            reconstructed["aggregator_discount_paise"],
            serde_json::json!(0)
        );
        assert_eq!(
            reconstructed["merchant_discount_paise"],
            serde_json::json!(0)
        );
        assert_eq!(reconstructed["customer"], serde_json::Value::Null);
        assert_eq!(reconstructed["delivery_address"], serde_json::Value::Null);
        assert_eq!(reconstructed["rider"], serde_json::Value::Null);
        assert_eq!(
            reconstructed["preparation_time_minutes"],
            serde_json::Value::Null
        );

        let reconstructed_bytes = serde_json::to_string(&reconstructed).unwrap();
        let fixture_bytes = serde_json::to_string(&fixture).unwrap();
        assert_eq!(
            reconstructed_bytes, fixture_bytes,
            "the full CanonicalOrder envelope must round-trip the fixture byte-for-byte, \
             modulo the eight fields pinned above that have no column as of contracts 0.2.4"
        );
    }

    // ---------------------------------------------------- Milestone 2: kitchen --

    fn seed_station(db: &Db, id: &str, outlet_id: &str, code: &str) {
        repo::upsert_station(
            db.connection(),
            &model::Station {
                id: id.to_string(),
                outlet_id: outlet_id.to_string(),
                code: code.to_string(),
                name: code.to_string(),
                sort_order: 0,
                is_active: true,
                config_version: 1,
            },
        )
        .expect("seed station");
    }

    fn route_item_to_stations(db: &Db, menu_item_id: &str, station_ids: &[String]) {
        repo::replace_menu_item_stations(db.connection(), menu_item_id, station_ids, 1)
            .expect("route item to stations");
    }

    fn confirm_for_kitchen(db: &mut Db, order_id: &str) {
        db.confirm_order_with_outbox(order_id, &order_confirmed_meta(order_id))
            .expect("confirm order before sending to kitchen");
    }

    fn send_meta(device_id: &str, occurred_at: &str) -> model::SendToKitchenMeta {
        model::SendToKitchenMeta {
            device_id: device_id.to_string(),
            occurred_at: occurred_at.to_string(),
        }
    }

    /// The central multi-station proof required by the task: a single item
    /// routed to two stations (a thali hitting MAIN_KITCHEN and TANDOOR)
    /// must produce two `kot` rows — one per station — never one ticket
    /// for the whole order, and the `kot` rows plus their `KOTCreated`
    /// outbox events must all land in the same transaction.
    #[test]
    fn multi_station_routing_produces_multiple_kots_atomically() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");

        seed_station(&db, "station-main", "outlet-1", "MAIN_KITCHEN");
        seed_station(&db, "station-tandoor", "outlet-1", "TANDOOR");
        route_item_to_stations(
            &db,
            &menu_item_id,
            &[
                "station-main".to_string(),
                "station-tandoor".to_string(),
            ],
        );

        let order = sample_order("order-thali", "outlet-1", "device-1");
        let item = sample_order_item("item-thali", "order-thali", &menu_item_id, 30000);
        db.create_order_with_outbox(&order, &[item], &sample_outbox("order-thali"))
            .expect("create draft order with thali item");
        confirm_for_kitchen(&mut db, "order-thali");

        let created = db
            .send_order_to_kitchen_with_outbox(
                "order-thali",
                &send_meta("device-1", "2026-08-09T10:00:00Z"),
            )
            .expect("send to kitchen");

        assert_eq!(created.len(), 2, "one ticket per routed station");
        let mut stations: Vec<&str> = created.iter().map(|k| k.station.as_str()).collect();
        stations.sort_unstable();
        assert_eq!(stations, vec!["MAIN_KITCHEN", "TANDOOR"]);
        assert!(created.iter().all(|k| k.sequence == 1));
        assert!(created.iter().all(|k| k.status == "NEW"));

        let stored = repo::list_kots_for_order(db.connection(), "order-thali").unwrap();
        assert_eq!(stored.len(), 2);

        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        let kot_created_count = pending
            .iter()
            .filter(|e| e.event_type == "KOTCreated" && e.aggregate_id != "order-thali")
            .count();
        assert_eq!(
            kot_created_count, 2,
            "one KOTCreated outbox event per station ticket"
        );
        assert!(
            pending
                .iter()
                .any(|e| e.event_type == "SentToKitchen" && e.aggregate_id == "order-thali"),
            "SentToKitchen must also be emitted"
        );

        let stored_order = db.get_order("order-thali").unwrap().expect("order exists");
        assert_eq!(stored_order.status, "SENT_TO_KITCHEN");
    }

    /// Regression for docs/backlog-m2.md Track A / docs/m3-planning.md §2
    /// Track A: a mixed order — one routed item, one unrouted item — used to
    /// send "successfully" with the unrouted line silently `continue`d past
    /// (the guard only fired when *every* line was unrouted). This asserts
    /// the whole call now rejects, names the unrouted item, and — critically
    /// — writes zero `kot` rows for *either* line, so the routed item never
    /// reaches the kitchen half-sent while the order silently drops the rest.
    #[test]
    fn mixed_order_with_one_unrouted_line_rejects_and_writes_no_kots() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (category_id, routed_item_id, _) = seed_menu(&db, "outlet-1");

        // A second menu item that is never routed to any station — the
        // config gap that used to be swallowed.
        let unrouted_item_id = "item-unrouted".to_string();
        repo::upsert_menu_item(
            db.connection(),
            &model::MenuItem {
                id: unrouted_item_id.clone(),
                outlet_id: "outlet-1".to_string(),
                category_id: category_id.clone(),
                name: "Mystery Side".to_string(),
                base_price_paise: 8000,
                is_available: true,
                config_version: 1,
            },
        )
        .expect("seed unrouted menu item");

        seed_station(&db, "station-main", "outlet-1", "MAIN_KITCHEN");
        route_item_to_stations(&db, &routed_item_id, &["station-main".to_string()]);
        // Deliberately no `route_item_to_stations` call for `unrouted_item_id`.

        let order = sample_order("order-mixed", "outlet-1", "device-1");
        let routed_line = sample_order_item("item-routed", "order-mixed", &routed_item_id, 25000);
        let unrouted_line =
            sample_order_item("item-mystery", "order-mixed", &unrouted_item_id, 8000);
        db.create_order_with_outbox(
            &order,
            &[routed_line, unrouted_line],
            &sample_outbox("order-mixed"),
        )
        .expect("create draft order with mixed lines");
        confirm_for_kitchen(&mut db, "order-mixed");

        let result = db.send_order_to_kitchen_with_outbox(
            "order-mixed",
            &send_meta("device-1", "2026-08-09T10:00:00Z"),
        );

        match result {
            Err(DbError::UnroutedKitchenItems { order_id, items }) => {
                assert_eq!(order_id, "order-mixed");
                assert_eq!(items.len(), 1, "only the unrouted line is named");
                assert_eq!(items[0].order_item_id, "item-mystery");
                assert_eq!(items[0].name, "Mystery Side");
            }
            other => panic!("expected UnroutedKitchenItems, got {other:?}"),
        }

        // The routed item must NOT have been ticketed either — a partial
        // send that tells nobody is exactly the defect this guards against.
        let stored = repo::list_kots_for_order(db.connection(), "order-mixed").unwrap();
        assert!(
            stored.is_empty(),
            "no kot row for either line when any line is unrouted"
        );
        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        assert!(
            !pending
                .iter()
                .any(|e| e.event_type == "KOTCreated" || e.event_type == "SentToKitchen"),
            "no KOTCreated/SentToKitchen outbox rows on a rejected send"
        );
        let stored_order = db
            .get_order("order-mixed")
            .unwrap()
            .expect("order still exists");
        assert_ne!(
            stored_order.status, "SENT_TO_KITCHEN",
            "order status must not advance on a rejected send"
        );
    }

    /// The all-unrouted case was already correct before this Track A fix
    /// (the guard at the bottom of the routing loop fired whenever
    /// `by_station` was empty) and must keep its exact outcome —
    /// `NothingToSendToKitchen`, not the new `UnroutedKitchenItems` — since
    /// only the *mixed* case was ever silent. Guards against a fix for the
    /// mixed case accidentally widening to change this one's wire contract.
    #[test]
    fn all_unrouted_order_keeps_nothing_to_send_to_kitchen_not_unrouted_items() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (category_id, _, _) = seed_menu(&db, "outlet-1");

        let unrouted_item_id = "item-unrouted-only".to_string();
        repo::upsert_menu_item(
            db.connection(),
            &model::MenuItem {
                id: unrouted_item_id.clone(),
                outlet_id: "outlet-1".to_string(),
                category_id: category_id.clone(),
                name: "Service Charge".to_string(),
                base_price_paise: 5000,
                is_available: true,
                config_version: 1,
            },
        )
        .expect("seed unrouted-only menu item");
        // Deliberately no station routing for this item at all.

        let order = sample_order("order-all-unrouted", "outlet-1", "device-1");
        let line = sample_order_item("item-only", "order-all-unrouted", &unrouted_item_id, 5000);
        db.create_order_with_outbox(&order, &[line], &sample_outbox("order-all-unrouted"))
            .expect("create draft order");
        confirm_for_kitchen(&mut db, "order-all-unrouted");

        let result = db.send_order_to_kitchen_with_outbox(
            "order-all-unrouted",
            &send_meta("device-1", "2026-08-09T10:00:00Z"),
        );

        assert!(
            matches!(result, Err(DbError::NothingToSendToKitchen { .. })),
            "expected NothingToSendToKitchen, got {result:?}"
        );
    }

    /// The pre-flight guard: sending an order to the kitchen while it is
    /// not in a sendable status must produce zero `kot` rows and zero
    /// `KOTCreated`/`SentToKitchen` outbox rows. This exercises the guard
    /// at the top of the call, before any station routing has even begun —
    /// it does NOT exercise the transactional-outbox rollback partway
    /// through routing; see
    /// `failed_send_to_kitchen_mid_transaction_leaves_no_kot_or_outbox_rows_at_all`
    /// below for that.
    #[test]
    fn send_to_kitchen_rejects_non_sendable_order_and_writes_nothing() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");

        let order_draft = sample_order("order-fail-draft", "outlet-1", "device-1");
        let item_draft =
            sample_order_item("item-fail-draft", "order-fail-draft", &menu_item_id, 10000);
        db.create_order_with_outbox(
            &order_draft,
            &[item_draft],
            &sample_outbox("order-fail-draft"),
        )
        .expect("create draft order");

        let result = db.send_order_to_kitchen_with_outbox(
            "order-fail-draft",
            &send_meta("device-1", "2026-08-09T10:00:00Z"),
        );
        assert!(matches!(
            result,
            Err(DbError::OrderNotSendableToKitchen { .. })
        ));

        let stored = repo::list_kots_for_order(db.connection(), "order-fail-draft").unwrap();
        assert!(stored.is_empty(), "no kot row may exist after rejection");
        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        assert!(
            pending.iter().all(|e| {
                !(e.aggregate_id == "order-fail-draft"
                    && matches!(e.event_type.as_str(), "KOTCreated" | "SentToKitchen"))
            }),
            "no KOTCreated/SentToKitchen outbox row may exist after rejection"
        );
    }

    /// The real atomicity proof ADR-007 requires: a two-station send whose
    /// SECOND station fails to write its `KOTCreated` outbox row (forced
    /// via `send_order_to_kitchen_with_outbox_with_forced_ids` — the first
    /// station's `kot` row and outbox row are already committed to the
    /// open transaction by the time this collision hits) must roll back
    /// the *entire* call: neither station's `kot` row, nor either
    /// station's outbox row, may survive.
    ///
    /// A transaction that inserted all `kot` rows first and all outbox
    /// rows in a second, separate transaction would pass the pre-flight
    /// guard test above unchanged but fail this one — this test was
    /// verified red against exactly that split before being written back
    /// to the real (single-transaction) implementation; see the commit
    /// message / task report for how that check was performed.
    #[test]
    fn failed_send_to_kitchen_mid_transaction_leaves_no_kot_or_outbox_rows_at_all() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");
        seed_station(&db, "station-main", "outlet-1", "MAIN_KITCHEN");
        seed_station(&db, "station-tandoor", "outlet-1", "TANDOOR");
        route_item_to_stations(
            &db,
            &menu_item_id,
            &["station-main".to_string(), "station-tandoor".to_string()],
        );

        let order = sample_order("order-fail-mid", "outlet-1", "device-1");
        let item = sample_order_item("item-fail-mid", "order-fail-mid", &menu_item_id, 30000);
        db.create_order_with_outbox(&order, &[item], &sample_outbox("order-fail-mid"))
            .expect("create draft order");
        confirm_for_kitchen(&mut db, "order-fail-mid");

        // by_station iterates alphabetically: MAIN_KITCHEN first, TANDOOR
        // second. Forcing both stations' outbox ids to the same value
        // means the first station's kot row AND outbox row commit fine
        // inside the open transaction, then the second station's outbox
        // insert collides on local_outbox's PRIMARY KEY.
        let forced_ids = vec![
            ("kot-main".to_string(), "outbox-colliding".to_string()),
            ("kot-tandoor".to_string(), "outbox-colliding".to_string()),
        ];
        let result = db.send_order_to_kitchen_with_outbox_with_forced_ids(
            "order-fail-mid",
            &send_meta("device-1", "2026-08-09T10:00:00Z"),
            forced_ids,
        );
        assert!(
            result.is_err(),
            "the colliding second station's outbox insert must fail the whole call"
        );

        let stored = repo::list_kots_for_order(db.connection(), "order-fail-mid").unwrap();
        assert!(
            stored.is_empty(),
            "neither station's kot row may survive — including the first, already-inserted one"
        );
        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        assert!(
            pending.iter().all(|e| e.id != "outbox-colliding"),
            "neither station's outbox row may survive either"
        );
        assert!(
            pending
                .iter()
                .all(|e| e.event_type != "SentToKitchen" || e.aggregate_id != "order-fail-mid"),
            "SentToKitchen must not have been reached/committed either"
        );
        let stored_order = db
            .get_order("order-fail-mid")
            .unwrap()
            .expect("order exists");
        assert_eq!(
            stored_order.status, "CONFIRMED",
            "the rolled-back call must not have advanced order status"
        );
    }

    /// docs/spec/kitchen.md's change history: an item added after the first
    /// send-to-kitchen must produce a brand-new ticket for the delta, not a
    /// mutation of the original — and that new ticket carries the next
    /// sequence number, shared across whatever stations the delta routes
    /// to.
    #[test]
    fn additions_after_first_send_produce_new_ticket_with_next_sequence() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");
        seed_station(&db, "station-main", "outlet-1", "MAIN_KITCHEN");
        route_item_to_stations(&db, &menu_item_id, &["station-main".to_string()]);

        let order = sample_order("order-132", "outlet-1", "device-1");
        let item_1 = sample_order_item("item-132-1", "order-132", &menu_item_id, 10000);
        db.create_order_with_outbox(&order, &[item_1], &sample_outbox("order-132"))
            .expect("create draft order");
        confirm_for_kitchen(&mut db, "order-132");

        let first_round = db
            .send_order_to_kitchen_with_outbox(
                "order-132",
                &send_meta("device-1", "2026-08-09T10:00:00Z"),
            )
            .expect("first send");
        assert_eq!(first_round.len(), 1);
        assert_eq!(first_round[0].sequence, 1);
        let first_kot_id = first_round[0].id.clone();

        // A second line added directly at the storage layer (order-item
        // amendment after confirmation is outside this crate's Milestone 1
        // DRAFT-only guard and is not this task's concern) — what matters
        // here is purely the KOT-generation delta behaviour.
        let item_2 = sample_order_item("item-132-2", "order-132", &menu_item_id, 5000);
        {
            let tx = db.connection_mut().transaction().expect("begin tx");
            repo::insert_order_item(&tx, &item_2).expect("insert addition line");
            tx.commit().expect("commit addition line");
        }

        let second_round = db
            .send_order_to_kitchen_with_outbox(
                "order-132",
                &send_meta("device-1", "2026-08-09T10:05:00Z"),
            )
            .expect("second send (addition)");

        assert_eq!(
            second_round.len(),
            1,
            "the addition produces exactly one new ticket for the delta"
        );
        assert_eq!(
            second_round[0].sequence, 2,
            "the addition's ticket carries the next sequence, #132-A"
        );
        assert_ne!(
            second_round[0].id, first_kot_id,
            "the addition is a NEW ticket, never a mutation of the first"
        );

        let all_kots = repo::list_kots_for_order(db.connection(), "order-132").unwrap();
        assert_eq!(all_kots.len(), 2, "both tickets must persist side by side");

        // A third call with nothing new must not silently produce an empty
        // ticket — every item is already ticketed.
        let third = db.send_order_to_kitchen_with_outbox(
            "order-132",
            &send_meta("device-1", "2026-08-09T10:10:00Z"),
        );
        assert!(matches!(
            third,
            Err(DbError::NothingToSendToKitchen { .. })
        ));
    }

    fn kot_transition_meta(
        status_history_id: &str,
        outbox_id: &str,
        device_id: &str,
        occurred_at: &str,
    ) -> model::KotTransitionMeta {
        model::KotTransitionMeta {
            status_history_id: status_history_id.to_string(),
            outbox_id: outbox_id.to_string(),
            changed_by_device_id: device_id.to_string(),
            occurred_at: occurred_at.to_string(),
        }
    }

    fn send_single_kot(db: &mut Db, order_id: &str, menu_item_id: &str, item_id: &str) -> String {
        seed_station(db, "station-kds", "outlet-1", "MAIN_KITCHEN");
        route_item_to_stations(db, menu_item_id, &["station-kds".to_string()]);
        let order = sample_order(order_id, "outlet-1", "device-1");
        let item = sample_order_item(item_id, order_id, menu_item_id, 10000);
        db.create_order_with_outbox(&order, &[item], &sample_outbox(order_id))
            .expect("create draft order");
        confirm_for_kitchen(db, order_id);
        let created = db
            .send_order_to_kitchen_with_outbox(
                order_id,
                &send_meta("device-1", "2026-08-09T10:00:00Z"),
            )
            .expect("send to kitchen");
        created[0].id.clone()
    }

    /// Legal transitions write the status, a `kot_status_history` row and a
    /// `KOTStatusChanged` outbox event atomically, and once the last KOT on
    /// the order reaches READY, the order itself becomes READY and an
    /// `OrderReady` event is emitted — the domain-layer derivation, not a
    /// query a caller could forget to run.
    #[test]
    fn legal_transition_chain_marks_order_ready_when_its_only_kot_is_ready() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");
        let kot_id = send_single_kot(&mut db, "order-ready", &menu_item_id, "item-ready");

        db.transition_kot_status_with_outbox(
            &kot_id,
            "ACKNOWLEDGED",
            &kot_transition_meta("h1", "o1", "device-1", "2026-08-09T10:01:00Z"),
        )
        .expect("NEW -> ACKNOWLEDGED");
        db.transition_kot_status_with_outbox(
            &kot_id,
            "PREPARING",
            &kot_transition_meta("h2", "o2", "device-1", "2026-08-09T10:02:00Z"),
        )
        .expect("ACKNOWLEDGED -> PREPARING");

        let order_before = db.get_order("order-ready").unwrap().expect("order exists");
        assert_eq!(
            order_before.status, "SENT_TO_KITCHEN",
            "order must not be READY before its only KOT is"
        );

        db.transition_kot_status_with_outbox(
            &kot_id,
            "READY",
            &kot_transition_meta("h3", "o3", "device-1", "2026-08-09T10:03:00Z"),
        )
        .expect("PREPARING -> READY");

        let stored_kot = repo::list_kots_for_order(db.connection(), "order-ready")
            .unwrap()
            .into_iter()
            .find(|k| k.id == kot_id)
            .expect("kot exists");
        assert_eq!(stored_kot.status, "READY");

        let history: Vec<String> = db
            .connection()
            .prepare("SELECT status FROM kot_status_history WHERE kot_id = ?1 ORDER BY changed_at")
            .unwrap()
            .query_map(rusqlite::params![kot_id], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(history, vec!["ACKNOWLEDGED", "PREPARING", "READY"]);

        let order_after = db.get_order("order-ready").unwrap().expect("order exists");
        assert_eq!(order_after.status, "READY");

        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        assert!(pending
            .iter()
            .filter(|e| e.event_type == "KOTStatusChanged")
            .count()
            >= 3);
        assert!(
            pending
                .iter()
                .any(|e| e.event_type == "OrderReady" && e.aggregate_id == "order-ready"),
            "OrderReady must be emitted once the only KOT reaches READY"
        );
    }

    /// Illegal transitions are rejected outright, never silently ignored —
    /// and reject with nothing written: no status change, no history row,
    /// no outbox row.
    #[test]
    fn illegal_kot_transition_is_rejected_and_writes_nothing() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");
        let kot_id = send_single_kot(&mut db, "order-illegal", &menu_item_id, "item-illegal");

        // NEW -> READY skips ACKNOWLEDGED/PREPARING: illegal.
        let result = db.transition_kot_status_with_outbox(
            &kot_id,
            "READY",
            &kot_transition_meta("h-bad", "o-bad", "device-1", "2026-08-09T10:01:00Z"),
        );
        assert!(matches!(
            result,
            Err(DbError::IllegalKotStatusTransition { .. })
        ));

        let stored_kot = repo::list_kots_for_order(db.connection(), "order-illegal")
            .unwrap()
            .into_iter()
            .find(|k| k.id == kot_id)
            .expect("kot exists");
        assert_eq!(stored_kot.status, "NEW", "status must be untouched");

        let history_count: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM kot_status_history WHERE kot_id = ?1",
                rusqlite::params![kot_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(history_count, 0, "no history row on a rejected transition");

        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        assert!(pending.iter().all(|e| e.id != "o-bad"));
        assert!(pending
            .iter()
            .all(|e| e.event_type != "KOTStatusChanged"));

        // SERVED and CANCELLED are terminal — no further transition legal.
        db.transition_kot_status_with_outbox(
            &kot_id,
            "ACKNOWLEDGED",
            &kot_transition_meta("h1", "o1", "device-1", "2026-08-09T10:02:00Z"),
        )
        .unwrap();
        db.transition_kot_status_with_outbox(
            &kot_id,
            "CANCELLED",
            &kot_transition_meta("h2", "o2", "device-1", "2026-08-09T10:03:00Z"),
        )
        .unwrap();
        let after_cancel = db.transition_kot_status_with_outbox(
            &kot_id,
            "PREPARING",
            &kot_transition_meta("h3", "o3", "device-1", "2026-08-09T10:04:00Z"),
        );
        assert!(matches!(
            after_cancel,
            Err(DbError::IllegalKotStatusTransition { .. })
        ));
    }

    /// A cancelled KOT must not block the order from becoming READY, and a
    /// cancel-only order (no non-cancelled KOTs left) must never be counted
    /// as READY.
    #[test]
    fn cancelled_kot_is_excluded_from_the_order_ready_derivation() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");
        seed_station(&db, "station-main", "outlet-1", "MAIN_KITCHEN");
        seed_station(&db, "station-bar", "outlet-1", "BAR");

        let order = sample_order("order-mixed", "outlet-1", "device-1");
        // Two lines, routed one each to a different station so send_to_kitchen
        // produces two independent KOTs.
        let item_food = sample_order_item("item-food", "order-mixed", &menu_item_id, 10000);
        db.create_order_with_outbox(&order, &[item_food], &sample_outbox("order-mixed"))
            .expect("create draft order");
        route_item_to_stations(&db, &menu_item_id, &["station-main".to_string()]);
        confirm_for_kitchen(&mut db, "order-mixed");

        let created = db
            .send_order_to_kitchen_with_outbox(
                "order-mixed",
                &send_meta("device-1", "2026-08-09T10:00:00Z"),
            )
            .expect("send to kitchen");
        assert_eq!(created.len(), 1);
        let kot_id = created[0].id.clone();

        db.transition_kot_status_with_outbox(
            &kot_id,
            "CANCELLED",
            &kot_transition_meta("h1", "o1", "device-1", "2026-08-09T10:01:00Z"),
        )
        .expect("NEW -> CANCELLED");

        let order_after = db.get_order("order-mixed").unwrap().expect("order exists");
        assert_ne!(
            order_after.status, "READY",
            "an order whose only KOT was cancelled must never be READY"
        );
        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        assert!(
            pending
                .iter()
                .all(|e| !(e.event_type == "OrderReady" && e.aggregate_id == "order-mixed")),
            "OrderReady must not fire when the only KOT was cancelled, not readied"
        );
    }

    /// The exclusion branch `cancelled_kot_is_excluded_from_the_order_ready_derivation`
    /// never actually exercised: an order with one CANCELLED KOT and one
    /// READY KOT (not just one lone cancelled KOT) must still become
    /// READY — the SQL FILTER excludes CANCELLED from both the
    /// denominator and the "not ready" count, so a cancelled ticket must
    /// never be the thing blocking readiness.
    #[test]
    fn mixed_cancelled_and_ready_kots_still_make_order_ready() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");
        seed_station(&db, "station-main", "outlet-1", "MAIN_KITCHEN");
        seed_station(&db, "station-bar", "outlet-1", "BAR");

        // A second menu item routed to a different station, so one
        // send-to-kitchen round produces two independent KOTs.
        let menu_item_2 = "item-2-mixed".to_string();
        repo::upsert_menu_item(
            db.connection(),
            &model::MenuItem {
                id: menu_item_2.clone(),
                outlet_id: "outlet-1".to_string(),
                category_id: "category-1".to_string(),
                name: "Second".to_string(),
                base_price_paise: 5000,
                is_available: true,
                config_version: 1,
            },
        )
        .expect("seed second menu item");
        route_item_to_stations(&db, &menu_item_id, &["station-main".to_string()]);
        route_item_to_stations(&db, &menu_item_2, &["station-bar".to_string()]);

        let order = sample_order("order-mixed-ready", "outlet-1", "device-1");
        let item_a = sample_order_item("item-mixed-a", "order-mixed-ready", &menu_item_id, 10000);
        let item_b = sample_order_item("item-mixed-b", "order-mixed-ready", &menu_item_2, 5000);
        db.create_order_with_outbox(
            &order,
            &[item_a, item_b],
            &sample_outbox("order-mixed-ready"),
        )
        .expect("create draft order");
        confirm_for_kitchen(&mut db, "order-mixed-ready");

        let created = db
            .send_order_to_kitchen_with_outbox(
                "order-mixed-ready",
                &send_meta("device-1", "2026-08-09T10:00:00Z"),
            )
            .expect("send to kitchen");
        assert_eq!(created.len(), 2);
        let main_kot = created
            .iter()
            .find(|k| k.station == "MAIN_KITCHEN")
            .expect("main kot")
            .id
            .clone();
        let bar_kot = created
            .iter()
            .find(|k| k.station == "BAR")
            .expect("bar kot")
            .id
            .clone();

        db.transition_kot_status_with_outbox(
            &bar_kot,
            "CANCELLED",
            &kot_transition_meta("h1", "o1", "device-1", "2026-08-09T10:01:00Z"),
        )
        .expect("cancel the bar ticket");

        db.transition_kot_status_with_outbox(
            &main_kot,
            "ACKNOWLEDGED",
            &kot_transition_meta("h2", "o2", "device-1", "2026-08-09T10:02:00Z"),
        )
        .unwrap();
        db.transition_kot_status_with_outbox(
            &main_kot,
            "PREPARING",
            &kot_transition_meta("h3", "o3", "device-1", "2026-08-09T10:03:00Z"),
        )
        .unwrap();
        db.transition_kot_status_with_outbox(
            &main_kot,
            "READY",
            &kot_transition_meta("h4", "o4", "device-1", "2026-08-09T10:04:00Z"),
        )
        .expect("main ticket reaches READY");

        let order_after = db
            .get_order("order-mixed-ready")
            .unwrap()
            .expect("order exists");
        assert_eq!(
            order_after.status, "READY",
            "one CANCELLED + one READY KOT must still make the order READY"
        );

        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        assert!(
            pending
                .iter()
                .any(|e| e.event_type == "OrderReady" && e.aggregate_id == "order-mixed-ready"),
            "OrderReady must fire once the cancelled ticket stops blocking readiness"
        );
    }

    /// docs/spec/kitchen.md's `#132 -> #132-C` cancellation step: cancelling
    /// an already-ticketed line must produce a brand-new ticket announcing
    /// the cancellation — never just a status flag on the original — at
    /// the same station the original line was ticketed to, carrying the
    /// next sequence number.
    #[test]
    fn cancel_kitchen_items_produces_new_cancellation_ticket_at_original_station() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");
        seed_station(&db, "station-main", "outlet-1", "MAIN_KITCHEN");
        route_item_to_stations(&db, &menu_item_id, &["station-main".to_string()]);

        let order = sample_order("order-132c", "outlet-1", "device-1");
        let item_1 = sample_order_item("item-132c-1", "order-132c", &menu_item_id, 10000);
        db.create_order_with_outbox(&order, &[item_1], &sample_outbox("order-132c"))
            .expect("create draft order");
        confirm_for_kitchen(&mut db, "order-132c");

        let first_round = db
            .send_order_to_kitchen_with_outbox(
                "order-132c",
                &send_meta("device-1", "2026-08-09T10:00:00Z"),
            )
            .expect("first send (#132)");
        assert_eq!(first_round.len(), 1);
        assert_eq!(first_round[0].sequence, 1);

        let cancelled = db
            .cancel_kitchen_items_with_outbox(
                "order-132c",
                &["item-132c-1".to_string()],
                &send_meta("device-1", "2026-08-09T10:05:00Z"),
            )
            .expect("cancel (#132-C)");

        assert_eq!(
            cancelled.len(),
            1,
            "one cancellation ticket, at the item's original station"
        );
        assert_eq!(cancelled[0].station, "MAIN_KITCHEN");
        assert_eq!(
            cancelled[0].sequence, 2,
            "the cancellation gets the next sequence — #132-C"
        );
        assert_eq!(
            cancelled[0].status, "CANCELLED",
            "a cancellation ticket is created already CANCELLED — it announces, it never transitions into it"
        );
        assert_ne!(
            cancelled[0].id, first_round[0].id,
            "the cancellation is a NEW ticket, never a mutation of the original"
        );

        let items: serde_json::Value = serde_json::from_str(&cancelled[0].items_json).unwrap();
        assert_eq!(items[0]["order_item_id"], "item-132c-1");

        let all_kots = repo::list_kots_for_order(db.connection(), "order-132c").unwrap();
        assert_eq!(
            all_kots.len(),
            2,
            "both #132 and #132-C must persist side by side"
        );

        let pending = repo::list_unpublished_outbox(db.connection(), 100).unwrap();
        assert!(pending
            .iter()
            .any(|e| e.event_type == "KOTCreated" && e.aggregate_id == cancelled[0].id));
    }

    /// Cancelling a line that was never sent to the kitchen at all must be
    /// rejected, not silently ticketed anywhere.
    #[test]
    fn cancel_kitchen_items_rejects_a_never_ticketed_line() {
        let mut db = Db::open_in_memory_for_tests().expect("open");
        seed_outlet_and_device(&db, "outlet-1", "device-1");
        let (_, menu_item_id, _) = seed_menu(&db, "outlet-1");
        seed_station(&db, "station-main", "outlet-1", "MAIN_KITCHEN");
        route_item_to_stations(&db, &menu_item_id, &["station-main".to_string()]);

        let order = sample_order("order-cancel-untixed", "outlet-1", "device-1");
        let item = sample_order_item(
            "item-cancel-untixed",
            "order-cancel-untixed",
            &menu_item_id,
            10000,
        );
        db.create_order_with_outbox(&order, &[item], &sample_outbox("order-cancel-untixed"))
            .expect("create draft order");
        confirm_for_kitchen(&mut db, "order-cancel-untixed");
        // Deliberately never sent to the kitchen.

        let result = db.cancel_kitchen_items_with_outbox(
            "order-cancel-untixed",
            &["item-cancel-untixed".to_string()],
            &send_meta("device-1", "2026-08-09T10:00:00Z"),
        );
        assert!(matches!(result, Err(DbError::NotFound("order_item"))));

        let stored = repo::list_kots_for_order(db.connection(), "order-cancel-untixed").unwrap();
        assert!(stored.is_empty());
    }
}
