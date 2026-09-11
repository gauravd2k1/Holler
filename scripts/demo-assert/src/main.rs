//! Dev-only assertion tool for `scripts/demo-reset.ps1`.
//!
//! Opens the sealed edge SQLite database (the same `Db::open` path
//! `apps/pos/src-tauri` and `edge/database/src/bin/devseed.rs` use) and
//! checks the four invariants `seed/README.md`'s "The one command" and this
//! milestone's demo-kickoff brief require after a clean reset:
//!
//!   1. zero rows in `sync_replay_block` (ranged-stream give-ups, contracts
//!      0.5.8/0.6.0)
//!   2. zero rows in `stock_deduction_gap` (a sale with no resolvable
//!      recipe, contracts 0.5.0) -- distinct from `grn_gap`, which the demo
//!      seed legitimately populates with one `NO_PURCHASE_ORDER` row
//!      (ADR-019) and which this tool deliberately does not touch
//!   3. zero rows in `sync_outbox_block` with `blocked_at` set -- the
//!      general outbox's durable "given up" record (contracts 0.6.4,
//!      ADR-023); this is what "zero blocked rows in local_outbox" in the
//!      brief actually queries, since `local_outbox` itself carries no
//!      blocked flag of its own
//!   4. a PROXY for "the POS sync banner is empty", NOT the banner itself.
//!      `apps/pos/src/components/SyncBlockedBanner.tsx` renders two groups,
//!      backed by two Tauri commands
//!      (`list_blocked_outbox_rows`/`list_persistently_failing_outbox_rows`,
//!      `apps/pos/src-tauri/src/commands/inventory.rs`): the blocked group
//!      (assertion 3, above) and the "still retrying after repeated
//!      failures" group, which is `sync_outbox_block` rows with
//!      `blocked_at IS NULL AND attempts >= OUTBOX_ATTENTION_ATTEMPTS`. This
//!      tool queries that second group directly and reports it as
//!      assertion 4. It has never rendered the banner in a browser and does
//!      not claim to -- see the script's own output and
//!      `docs/demo-reset.md` for that caveat stated plainly.
//!
//! `OUTBOX_ATTENTION_ATTEMPTS` (below) is duplicated from
//! `edge/sync/src/worker.rs:84` rather than pulled in as a dependency --
//! same reasoning `devseed.rs` gives for duplicating `parse_key_hex` from
//! `apps/pos/src-tauri/src/state.rs`: this is a dev-only tool, and depending
//! on the sync crate here would pull in its network stack for one constant.
//! If that threshold changes, this duplicate goes stale silently; it is
//! flagged here so a future reader knows where to look.
//!
//! Usage:
//!     demo-assert <edge_data_dir> [outlet_id]
//!
//! Environment:
//!     HOLLER_DB_KEY_HEX   required, 64 hex chars (32 bytes) -- must be the
//!                         SAME key the database was sealed with, or Db::open
//!                         fails with a decryption error (a different key
//!                         opens nothing, it does not open a different
//!                         database -- there is no other database to open).
//!
//! Exit codes:
//!     0   every assertion passed
//!     1   the sealed database could not be opened at all (loud failure --
//!         never a silent "0 rows" because nothing could be read)
//!     2   one or more assertions failed (their names are on stdout)
//!
//! This binary reseals the database before exiting either way (`Db::close`),
//! so a run of this tool never leaves a plaintext copy behind on top of gap
//! A6's own.

use std::path::PathBuf;
use std::process::ExitCode;

use holler_edge_database::crypto::EncryptionKey;
use holler_edge_database::repo;
use holler_edge_database::Db;

/// Matches `edge/database/src/bin/devseed.rs`'s `OUTLET_ID` -- the demo reset
/// re-seeds against the same fixed devseed ids every time (seed/README.md:
/// "Fixed ids that predate this file ... keep their exact current values").
/// Overridable via the second CLI argument for a future seed that mints a
/// different outlet, but there is no such seed today.
const DEFAULT_OUTLET_ID: &str = "0191a000-0000-7000-8000-00000000000a";

/// Duplicated from `edge/sync/src/worker.rs:84`. See the module doc comment.
const OUTBOX_ATTENTION_ATTEMPTS: i64 = 3;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(data_dir_arg) = args.next() else {
        eprintln!("usage: demo-assert <edge_data_dir> [outlet_id]");
        eprintln!("  edge_data_dir: the directory holding edge.db.enc (and edge.db, if unsealed)");
        return ExitCode::from(1);
    };
    let outlet_id = args.next().unwrap_or_else(|| DEFAULT_OUTLET_ID.to_string());
    let data_dir = PathBuf::from(data_dir_arg);

    match run(&data_dir, &outlet_id) {
        Ok(true) => {
            println!("demo-assert: all checks OK");
            ExitCode::from(0)
        }
        Ok(false) => {
            eprintln!("demo-assert: one or more checks FAILED (see above)");
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("demo-assert: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(data_dir: &PathBuf, outlet_id: &str) -> Result<bool, String> {
    let key_hex = std::env::var("HOLLER_DB_KEY_HEX")
        .map_err(|_| "HOLLER_DB_KEY_HEX is not set -- cannot open the sealed edge database. \
            Set it to the SAME key the database was sealed with.".to_string())?;
    let key = parse_key_hex(&key_hex)?;

    let sealed_path = data_dir.join("edge.db.enc");
    let plaintext_path = data_dir.join("edge.db");

    if !sealed_path.exists() {
        return Err(format!(
            "no sealed database at {sealed_path:?} -- the edge devseed step must run \
             (and complete: Db::close reseals it) before assertions can be checked. \
             This is a loud failure, not zero rows: nothing was read."
        ));
    }

    let db = Db::open(&sealed_path, &plaintext_path, key)
        .map_err(|e| format!("opening {sealed_path:?}: {e}"))?;

    let mut all_ok = true;

    // 1. sync_replay_block -- ranged-stream (LEDGER/DEDUCTION_GAP) give-ups.
    let replay_blocked = repo::list_blocked_replays(db.connection(), outlet_id)
        .map_err(|e| format!("querying sync_replay_block: {e}"))?;
    all_ok &= report("sync_replay_block (blocked ranged-stream rows)", replay_blocked.len());

    // 2. stock_deduction_gap -- a sale with no resolvable recipe. Distinct
    // from grn_gap, which is NOT asserted here (ADR-019: a GRN never blocks
    // on a PO, and the demo seed legitimately produces one NO_PURCHASE_ORDER
    // row there). Bounded at repo::STOCK_DEDUCTION_GAP_REPORT_LIMIT (500) --
    // irrelevant for a pass (0 < 500) and noted here for an honest failure
    // report.
    let deduction_gaps = db
        .list_stock_deduction_gaps(outlet_id)
        .map_err(|e| format!("querying stock_deduction_gap: {e}"))?;
    let capped_note = if deduction_gaps.len() >= 500 {
        " (>= the 500-row report cap -- the true count may be higher)"
    } else {
        ""
    };
    all_ok &= report(
        &format!("stock_deduction_gap{capped_note}"),
        deduction_gaps.len(),
    );

    // 3. sync_outbox_block, blocked_at IS NOT NULL -- the general outbox's
    // durable "given up" record. This is what "zero blocked rows in
    // local_outbox" in seed/README.md and the demo-kickoff brief actually
    // means: local_outbox itself has no blocked flag (contracts 0.6.4).
    let outbox_blocked = repo::list_blocked_outbox_rows(db.connection(), outlet_id)
        .map_err(|e| format!("querying sync_outbox_block (blocked_at IS NOT NULL): {e}"))?;
    all_ok &= report(
        "sync_outbox_block blocked rows (blocked_at IS NOT NULL) -- the local_outbox check",
        outbox_blocked.len(),
    );

    // 4. PROXY for "the POS sync banner is empty". The banner also renders a
    // second, non-blocked group: rows still being retried that have failed
    // repeatedly (blocked_at IS NULL, attempts >= OUTBOX_ATTENTION_ATTEMPTS).
    // This is a database proxy, checked by this tool -- it is NOT a
    // screenshot of the banner and does not prove the React component
    // renders nothing. State that plainly rather than implying a screen was
    // looked at.
    let outbox_failing = repo::list_persistently_failing_outbox_rows(
        db.connection(),
        outlet_id,
        OUTBOX_ATTENTION_ATTEMPTS,
    )
    .map_err(|e| format!("querying sync_outbox_block (persistently failing): {e}"))?;
    all_ok &= report(
        "sync_outbox_block persistently-failing rows (PROXY for the sync banner, not the banner itself)",
        outbox_failing.len(),
    );

    // Reseal before returning either way -- a run of this tool must not add
    // a second reason a plaintext edge.db is left on disk (gap A6 already
    // leaves one from the POS's own exit path; this tool must not compound
    // it).
    db.close().map_err(|e| format!("resealing {sealed_path:?}: {e}"))?;

    Ok(all_ok)
}

/// Prints one assertion result by name with its actual count, pass or fail --
/// never a bare "checks passed". Returns whether it passed.
fn report(name: &str, count: usize) -> bool {
    if count == 0 {
        println!("{name}: 0 rows -- OK");
        true
    } else {
        println!("{name}: {count} rows -- FAIL");
        false
    }
}

/// Same 32-byte hex key parsing as `apps/pos/src-tauri/src/state.rs` and
/// `edge/database/src/bin/devseed.rs`, duplicated for the same reason
/// devseed.rs gives: this is a dev-only tool and must not change POS code
/// nor `edge/database/src/bin/devseed.rs` (owned by a parallel track today).
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

#[cfg(test)]
mod falsifier_test {
    use super::*;

    // §66: falsify the guard before trusting it. Plants a fake blocked row
    // directly and confirms `run` reports it as a FAILURE rather than
    // passing silently. Not part of the shipped binary's behaviour -- a
    // one-time proof, kept as a test so it can be re-run rather than only
    // having been run once by hand.
    #[test]
    fn reports_a_planted_blocked_row_as_failure() {
        let tmp = std::env::temp_dir().join(format!("demo-assert-falsifier-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let key_hex = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let sealed_path = tmp.join("edge.db.enc");
        let plaintext_path = tmp.join("edge.db");
        let key = parse_key_hex(key_hex).unwrap();
        let db = Db::open(&sealed_path, &plaintext_path, key).unwrap();
        db.connection()
            .execute(
                "INSERT INTO outlet (id, brand_id, name, created_at, updated_at) \
                 VALUES ('poison-outlet', 'poison-brand', 'Poison', \
                         '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        db.connection()
            .execute(
                "INSERT INTO local_outbox (id, aggregate_type, aggregate_id, event_type, payload_json, created_at) \
                 VALUES ('poison-row', 'order', 'poison-order', 'OrderCreated', '{}', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        db.connection()
            .execute(
                "INSERT INTO sync_outbox_block \
                 (outlet_id, outbox_id, aggregate_type, aggregate_id, attempts, last_error, \
                  first_attempt_at, last_attempt_at, blocked_at) \
                 VALUES ('poison-outlet', 'poison-row', 'order', 'poison-order', 5, 'planted', \
                         '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        db.close().unwrap();

        std::env::set_var("HOLLER_DB_KEY_HEX", key_hex);
        let passed = run(&tmp, "poison-outlet").unwrap();
        assert!(!passed, "a planted blocked outbox row must fail the check, not pass silently");
    }
}
