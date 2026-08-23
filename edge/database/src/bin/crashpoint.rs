//! Drives ONE real confirm+deduct against a real sealed edge database, so a
//! crash can be induced at a deterministic point inside that transaction
//! (M4 acceptance criterion 2).
//!
//! Built only with `--features crash-points`, and declared with
//! `required-features` so it cannot appear in a release build by accident.
//!
//! It goes through `Db::confirm_order_with_outbox` — the same entry point the
//! POS calls — rather than reproducing the write itself. A harness that
//! reimplements the path it is testing proves only that the harness works;
//! the acceptance rules exclude that kind of evidence, and this binary exists
//! precisely to avoid it.
//!
//! Usage:
//!     crashpoint <data_dir> <order_id>
//!
//! Environment:
//!     HOLLER_DB_KEY_HEX   64 hex chars, the same key the database was sealed with
//!     HOLLER_CRASH_POINT  optional; when it names a point, the process aborts there
//!
//! Exit codes are the test's signal, so they are explicit:
//!     0  the confirm committed and the database was sealed cleanly
//!     2  a usage or database error — never to be confused with a crash
//! An abort produces neither: the OS reports the abnormal termination, which
//! is exactly what the test waits for.

use std::path::PathBuf;
use std::process::ExitCode;

use holler_edge_database::crypto::EncryptionKey;
use holler_edge_database::model::OrderConfirmedMeta;
use holler_edge_database::Db;

const EXIT_ERROR: u8 = 2;

fn main() -> ExitCode {
    match run() {
        Ok(()) => {
            // Reached only when no crash point fired. The test's positive
            // control depends on this being distinguishable from an abort.
            println!("crashpoint: confirm committed and database sealed");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("crashpoint: {e}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let data_dir = PathBuf::from(
        args.next()
            .ok_or("usage: crashpoint <data_dir> <order_id>")?,
    );
    let order_id = args
        .next()
        .ok_or("usage: crashpoint <data_dir> <order_id>")?;

    let key_hex = std::env::var("HOLLER_DB_KEY_HEX").map_err(|_| "HOLLER_DB_KEY_HEX is not set")?;
    let key = parse_key_hex(&key_hex)?;

    let sealed = data_dir.join("edge.db.enc");
    let plaintext = data_dir.join("edge.db");

    let mut db = Db::open(&sealed, &plaintext, key).map_err(|e| format!("opening db: {e}"))?;

    // The confirm the POS itself performs. If HOLLER_CRASH_POINT names a
    // point inside this call, the process dies here: no unwind, no Drop, no
    // seal, no commit.
    db.confirm_order_with_outbox(
        &order_id,
        &OrderConfirmedMeta {
            outbox_id: format!("outbox-confirm-{order_id}"),
            occurred_at: "2026-08-23T10:30:00Z".to_string(),
            confirmed_at: "2026-08-23T10:30:00Z".to_string(),
        },
    )
    .map_err(|e| format!("confirming {order_id}: {e}"))?;

    db.close().map_err(|e| format!("sealing db: {e}"))?;
    Ok(())
}

fn parse_key_hex(hex: &str) -> Result<EncryptionKey, String> {
    if hex.len() != 64 {
        return Err(format!(
            "HOLLER_DB_KEY_HEX must be 64 hex chars, got {}",
            hex.len()
        ));
    }
    let mut bytes = [0u8; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|e| format!("HOLLER_DB_KEY_HEX is not hex: {e}"))?;
    }
    Ok(EncryptionKey::new(bytes))
}
