//! Tauri-managed application state: the single open [`Db`] handle plus the
//! identity of this device (`outlet_id`/`device_id`), guarded by a mutex so
//! concurrent command invocations serialize on the one SQLite connection
//! (`edge/database` is not internally `Sync`-safe across threads).
//!
//! Device/outlet enrollment (how a POS till first learns its own
//! `outlet_id`/`device_id`, and how the local encryption key is provisioned)
//! is out of this task's scope — it is not on Milestone 1's excludes list
//! but no enrollment flow exists yet anywhere in the codebase for this task
//! to consume. Rather than inventing one, device identity and the local
//! encryption key are read from environment variables at startup and the
//! process fails fast with a clear message if they are absent; see the task
//! report for this gap.
//!
//! ## The KDS LAN server lives here too (T12)
//!
//! `db` is `Arc<Mutex<Db>>`, not `Mutex<Db>`, specifically so it can be
//! handed to `holler_edge_device::server::start` as well as to every Tauri
//! command: the LAN protocol's own `set_kot_status` handling
//! (`edge/device/src/server.rs::handle_command`) writes through that shared
//! `Db` and then calls back into the SAME `Hub` this state holds, and
//! `commands::kitchen` needs to call `Hub::notify_kot_upserted`/
//! `notify_kot_removed` on every KOT state change it causes. Both directions
//! only work if POS and the LAN listener share one `Hub` in one process —
//! see `docs/DEV_SETUP.md` for why this state does NOT also open the sealed
//! database a second time via the standalone `kds-lan-server` binary.
//!
//! `hub` is `None` when the LAN server failed to bind (port already in use,
//! etc.) — that failure must never take down the POS itself (CLAUDE.md
//! Milestone 1 acceptance: a cashier must still be able to create orders
//! offline even if the LAN/KDS feature is unavailable), so every notify call
//! site treats a missing hub as a no-op, not an error.

use std::env;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use holler_edge_database::crypto::EncryptionKey;
use holler_edge_database::Db;
use holler_edge_device::{server, Hub, LanServerHandle};
use holler_edge_sync::{StopReason, SyncWorker, WorkerConfig};
use std::time::{Duration, Instant};

/// Default bind address for the embedded KDS LAN server. Fixed and
/// documented (not `:0`) so it can be written into a KDS's
/// `VITE_KDS_LAN_URL` ahead of time — see `edge/device/src/bin/kds_lan_server.rs`
/// for the same constant and the reasoning behind picking a fixed port at
/// all. Overridable with `HOLLER_LAN_BIND_ADDR`.
pub const DEFAULT_LAN_BIND_ADDR: &str = "0.0.0.0:9310";

/// How long the shutdown drain may spend trying to reach the cloud before it
/// gives up and lets the process exit (ADR-020).
///
/// AN OUTLET CLOSING WITH NO UPLINK IS THE NORMAL CASE, NOT AN ERROR. The
/// drain attempts, gives up on this deadline, leaves the rows in the outbox
/// and exits; the startup drain picks them up next trading day. A shutdown
/// that blocks waiting for a network that is not coming is a worse defect
/// than the one hosting the worker fixes -- it would strand a cashier at a
/// till that will not close.
///
/// Sized against `HttpClient`'s own 5s connect timeout so at least one
/// attempt can complete and fail honestly before the deadline bites.
pub const SHUTDOWN_DRAIN_BUDGET: Duration = Duration::from_secs(20);

/// Outbox rows to request per pump call. Bounded so one drain cannot walk an
/// unbounded backlog while a deadline is running.
const DRAIN_BATCH_LIMIT: i64 = 200;

pub struct AppState {
    pub db: Arc<Mutex<Db>>,
    pub outlet_id: String,
    pub device_id: String,
    /// `None` if the embedded LAN server failed to start (e.g. the port is
    /// already bound by a standalone `kds-lan-server` process — see the
    /// module doc). Every KOT-notification call site must tolerate `None`.
    pub hub: Option<Arc<Hub>>,
    /// Kept alive so the accept/heartbeat threads are not orphaned mid-run;
    /// `LanServerHandle` documents that dropping it does not itself stop the
    /// server, so this is book-keeping, not a correctness requirement.
    lan_handle: Mutex<Option<LanServerHandle>>,
    /// The ADR-020 sync host. `None` when this node has no cloud
    /// configuration (no base URL, tenant or device token) -- the ordinary
    /// case for a till that has never been enrolled, and NEVER fatal to
    /// startup, for the same reason the LAN server is not: Milestone 1's
    /// acceptance requires a cashier to work with no uplink at all.
    /// `Mutex` because `SyncWorker` keeps its enrollment-verified flag in a
    /// `Cell` and is therefore `Send` but not `Sync`, while Tauri managed
    /// state must be `Sync`. Wrapping here rather than changing that `Cell`
    /// to an atomic keeps the change inside the consumer: the sync crate
    /// documents itself as driven by ONE caller, and this host is that one
    /// caller.
    sync: Mutex<Option<SyncWorker>>,
}

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error("environment variable {0} is required (device enrollment is not yet implemented)")]
    MissingEnv(&'static str),

    #[error("HOLLER_DB_KEY_HEX must be exactly 64 hex characters (32 bytes)")]
    InvalidKey,

    #[error("database error: {0}")]
    Db(#[from] holler_edge_database::DbError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

fn parse_key_hex(hex: &str) -> Result<EncryptionKey, StartupError> {
    if hex.len() != 64 {
        return Err(StartupError::InvalidKey);
    }
    let mut bytes = [0u8; 32];
    for i in 0..32 {
        let byte =
            u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|_| StartupError::InvalidKey)?;
        bytes[i] = byte;
    }
    Ok(EncryptionKey::new(bytes))
}

impl AppState {
    /// Opens the edge database at `<app_data_dir>/edge.db.enc`, using
    /// `HOLLER_OUTLET_ID`, `HOLLER_DEVICE_ID` and `HOLLER_DB_KEY_HEX` from the
    /// process environment.
    pub fn open(app_data_dir: &std::path::Path) -> Result<Self, StartupError> {
        let outlet_id = env::var("HOLLER_OUTLET_ID")
            .map_err(|_| StartupError::MissingEnv("HOLLER_OUTLET_ID"))?;
        let device_id = env::var("HOLLER_DEVICE_ID")
            .map_err(|_| StartupError::MissingEnv("HOLLER_DEVICE_ID"))?;
        let key_hex = env::var("HOLLER_DB_KEY_HEX")
            .map_err(|_| StartupError::MissingEnv("HOLLER_DB_KEY_HEX"))?;
        let key = parse_key_hex(&key_hex)?;

        std::fs::create_dir_all(app_data_dir)?;
        let sealed_path = app_data_dir.join("edge.db.enc");
        let plaintext_path = app_data_dir.join("edge.db");

        let db = Db::open(&sealed_path, &plaintext_path, key)?;
        let db = Arc::new(Mutex::new(db));

        let bind_addr_str =
            env::var("HOLLER_LAN_BIND_ADDR").unwrap_or_else(|_| DEFAULT_LAN_BIND_ADDR.to_string());
        let (hub, lan_handle) = match bind_addr_str.parse::<SocketAddr>() {
            Ok(addr) => start_lan_server(addr, db.clone()),
            Err(e) => {
                eprintln!(
                    "holler-pos: invalid HOLLER_LAN_BIND_ADDR {bind_addr_str:?} ({e}); \
                     KDS LAN server disabled for this session"
                );
                (None, None)
            }
        };

        let sync = build_sync_worker(&outlet_id, &device_id);

        let state = Self {
            db,
            outlet_id,
            device_id,
            hub,
            lan_handle: Mutex::new(lan_handle),
            sync: Mutex::new(sync),
        };

        // ADR-020: DRAIN ON LAUNCH, BEFORE ANYTHING ELSE -- ahead of the first
        // sale of the day, not lazily whenever a timer first fires. Together
        // with the shutdown drain this turns "syncs while the till is open"
        // into "the day reaches the cloud at both ends of every trading day",
        // which is a sentence that can be said to a restaurant.
        state.drain_outbox("startup", SHUTDOWN_DRAIN_BUDGET);

        Ok(state)
    }

    /// Wraps an already-open `Db` (e.g. `Db::open_in_memory_for_tests()`) in
    /// application state without going through environment variables or a
    /// real encrypted file — used by this crate's integration tests, which
    /// must not depend on the (not-yet-built) device enrollment flow. Never
    /// starts the LAN server: dozens of these can exist in one test binary,
    /// and binding a shared network port from every one of them would make
    /// the suite flaky by construction rather than by bug. Tests that need to
    /// exercise `Hub` notifications use [`AppState::new_with_hub`].
    pub fn new(db: Db, outlet_id: String, device_id: String) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            outlet_id,
            device_id,
            hub: None,
            lan_handle: Mutex::new(None),
            sync: Mutex::new(None),
        }
    }

    /// Same as [`AppState::new`], but with a caller-supplied [`Hub`] wired in
    /// — for tests that assert a command notifies subscribers, without
    /// binding any real socket.
    pub fn new_with_hub(db: Db, outlet_id: String, device_id: String, hub: Arc<Hub>) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            outlet_id,
            device_id,
            hub: Some(hub),
            lan_handle: Mutex::new(None),
            sync: Mutex::new(None),
        }
    }

    /// Stops accepting new KDS connections and the heartbeat loop, called
    /// from the `RunEvent::Exit` hook in `lib.rs` alongside sealing the edge
    /// database. A no-op if the LAN server never started. Existing sockets
    /// are not force-closed (see `LanServerHandle::shutdown`'s own doc) —
    /// acceptable on process exit, where they are about to be torn down by
    /// the OS regardless.
    /// Same as [`AppState::new`], but hosting a caller-supplied [`SyncWorker`]
    /// — for the ADR-020 falsification tests, which must point the worker at a
    /// fake cloud rather than at whatever `HOLLER_CLOUD_BASE_URL` says.
    pub fn new_with_sync(db: Db, outlet_id: String, device_id: String, worker: SyncWorker) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            outlet_id,
            device_id,
            hub: None,
            lan_handle: Mutex::new(None),
            sync: Mutex::new(Some(worker)),
        }
    }

    /// Seals and closes the edge database, exactly as the `RunEvent::Exit` hook
    /// does. Exposed so a test can assert what a drain finds AFTER the seal.
    pub fn seal_for_tests(&self) {
        let mut db = match self.db.lock() {
            Ok(db) => db,
            Err(e) => e.into_inner(),
        };
        let _ = db.shutdown_in_place();
    }

    /// Rows still unpublished in `local_outbox`, read straight from the
    /// database.
    ///
    /// **INDEPENDENT OF THE DRAIN'S OWN CLAIM.** "published N" is what the
    /// drain says about itself; this is what the table says. A drain that
    /// mis-routes, mis-counts or silently consumes rows cannot make this number
    /// agree with it, which is the whole point — the ADR-020 defect printed
    /// "published 0" with the outbox full, and no assertion existed that could
    /// contradict it.
    ///
    /// Logged after every drain so the two numbers sit side by side in the same
    /// terminal: a non-zero remainder after a successful drain is not
    /// necessarily wrong (offline is normal), but it must never be a surprise.
    pub fn pending_outbox_rows(&self) -> i64 {
        let db = match self.db.lock() {
            Ok(db) => db,
            Err(e) => e.into_inner(),
        };
        db.connection()
            .query_row(
                "SELECT COUNT(*) FROM local_outbox WHERE published_at IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap_or(-1)
    }

    /// Drains `local_outbox` toward the cloud, bounded by `budget` (ADR-020).
    ///
    /// Returns the number of rows acknowledged. A no-op returning 0 when this
    /// node has no cloud configuration -- an unenrolled till is not an error.
    ///
    /// BOUNDED, AND THE BOUND IS THE POINT. Every pass checks the deadline
    /// before starting another, and a transport failure stops immediately
    /// rather than retrying: with the WAN down, retrying inside a shutdown path
    /// only spends the budget to reach the same answer. Offline is the expected
    /// outcome here, not a failure to report loudly.
    ///
    /// MUST BE CALLED BEFORE `Db::shutdown_in_place`. The drain needs a live
    /// database connection; after the seal it would find nothing to send and
    /// would silently succeed at doing nothing -- precisely the shape of defect
    /// that passes review (ADR-020).
    pub fn drain_outbox(&self, phase: &str, budget: Duration) -> usize {
        // Lock order is ALWAYS sync-then-db. The drain takes the database
        // lock inside this one; taking them in the other order anywhere else
        // would deadlock a shutdown path, which is the worst place for one.
        let guard = match self.sync.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        let Some(worker) = guard.as_ref() else {
            return 0;
        };
        let deadline = Instant::now() + budget;
        let mut acked = 0usize;

        loop {
            if Instant::now() >= deadline {
                eprintln!(
                    "holler-pos: {phase} outbox drain hit its {}s budget; rows still pending stay in local_outbox and the next drain picks them up",
                    budget.as_secs()
                );
                break;
            }

            let mut db = match self.db.lock() {
                Ok(db) => db,
                Err(e) => e.into_inner(),
            };
            let report = match worker.pump_outbox(&mut db, DRAIN_BATCH_LIMIT) {
                Ok(report) => report,
                Err(e) => {
                    eprintln!("holler-pos: {phase} outbox drain failed: {e}");
                    break;
                }
            };

            // THREE STREAMS, NOT ONE. `pump_outbox` routes orders and table
            // sessions; a goods receipt is `("goods_receipt_note",
            // "GoodsReceiptRecorded")`, which that router does not map at all --
            // it reports the row as `unrouted_skipped` and leaves it pending
            // forever. Procurement has its own pump, and the two high-volume
            // stock streams have a third. Hosting one of three would have left
            // every GRN, return, transfer and ledger entry sitting in an outbox
            // that a drain reported as "nothing to send".
            let procurement = match worker.pump_procurement(&mut db, DRAIN_BATCH_LIMIT) {
                Ok(report) => report,
                Err(e) => {
                    eprintln!("holler-pos: {phase} procurement drain failed: {e}");
                    break;
                }
            };
            let ranged = match worker.pump_ranged_streams(&mut db, DRAIN_BATCH_LIMIT) {
                Ok(report) => report,
                Err(e) => {
                    eprintln!("holler-pos: {phase} stock stream drain failed: {e}");
                    break;
                }
            };
            drop(db);

            // PER-STREAM, AND UNROUTED IS ITS OWN NUMBER. A single
            // "published N" cannot distinguish an EMPTY outbox from an
            // UNROUTED one: both report zero, and the unrouted case is the
            // ADR-020 defect that shipped -- a host driving one of three pumps
            // printed "published 0" while every GRN sat pending, which reads
            // as "nothing to send".
            //
            // Fixing the three streams that exist would leave the NEXT stream
            // hitting the same wall and reading the same way. So the counter
            // reports what it actually saw, per stream, and a non-zero
            // `unrouted` is the signal that a row exists which nothing knows
            // how to send.
            for line in [
                StreamTally {
                    stream: "orders",
                    published: report.published.len(),
                    unrouted: report.unrouted_skipped.len(),
                    refused: report.authority_violations.len(),
                },
                StreamTally {
                    stream: "procurement",
                    published: procurement.published.len(),
                    unrouted: 0,
                    refused: procurement.blocked.len() + procurement.over_budget.len(),
                },
                StreamTally {
                    stream: "stock",
                    published: ranged.ledger_acked.len() + ranged.gap_acked.len(),
                    unrouted: 0,
                    refused: ranged.blocked.len(),
                },
            ] {
                line.report(phase);
            }

            acked += report.published.len()
                + procurement.published.len()
                + ranged.ledger_acked.len()
                + ranged.gap_acked.len();
            let progressed = !report.published.is_empty()
                || !procurement.published.is_empty()
                || !ranged.ledger_acked.is_empty()
                || !ranged.gap_acked.is_empty();

            // A stream that stopped is reported by its own name: "the drain
            // stopped" without saying WHICH stream is the swallowed-stderr
            // failure from the e2e-scenario sweep, one layer in.
            for (name, stop) in [
                ("procurement", &procurement.stopped),
                ("stock streams", &ranged.stopped),
            ] {
                if let Some(reason) = stop {
                    eprintln!("holler-pos: {phase} {name} drain stopped: {reason:?}");
                }
            }

            match report.stopped {
                // No route to the cloud: the shop-floor case. Stop now. The
                // rows are safe in the outbox and nothing is lost by waiting.
                Some(StopReason::Offline) => {
                    eprintln!(
                        "holler-pos: {phase} outbox drain found no route to the cloud; {acked} row(s) sent, the rest stay pending"
                    );
                    break;
                }
                Some(reason) => {
                    eprintln!(
                        "holler-pos: {phase} outbox drain stopped after {acked} row(s): {reason:?}"
                    );
                    break;
                }
                // Backlog exhausted for this pass. Another pass only helps if
                // this one actually moved rows.
                None => {
                    if !progressed {
                        break;
                    }
                }
            }
        }

        if acked > 0 {
            eprintln!("holler-pos: {phase} outbox drain published {acked} row(s)");
        }

        // THE INDEPENDENT NUMBER, printed beside the drain's own claim. Read
        // from local_outbox, not accumulated by the loop above, so a
        // mis-routing or mis-counting drain cannot make the two agree.
        let pending = self.pending_outbox_rows();
        eprintln!("holler-pos: {phase} drain complete: {acked} published this pass, {pending} row(s) still pending in local_outbox");
        acked
    }

    pub fn shutdown_lan_server(&self) {
        let handle = self
            .lan_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(handle) = handle {
            handle.shutdown();
        }
    }
}

/// One stream's outcome from a single drain pass (ADR-020 correction,
/// 2026-09-02).
///
/// **`published` alone is not a report.** A drain that says "published 0"
/// cannot distinguish an EMPTY outbox from one full of rows nothing knows how
/// to route — and the second is the defect that shipped in this ADR's first
/// implementation, where a host driving one of three pumps printed zero while
/// every goods receipt sat pending. Reporting the counts separately, per
/// stream, is what makes the next unrouted stream visible instead of silent.
struct StreamTally {
    stream: &'static str,
    /// Acknowledged by the cloud and marked published.
    published: usize,
    /// Rows this pump SAW and had no route for. Left pending, not an error —
    /// and the number that says "something is here that nothing can send".
    unrouted: usize,
    /// Refused locally or out of retry budget: an authority violation, a
    /// blocked entry, a row past its per-entry budget. Also left pending.
    refused: usize,
}

impl StreamTally {
    /// Silent only when the stream was genuinely empty and clean, so that a
    /// quiet drain means "nothing was there" and never "nothing was routable".
    fn report(&self, phase: &str) {
        if self.published == 0 && self.unrouted == 0 && self.refused == 0 {
            return;
        }
        eprintln!(
            "holler-pos: {phase} drain [{}] published={} unrouted={} refused={}",
            self.stream, self.published, self.unrouted, self.refused
        );
        if self.unrouted > 0 {
            eprintln!(
                "holler-pos:   {} {} row(s) have NO ROUTE and will stay pending until one exists — this is not an empty queue",
                self.unrouted, self.stream
            );
        }
    }
}

/// Builds the ADR-020 sync host from the environment, or `None` when this node
/// has no cloud configuration.
///
/// `HOLLER_CLOUD_BASE_URL`, `HOLLER_TENANT_ID` and `HOLLER_DEVICE_TOKEN` are all
/// required together: a worker with a URL and no credential would fail every
/// request with a 401 and burn retry budget doing it.
///
/// ABSENCE IS NOT AN ERROR AND MUST NEVER BE FATAL. A till that has never been
/// enrolled still takes orders, bills, prints and receives goods -- that is
/// Milestone 1's acceptance and ADR-013's whole premise. The same rule the LAN
/// server follows: log what is disabled, and start anyway.
///
/// The device token is read from the environment and handed to `WorkerConfig`,
/// which documents that it is never logged, never placed in an error and never
/// persisted by that crate. It is not echoed here either.
fn build_sync_worker(outlet_id: &str, device_id: &str) -> Option<SyncWorker> {
    let base_url = env::var("HOLLER_CLOUD_BASE_URL")
        .ok()
        .filter(|s| !s.is_empty());
    let tenant_id = env::var("HOLLER_TENANT_ID").ok().filter(|s| !s.is_empty());
    let device_token = env::var("HOLLER_DEVICE_TOKEN")
        .ok()
        .filter(|s| !s.is_empty());

    match (base_url, tenant_id, device_token) {
        (Some(base_url), Some(tenant_id), Some(device_token)) => {
            eprintln!("holler-pos: sync worker hosted, cloud at {base_url}");
            Some(SyncWorker::new(WorkerConfig {
                tenant_id,
                outlet_id: outlet_id.to_string(),
                device_id: device_id.to_string(),
                base_url,
                device_token,
            }))
        }
        _ => {
            eprintln!(
                "holler-pos: sync worker disabled (HOLLER_CLOUD_BASE_URL, HOLLER_TENANT_ID and HOLLER_DEVICE_TOKEN are required together); the outlet works offline and nothing replays"
            );
            None
        }
    }
}

/// Starts the embedded KDS LAN server over the POS's own `Arc<Mutex<Db>>`.
/// Never fatal to POS startup: Milestone 1's acceptance (a cashier can
/// create orders fully offline) must hold even if this LAN feature cannot
/// bind its port — logs and returns `(None, None)` instead of propagating an
/// error.
fn start_lan_server(
    addr: SocketAddr,
    db: Arc<Mutex<Db>>,
) -> (Option<Arc<Hub>>, Option<LanServerHandle>) {
    // Local-first verification, with NO cloud fallback (ADR-017 amendment).
    // The standalone kds-lan-server binary requires HOLLER_CLOUD_BASE_URL for a
    // fallback path; the POS deliberately does not, because requiring a cloud
    // URL to start would make the POS's own startup depend on the uplink —
    // exactly what ADR-013 and Milestone 1's acceptance forbid.
    //
    // The consequence is honest and fail-closed: a device credential that has
    // never synced to this node cannot connect until it does. Absence is
    // treated as unknown, never as revoked.
    let verifier: Arc<dyn holler_edge_device::DeviceTokenVerifier> = Arc::new(
        holler_edge_device::CachedCredentialVerifier::new(db.clone(), "KDS", None),
    );

    match server::start(addr, db, server::DEFAULT_HEARTBEAT_INTERVAL, verifier) {
        Ok(handle) => {
            if addr.ip().is_unspecified() {
                eprintln!(
                    "holler-pos: KDS LAN server listening on {} (0.0.0.0 — reachable from \
                     anywhere on this LAN; connections must present an enrolled device \
                     credential, verified against the local cache)",
                    handle.local_addr()
                );
            } else {
                eprintln!(
                    "holler-pos: KDS LAN server listening on {}",
                    handle.local_addr()
                );
            }
            let hub = handle.hub.clone();
            (Some(hub), Some(handle))
        }
        Err(e) => {
            eprintln!(
                "holler-pos: KDS LAN server failed to bind {addr}: {e}; kitchen tickets will \
                 not reach any KDS screen this session (POS itself is unaffected)"
            );
            (None, None)
        }
    }
}
