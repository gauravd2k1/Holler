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

use std::env;
use std::sync::Mutex;

use holler_edge_database::crypto::EncryptionKey;
use holler_edge_database::Db;

pub struct AppState {
    pub db: Mutex<Db>,
    pub outlet_id: String,
    pub device_id: String,
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

        Ok(Self::new(db, outlet_id, device_id))
    }

    /// Wraps an already-open `Db` (e.g. `Db::open_in_memory_for_tests()`) in
    /// application state without going through environment variables or a
    /// real encrypted file — used by this crate's integration tests, which
    /// must not depend on the (not-yet-built) device enrollment flow.
    pub fn new(db: Db, outlet_id: String, device_id: String) -> Self {
        Self {
            db: Mutex::new(db),
            outlet_id,
            device_id,
        }
    }
}
