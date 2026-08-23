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

/// Default bind address for the embedded KDS LAN server. Fixed and
/// documented (not `:0`) so it can be written into a KDS's
/// `VITE_KDS_LAN_URL` ahead of time — see `edge/device/src/bin/kds_lan_server.rs`
/// for the same constant and the reasoning behind picking a fixed port at
/// all. Overridable with `HOLLER_LAN_BIND_ADDR`.
pub const DEFAULT_LAN_BIND_ADDR: &str = "0.0.0.0:9310";

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

        Ok(Self {
            db,
            outlet_id,
            device_id,
            hub,
            lan_handle: Mutex::new(lan_handle),
        })
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
        }
    }

    /// Stops accepting new KDS connections and the heartbeat loop, called
    /// from the `RunEvent::Exit` hook in `lib.rs` alongside sealing the edge
    /// database. A no-op if the LAN server never started. Existing sockets
    /// are not force-closed (see `LanServerHandle::shutdown`'s own doc) —
    /// acceptable on process exit, where they are about to be torn down by
    /// the OS regardless.
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
