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
//!     crashpoint <data_dir> <order_id>     -- M4: one confirm + deduct
//!     crashpoint <data_dir> --grn          -- M5: one goods receipt + post
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
use holler_edge_database::model::{NewGoodsReceiptNote, NewGrnLine, OrderConfirmedMeta};
use holler_edge_database::Db;

/// The receipt the M5 crash run records. Fixed ids so the test can look the
/// rows up (or fail to) from an independent reopen.
const GRN_ID: &str = "0191b000-0000-7000-8000-0000000000d1";

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
    let target = args
        .next()
        .ok_or("usage: crashpoint <data_dir> <order_id|--grn>")?;

    let key_hex = std::env::var("HOLLER_DB_KEY_HEX").map_err(|_| "HOLLER_DB_KEY_HEX is not set")?;
    let key = parse_key_hex(&key_hex)?;

    let sealed = data_dir.join("edge.db.enc");
    let plaintext = data_dir.join("edge.db");

    let mut db = Db::open(&sealed, &plaintext, key).map_err(|e| format!("opening db: {e}"))?;

    // The write the POS itself performs. If HOLLER_CRASH_POINT names a point
    // inside this call, the process dies there: no unwind, no Drop, no seal,
    // no commit. Both branches go through the SAME public entry point the POS
    // uses -- a harness that reimplements the path it is testing proves only
    // that the harness works.
    if target == "--grn" {
        receive_goods(&mut db)?;
    } else {
        db.confirm_order_with_outbox(
            &target,
            &OrderConfirmedMeta {
                outbox_id: format!("outbox-confirm-{target}"),
                occurred_at: "2026-08-23T10:30:00Z".to_string(),
                confirmed_at: "2026-08-23T10:30:00Z".to_string(),
            },
        )
        .map_err(|e| format!("confirming {target}: {e}"))?;
    }

    db.close().map_err(|e| format!("sealing db: {e}"))?;
    Ok(())
}

/// Records one goods receipt against whatever the seed already stocks --
/// no purchase order and no supplier, which is the walk-in delivery case and
/// the one an outlet performs with the uplink down.
///
/// Reads the outlet, user and item out of the seeded database rather than
/// assuming them, so this follows the seed instead of silently testing a
/// fixture the seed no longer produces.
fn receive_goods(db: &mut Db) -> Result<(), String> {
    let (outlet_id, user_id, item_id) = db
        .connection()
        .query_row(
            "SELECT o.id, u.id, i.id              FROM outlet o              JOIN app_user u ON u.outlet_id = o.id              JOIN inventory_item i ON i.outlet_id = o.id              ORDER BY i.id LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| format!("the seed must contain an outlet, a user and an item: {e}"))?;

    let dimension: String = db
        .connection()
        .query_row(
            "SELECT dimension FROM inventory_item WHERE id = ?1",
            [&item_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("reading the item dimension: {e}"))?;

    db.record_goods_receipt(NewGoodsReceiptNote {
        id: GRN_ID.to_string(),
        outlet_id,
        purchase_order_id: None,
        supplier_id: None,
        delivery_note_ref: Some("DN-CRASH".to_string()),
        received_at: "2026-08-23T10:30:00Z".to_string(),
        received_by_user_id: user_id,
        notes: None,
        lines: vec![NewGrnLine {
            inventory_item_id: item_id,
            entered_purchase_unit: "kg".to_string(),
            entered_quantity_micro: 5_000_000,
            // The author's declaration, matching the item so the run is not
            // also exercising a dimension mismatch.
            quantity_dimension: dimension,
            purchase_price_paise: 4_000,
            batch_code: None,
            expiry_date: None,
            purchase_order_line_id: None,
        }],
    })
    .map_err(|e| format!("receiving goods: {e}"))?;
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
