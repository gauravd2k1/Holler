//! Ships the KDS LAN WebSocket server (`holler_edge_device::server::start`)
//! as an actual process. Milestone 2 built `edge/device` as a library with no
//! caller anywhere in the repo except a test harness that binds
//! `127.0.0.1:0` — unreachable from a second machine even by accident. This
//! binary is the seam: it opens the SAME sealed edge database the POS opens
//! (`apps/pos/src-tauri/src/state.rs::AppState::open`, mirrored from
//! `edge/database/src/bin/devseed.rs`) and serves it to any KDS screen on the
//! outlet LAN.
//!
//! DEVELOPMENT LAUNCH PATH TODAY (see docs/DEV_SETUP.md): run this alongside
//! the POS, both pointed at the same `HOLLER_EDGE_DATA_DIR`/`HOLLER_DB_KEY_HEX`.
//! Nothing here is outlet-installer wiring — ADR-013 still applies — this is
//! the process a real deployment would eventually launch, run by hand for now.
//!
//! # This binds 0.0.0.0 by default. TLS is still absent (ADR-017's own
//! posture: this is a plaintext LAN hop inside one outlet, ADR-013 — network
//! segmentation is the documented mitigation, not TLS on this port).
//! `outlet_id`/`device_id` in the handshake query string remain identity,
//! not authentication (ADR-015) — but every connection must now additionally
//! present a verified `device_token` as its first WS frame
//! (`server.rs::authenticate_first_frame`, ADR-017 hole 3) before it
//! receives a snapshot or has a command accepted. A captured/guessed
//! `device_id` alone can no longer drive `set_kot_status`. `verify()`
//! resolves whether the token belongs to *some* device enrolled at this
//! outlet, not specifically to the claimed `device_id` — see
//! `src/auth.rs`'s doc comment for why, and for what remains open.

use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use holler_edge_database::crypto::EncryptionKey;
use holler_edge_database::Db;
use holler_edge_device::{server, CloudConfigOracleVerifier};

/// Fixed, documented, non-ephemeral default. `server::start` accepts
/// `SocketAddr`, so `:0` (an OS-assigned ephemeral port) is technically legal
/// input — it is exactly what the test harness
/// (`tests/integration/kds-lan-bridge`) uses on purpose. A shipped launcher
/// must not default to that: an ephemeral port cannot be written into a KDS's
/// `VITE_KDS_LAN_URL` ahead of time. Override with `HOLLER_LAN_BIND_ADDR`.
const DEFAULT_BIND_ADDR: &str = "0.0.0.0:9310";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kds-lan-server: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let key_hex = require_env("HOLLER_DB_KEY_HEX")?;
    let key = parse_key_hex(&key_hex)?;

    // Must match AppState::open (apps/pos/src-tauri/src/state.rs) and
    // edge/database/src/bin/devseed.rs: same <data_dir>/edge.db.enc, or this
    // process opens/creates a second, empty database instead of the POS's.
    let data_dir = match env::var("HOLLER_EDGE_DATA_DIR") {
        Ok(d) => PathBuf::from(d),
        Err(_) => default_app_data_dir()?,
    };
    let sealed_path = data_dir.join("edge.db.enc");
    let plaintext_path = data_dir.join("edge.db");

    let bind_addr_str =
        env::var("HOLLER_LAN_BIND_ADDR").unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());
    let bind_addr: SocketAddr = bind_addr_str
        .parse()
        .map_err(|e| format!("invalid HOLLER_LAN_BIND_ADDR {bind_addr_str:?}: {e}"))?;

    println!(
        "kds-lan-server: opening edge database at {}",
        sealed_path.display()
    );
    let db = Db::open(&sealed_path, &plaintext_path, key).map_err(|e| format!("opening db: {e}"))?;
    let db = Arc::new(Mutex::new(db));

    // ADR-017 hole 3: every KDS connection must present a verified
    // device_token before it gets a snapshot. HOLLER_CLOUD_BASE_URL is the
    // same cloud this edge node syncs against — required, not defaulted, so
    // a misconfigured launch fails at startup rather than silently verifying
    // against nothing.
    let cloud_base_url = require_env("HOLLER_CLOUD_BASE_URL")?;
    let verifier: Arc<dyn holler_edge_device::DeviceTokenVerifier> =
        Arc::new(CloudConfigOracleVerifier::new(cloud_base_url));

    let handle = server::start(
        bind_addr,
        db.clone(),
        server::DEFAULT_HEARTBEAT_INTERVAL,
        verifier,
    )
    .map_err(|e| format!("binding {bind_addr}: {e}"))?;

    println!("kds-lan-server: listening on {}", handle.local_addr());
    if bind_addr.ip().is_unspecified() {
        println!(
            "kds-lan-server: WARNING — bound to {bind_addr} (all interfaces). This port has \
             NO AUTHENTICATION: any device on this LAN that can reach it and guess/observe an \
             outlet_id/device_id pair can read kitchen tickets and drive set_kot_status \
             (mark food SERVED or CANCELLED). See docs/backlog-m2.md, Device enrollment. Do \
             not run this on a network you do not control."
        );
    }
    println!("kds-lan-server: press Ctrl+C to stop.");

    // Graceful shutdown on Ctrl+C: seal the edge database (checkpoint,
    // re-encrypt, wipe the plaintext working copy) rather than leaving the
    // decrypted file on disk — same posture as the POS's own
    // `RunEvent::Exit` hook (apps/pos/src-tauri/src/lib.rs), ADR-011.
    // `shutdown_in_place` takes `&mut Db`, so this works through the shared
    // `Arc<Mutex<Db>>` without needing to unwrap the Arc first.
    let shutdown_db = db.clone();
    ctrlc::set_handler(move || {
        eprintln!("\nkds-lan-server: shutting down...");
        match shutdown_db.lock() {
            Ok(mut guard) => {
                if let Err(e) = guard.shutdown_in_place() {
                    eprintln!("kds-lan-server: failed to seal edge database on exit: {e}");
                }
            }
            Err(_) => eprintln!("kds-lan-server: database lock poisoned on exit"),
        }
        std::process::exit(0);
    })
    .map_err(|e| format!("installing Ctrl+C handler: {e}"))?;

    // The accept/heartbeat threads inside `handle` do the actual work; this
    // thread just needs to stay alive until the Ctrl+C handler above exits
    // the process. Keeping `handle` in scope (rather than dropping it) is
    // required for that.
    loop {
        std::thread::sleep(Duration::from_secs(3600));
        let _keep_alive = &handle;
    }
}

fn require_env(key: &'static str) -> Result<String, String> {
    env::var(key).map_err(|_| format!("environment variable {key} is required"))
}

/// Mirrors Tauri v2's `app_data_dir()` on Windows: `%APPDATA%\<identifier>`,
/// where the identifier is `com.holler.pos` from tauri.conf.json. Override
/// with `HOLLER_EDGE_DATA_DIR` if that ever resolves differently. Duplicated
/// from `edge/database/src/bin/devseed.rs` rather than shared — both are
/// dev-facing binaries in this crate's own tree, but `holler-edge-device`
/// must not gain a dependency on `apps/pos/src-tauri` (nor vice versa).
fn default_app_data_dir() -> Result<PathBuf, String> {
    let appdata = env::var("APPDATA")
        .map_err(|_| "APPDATA is not set; pass HOLLER_EDGE_DATA_DIR explicitly".to_string())?;
    Ok(PathBuf::from(appdata).join("com.holler.pos"))
}

/// Same 32-byte hex key parsing as `apps/pos/src-tauri/src/state.rs` and
/// `edge/database/src/bin/devseed.rs`, duplicated for the same reason those
/// two duplicate it from each other: it is a few lines, and this crate must
/// not depend on the POS crate to reach it.
fn parse_key_hex(hex: &str) -> Result<EncryptionKey, String> {
    if hex.len() != 64 {
        return Err("HOLLER_DB_KEY_HEX must be exactly 64 hex characters (32 bytes)".to_string());
    }
    let mut bytes = [0u8; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| "HOLLER_DB_KEY_HEX contains a non-hex character".to_string())?;
    }
    Ok(EncryptionKey::new(bytes))
}
