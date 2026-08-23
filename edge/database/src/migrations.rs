//! Applies the frozen contract schema files verbatim, in order.
//!
//! `packages/contracts/sqlite/0001_init.sql`, `0002_m1_identity_tables.sql`,
//! `0003_order_item_modifiers.sql` and `0004_order_canonical_fields.sql` are
//! read-only and authoritative (ADR-008, ADR-011). This module never edits,
//! reorders, or adds to their statements
//! — it only decides *whether* to run each file exactly once.
//!
//! Idempotency is tracked with SQLite's built-in `PRAGMA user_version`
//! rather than an extra bookkeeping table: this crate must not add a table
//! that is not in the frozen contracts, and `user_version` is a pragma, not
//! a schema object. `user_version == N` means migrations 1..=N have been
//! applied.

use rusqlite::Connection;

use crate::error::{DbError, DbResult};

/// The contract migration files, embedded at compile time so the crate has
/// no runtime dependency on the location of `packages/contracts` on disk.
/// Content is copied verbatim from the frozen files — do not hand-edit.
const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_init.sql",
        include_str!("../../../packages/contracts/sqlite/0001_init.sql"),
    ),
    (
        "0002_m1_identity_tables.sql",
        include_str!("../../../packages/contracts/sqlite/0002_m1_identity_tables.sql"),
    ),
    (
        "0003_order_item_modifiers.sql",
        include_str!("../../../packages/contracts/sqlite/0003_order_item_modifiers.sql"),
    ),
    (
        "0004_order_canonical_fields.sql",
        include_str!("../../../packages/contracts/sqlite/0004_order_canonical_fields.sql"),
    ),
    (
        "0005_m2_kitchen_stations_printers.sql",
        include_str!("../../../packages/contracts/sqlite/0005_m2_kitchen_stations_printers.sql"),
    ),
    (
        "0006_m3_billing.sql",
        include_str!("../../../packages/contracts/sqlite/0006_m3_billing.sql"),
    ),
    (
        "0007_menu_item_tax_profile.sql",
        include_str!("../../../packages/contracts/sqlite/0007_menu_item_tax_profile.sql"),
    ),
    (
        "0008_edge_device_credential_cache.sql",
        include_str!("../../../packages/contracts/sqlite/0008_edge_device_credential_cache.sql"),
    ),
    // 0009-0011 (ADR-016 0.4.5) were present on disk but, like 0005 before
    // them (see the regression test below), had never been added to this
    // list, so none of the three had ever actually applied to an edge
    // database: `payment` had no append-only triggers, `print_job` could
    // not reference an invoice, and `menu_item.hsn_sac` did not exist —
    // which is what let `assemble.rs:226` hard-code `hsn_sac: None` with
    // nowhere real to read a value from in the first place.
    (
        "0009_payment_append_only_triggers.sql",
        include_str!("../../../packages/contracts/sqlite/0009_payment_append_only_triggers.sql"),
    ),
    (
        "0010_print_job_invoice_ref.sql",
        include_str!("../../../packages/contracts/sqlite/0010_print_job_invoice_ref.sql"),
    ),
    (
        "0011_menu_item_hsn_sac.sql",
        include_str!("../../../packages/contracts/sqlite/0011_menu_item_hsn_sac.sql"),
    ),
    (
        "0012_printer_role.sql",
        include_str!("../../../packages/contracts/sqlite/0012_printer_role.sql"),
    ),
    // 0013-0017 (ADR-018, contracts 0.5.0) — M4 inventory. Registered in the
    // same change that created them, deliberately: 0009-0011 and 0005 before
    // them each sat on disk unregistered and therefore never applied, and the
    // symmetric check below is what now makes that impossible to repeat
    // silently. If you add a file to packages/contracts/sqlite/, it belongs
    // here in the same commit.
    (
        "0013_outlet_day_start.sql",
        include_str!("../../../packages/contracts/sqlite/0013_outlet_day_start.sql"),
    ),
    (
        "0014_menu_default_variant.sql",
        include_str!("../../../packages/contracts/sqlite/0014_menu_default_variant.sql"),
    ),
    (
        "0015_m4_inventory_config.sql",
        include_str!("../../../packages/contracts/sqlite/0015_m4_inventory_config.sql"),
    ),
    (
        "0016_m4_stock_ledger.sql",
        include_str!("../../../packages/contracts/sqlite/0016_m4_stock_ledger.sql"),
    ),
    (
        "0017_m4_stock_snapshot.sql",
        include_str!("../../../packages/contracts/sqlite/0017_m4_stock_snapshot.sql"),
    ),
    (
        "0018_immutability_enforcement.sql",
        include_str!("../../../packages/contracts/sqlite/0018_immutability_enforcement.sql"),
    ),
    (
        "0019_recipe_output.sql",
        include_str!("../../../packages/contracts/sqlite/0019_recipe_output.sql"),
    ),
    (
        "0020_recipe_ingredient_dimension.sql",
        include_str!("../../../packages/contracts/sqlite/0020_recipe_ingredient_dimension.sql"),
    ),
    (
        "0021_stock_ledger_sequence.sql",
        include_str!("../../../packages/contracts/sqlite/0021_stock_ledger_sequence.sql"),
    ),
    (
        "0022_order_item_quantity_bound.sql",
        include_str!("../../../packages/contracts/sqlite/0022_order_item_quantity_bound.sql"),
    ),
    (
        "0023_stock_count_integrity.sql",
        include_str!("../../../packages/contracts/sqlite/0023_stock_count_integrity.sql"),
    ),
    (
        "0024_fix_gap_reason_check.sql",
        include_str!("../../../packages/contracts/sqlite/0024_fix_gap_reason_check.sql"),
    ),
    (
        "0025_sync_ledger_cursors.sql",
        include_str!("../../../packages/contracts/sqlite/0025_sync_ledger_cursors.sql"),
    ),
    (
        "0026_gap_entry_seq.sql",
        include_str!("../../../packages/contracts/sqlite/0026_gap_entry_seq.sql"),
    ),
];

/// Applies any migrations not yet reflected in `PRAGMA user_version`. Safe
/// to call on every startup (idempotent): a database already at the latest
/// version is a no-op.
pub fn apply_all(conn: &Connection) -> DbResult<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let current = usize::try_from(current)
        .map_err(|_| DbError::Migration("negative user_version".to_string()))?;

    if current > MIGRATIONS.len() {
        return Err(DbError::Migration(format!(
            "database user_version {current} is ahead of the {} migrations this crate knows about",
            MIGRATIONS.len()
        )));
    }

    for (name, sql) in MIGRATIONS.iter().skip(current) {
        conn.execute_batch(sql)
            .map_err(|e| DbError::Migration(format!("applying {name}: {e}")))?;
        let applied = MIGRATIONS
            .iter()
            .position(|(n, _)| n == name)
            .expect("name is from MIGRATIONS")
            + 1;
        conn.pragma_update(None, "user_version", applied as i64)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pragma::configure_connection;

    #[test]
    fn migrations_are_idempotent() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");

        apply_all(&conn).expect("first apply");
        let version_after_first: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version_after_first, MIGRATIONS.len() as i64);

        // Re-running must not error (e.g. "table already exists") and must
        // leave the version unchanged.
        apply_all(&conn).expect("second apply is a no-op");
        let version_after_second: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version_after_second, version_after_first);
    }

    #[test]
    fn reserved_word_order_table_is_created_and_queryable() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");
        apply_all(&conn).expect("apply");

        // "order" is a reserved word; the migration quotes it and so must
        // every caller. This proves the table exists under that exact name.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM \"order\"", [], |row| row.get(0))
            .expect("querying quoted order table");
        assert_eq!(count, 0);
    }

    #[test]
    fn order_table_has_the_0004_canonical_columns() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");
        apply_all(&conn).expect("apply");

        let mut stmt = conn.prepare("PRAGMA table_info(\"order\")").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        // The rename: tax_paise no longer exists, taxes_paise does.
        assert!(
            !columns.iter().any(|c| c == "tax_paise"),
            "tax_paise must be renamed away, not left alongside taxes_paise"
        );
        for expected in [
            "taxes_paise",
            "source",
            "external_order_id",
            "payment_status",
            "payment_source",
            "confirmed_at",
            "source_payload_json",
            "schema_version",
        ] {
            assert!(
                columns.iter().any(|c| c == expected),
                "expected column {expected} on \"order\" after 0004"
            );
        }
    }

    #[test]
    fn all_milestone_1_tables_exist_after_migration() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");
        apply_all(&conn).expect("apply");

        for table in [
            "outlet",
            "device",
            "menu_category",
            "menu_item",
            "menu_item_variant",
            "menu_item_modifier",
            "order_item",
            "order_item_modifier",
            "kot",
            "local_outbox",
            "sync_state",
            "app_user",
            "restaurant_table",
            "table_session",
            "audit_event",
        ] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "expected table {table} to exist");
        }
    }

    /// Regression test for the same bug class as
    /// `all_milestone_2_kitchen_tables_exist_after_migration`, this time for
    /// 0009-0011 (ADR-016 0.4.5): all three existed on disk but were absent
    /// from `MIGRATIONS`, so `menu_item.hsn_sac` never actually existed on
    /// an edge database — silently defeating the HSN/SAC resolution this
    /// track adds to `invoice::assemble::build_invoice`, whatever the Rust
    /// code claimed to do with it.
    #[test]
    fn menu_item_has_hsn_sac_column_after_migration() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");
        apply_all(&conn).expect("apply");

        let mut stmt = conn.prepare("PRAGMA table_info(menu_item)").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            columns.iter().any(|c| c == "hsn_sac"),
            "expected column hsn_sac on menu_item after 0011"
        );
    }

    /// Regression test for the exact bug this migration's registration
    /// fixes: `0005_m2_kitchen_stations_printers.sql` existed on disk but was
    /// never in `MIGRATIONS`, so it never applied.
    #[test]
    fn all_milestone_2_kitchen_tables_exist_after_migration() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");
        apply_all(&conn).expect("apply");

        for table in [
            "station",
            "menu_item_station",
            "printer",
            "station_printer",
            "print_job",
            "kot_status_history",
        ] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "expected table {table} to exist");
        }

        let mut stmt = conn.prepare("PRAGMA table_info(\"order\")").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            columns.iter().any(|c| c == "preparation_time_minutes"),
            "expected preparation_time_minutes column on \"order\" after 0005"
        );
    }

    /// The hard-coded-count form of this check (`assert_eq!(MIGRATIONS.len(), 11)`)
    /// only caught someone bumping the constant without registering a
    /// migration. It could never catch the actual failure mode that shipped
    /// three dead migrations (0009-0011, ADR-016 0.4.5): a new
    /// `NNNN_*.sql` file landing in `packages/contracts/sqlite/` and nobody
    /// adding the matching entry to `MIGRATIONS`.
    ///
    /// This test instead compares the registered list against the directory
    /// itself, in both directions: every `*.sql` on disk must be registered,
    /// and every registered name must still exist on disk (catching a typo'd
    /// or stale entry too).
    ///
    /// The migrations are embedded at compile time via `include_str!`, so
    /// this crate has no *build* dependency on `packages/contracts` being
    /// present at a particular relative path at runtime — but this specific
    /// test does need to walk that directory to compare against it. If the
    /// directory cannot be found, that is reported as a hard failure
    /// (`panic!`), never a silent pass: a check that reads as coverage but
    /// quietly skips when the path is missing is worse than no check, per
    /// the repeated bug class this track was asked to close.
    #[test]
    fn every_contract_sqlite_file_is_registered_and_vice_versa() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let contracts_dir = std::path::Path::new(manifest_dir)
            .join("..")
            .join("..")
            .join("packages")
            .join("contracts")
            .join("sqlite");

        let entries = std::fs::read_dir(&contracts_dir).unwrap_or_else(|e| {
            panic!(
                "cannot read contracts sqlite directory at {}: {e}. \
                 This test must fail loudly, not skip, when it cannot find \
                 the directory it is supposed to verify against.",
                contracts_dir.display()
            )
        });

        let mut on_disk: Vec<String> = entries
            .map(|entry| entry.expect("readable directory entry"))
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("sql"))
            })
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        on_disk.sort();

        assert!(
            !on_disk.is_empty(),
            "found zero .sql files in {} — this almost certainly means the \
             path is wrong, not that contracts is empty",
            contracts_dir.display()
        );

        let mut registered: Vec<String> = MIGRATIONS
            .iter()
            .map(|(name, _)| name.to_string())
            .collect();
        registered.sort();

        assert_eq!(
            on_disk, registered,
            "MIGRATIONS in edge/database/src/migrations.rs must list exactly \
             the .sql files present in packages/contracts/sqlite/, no more \
             and no fewer — a file on disk but not in this list silently \
             never applies to any edge database (the 0009-0011 bug); an \
             entry in this list with no file on disk is stale"
        );
    }

    /// Tables described as APPEND-ONLY or IMMUTABLE in a contract migration
    /// with NO trigger enforcing it. A RATCHET: this list may shrink and must
    /// never grow.
    ///
    /// WHY THIS LINT EXISTS. `postgres/0007_m3_billing.sql:286` said
    /// "APPEND-ONLY" about `payment` and had nothing behind it for two
    /// contract versions, while SQLite had real triggers — the guarantee was
    /// structural where nobody has a console and prose where an engineer has a
    /// psql prompt. Writing this lint immediately found two more of the same
    /// shape, which is the argument for a guard over one more fix.
    ///
    /// A COMMENT THAT ASSERTS A PROPERTY NOTHING VERIFIES IS WORSE THAN NO
    /// COMMENT, because it stops the next reader from checking. That class is
    /// named in docs/retro.md (2026-08-20).
    /// Tables described as APPEND-ONLY or IMMUTABLE with no trigger behind the
    /// claim. A RATCHET: it may shrink and must never grow.
    ///
    /// **It is empty, and that is the point.** When this lint was written it
    /// held three entries -- `audit_event`, `cash_movement` and `invoice` --
    /// found on the very run that made the lint pass, after it had been
    /// written for a fourth (`payment`). Filing them behind a stated reason
    /// was the wrong disposition: an exemption with a reason is still a false
    /// claim sitting in the schema, and the reason makes it easier to live
    /// with rather than easier to fix. All four are enforced now
    /// (sqlite/0009, sqlite/0018, postgres/0018, postgres/0019).
    ///
    /// Leave it empty. An entry here is a claim the schema makes and does not
    /// keep.
    const UNENFORCED_IMMUTABILITY_CLAIMS: &[(&str, &str, &str)] = &[];

    /// The SQLite `invoice` immutability trigger enumerates columns, because
    /// SQLite has no whole-row comparison. That enumeration is itself a claim
    /// that could quietly become false: add a column to `invoice` and it is
    /// unprotected, silently, with every test still green.
    ///
    /// So the enumeration is checked against reality. The PostgreSQL mirror
    /// uses `to_jsonb(OLD) <> to_jsonb(NEW)` and covers new columns
    /// automatically, which is why only this side needs the guard.
    #[test]
    fn invoice_immutability_trigger_covers_every_column() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");
        apply_all(&conn).expect("apply");

        let mut stmt = conn.prepare("PRAGMA table_info(invoice)").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            !columns.is_empty(),
            "invoice has no columns -- wrong table?"
        );

        let trigger_sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='trigger' \
                 AND name='invoice_is_immutable_except_cancellation'",
                [],
                |row| row.get(0),
            )
            .expect("the invoice immutability trigger must exist");

        // What a legal cancellation is allowed to change. The first three are
        // the cancellation itself; the last three are sync bookkeeping -- a
        // cancellation IS a new version to replay, so version, sync_status and
        // updated_at necessarily move with it.
        //
        // This list was written with only the first three, and this test is
        // what caught the omission: `invoice` carries the sync trio, they were
        // absent from both the trigger and this allow-list, and the guard
        // refused to accept either answer until one was chosen deliberately.
        // Nothing else can move -- the trigger still requires
        // ISSUED -> CANCELLED for ANY update, so these three cannot be bumped
        // on their own.
        let mutable = [
            "status",
            "cancelled_at",
            "cancelled_reason",
            "updated_at",
            "version",
            "sync_status",
        ];

        for column in &columns {
            if mutable.contains(&column.as_str()) {
                continue;
            }
            assert!(
                trigger_sql.contains(&format!("NEW.{column} ")),
                "invoice.{column} is not compared in \
                 invoice_is_immutable_except_cancellation, so it can be \
                 changed on a cancellation without the trigger noticing. Add \
                 `AND NEW.{column} IS OLD.{column}` to \
                 sqlite/0018_immutability_enforcement.sql. (SQLite cannot \
                 compare whole rows; the PostgreSQL mirror can, and covers new \
                 columns automatically.)"
            );
        }
    }

    /// Comments whose APPEND-ONLY / IMMUTABLE wording is about the SYNC
    /// PROTOCOL rather than the storage engine, each with the reason it is not
    /// a storage claim.
    ///
    /// This list exists because the alternative -- silently skipping any line
    /// containing "replay" -- is an undeclared escape hatch, and an undeclared
    /// escape hatch is where the next false claim lives. Someone writes
    /// "append-only replay" about a table that genuinely should be immutable,
    /// and the lint waves it through leaving no trace that it did. Declared,
    /// the same wording fails until a human writes down why it is prose.
    const PROTOCOL_PROSE_CLAIMS: &[(&str, &str, &str)] = &[
        (
            "sqlite",
            "table_session",
            "0002:40 'replayed edge→cloud append-only' describes how the row              CROSSES the boundary. The row itself is mutated constantly at the              edge (it is live table state) and upserted by version on arrival,              so a trigger here would be wrong, not missing.",
        ),
        (
            "postgres",
            "table_session",
            "0002:74, the same protocol statement from the cloud side: 'the              cloud never mutates these rows' is about replay authority, not              row immutability.",
        ),
        (
            "postgres",
            "invoice",
            "0007:142 'edge→cloud, replay only, append-only'. The cloud never              originates an invoice edit; the row IS updated on the legal              ISSUED->CANCELLED transition, which postgres/0019 now enforces              precisely rather than blanket-forbidding.",
        ),
    ];

    /// Every APPEND-ONLY / IMMUTABLE claim must have enforcement behind it, or
    /// be declared above as a known gap with a reason.
    ///
    /// Deliberately narrow about what counts as a claim: prose describing
    /// REPLAY semantics ("replayed edge→cloud append-only", about
    /// `table_session`, which is mutated constantly) is a statement about the
    /// sync protocol, not about the storage engine. Only a claim attached to a
    /// table definition in the same file is treated as a storage claim.
    #[test]
    fn every_append_only_claim_has_a_trigger_behind_it() {
        let contracts = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("packages")
            .join("contracts");

        let mut unenforced: Vec<(String, String)> = Vec::new();
        let mut protocol_prose: Vec<(String, String)> = Vec::new();

        for store in ["sqlite", "postgres"] {
            let dir = contracts.join(store);

            // Collect every table that has an immutability trigger anywhere in
            // the store, and every table claimed immutable in a comment.
            let mut triggered: Vec<String> = Vec::new();
            // (table, was the claim worded as being about REPLAY)
            let mut claimed: Vec<(String, bool)> = Vec::new();

            let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
                .map(|e| e.expect("readable directory entry").path())
                .filter(|p| {
                    p.extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("sql"))
                })
                .collect();
            files.sort();

            assert!(
                !files.is_empty(),
                "no .sql files found in {}",
                dir.display()
            );

            for path in &files {
                let sql = std::fs::read_to_string(path).expect("readable migration");

                // Trigger targets: "... ON <table>" inside a CREATE TRIGGER.
                for line in sql.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("--") {
                        continue;
                    }
                    let upper = trimmed.to_uppercase();
                    if upper.contains("UPDATE") || upper.contains("DELETE") {
                        if let Some(idx) = upper.find(" ON ") {
                            if let Some(table) = trimmed[idx + 4..].split_whitespace().next() {
                                triggered.push(table.trim_matches('"').to_string());
                            }
                        }
                    }
                }

                // Claims: a comment naming append-only/immutable, attributed to
                // the table defined nearest below it in the same file.
                let lines: Vec<&str> = sql.lines().collect();
                for (i, line) in lines.iter().enumerate() {
                    let trimmed = line.trim();
                    if !trimmed.starts_with("--") {
                        continue;
                    }
                    let upper = trimmed.to_uppercase();
                    if !(upper.contains("APPEND-ONLY") || upper.contains("IMMUTABLE")) {
                        continue;
                    }
                    // A claim about REPLAY is about the sync protocol, not the
                    // storage engine -- but that escape hatch is NOT blanket.
                    //
                    // A keyword that silently excuses a line is exactly where
                    // the next false claim would live: someone writes
                    // "append-only replay" about a table that really should be
                    // immutable, and this lint waves it through with no record
                    // that it did. So the exclusion is a DECLARED LIST with a
                    // stated reason per entry, the same discipline as
                    // SINGLE_STORE_MIGRATIONS -- and an undeclared use of the
                    // word fails below rather than being skipped here.
                    let is_replay_worded = upper.contains("REPLAY");
                    // Attribute to the NEAREST `CREATE TABLE` by line
                    // distance, above or below.
                    //
                    // Not "the next one below", which is what this test did
                    // when first written -- and it promptly mis-attributed
                    // PostgreSQL's trigger comments, which sit AFTER their
                    // table (plpgsql needs the table to exist first), to
                    // whichever table happened to be defined next. The guard
                    // failed on its own bug rather than on a schema defect,
                    // which is the whole argument for falsifying guards too.
                    let table_at = |idx: usize| -> Option<String> {
                        let lt = lines[idx].trim();
                        if lt.starts_with("--") {
                            return None;
                        }
                        lt.to_uppercase()
                            .strip_prefix("CREATE TABLE ")
                            .and_then(|rest| rest.split_whitespace().next().map(str::to_string))
                            .map(|name| name.trim_matches('"').to_lowercase())
                    };

                    let below = (i..lines.len()).find_map(|j| table_at(j).map(|t| (j - i, t)));
                    let above = (0..=i).rev().find_map(|j| table_at(j).map(|t| (i - j, t)));

                    let nearest = match (above, below) {
                        (Some((da, ta)), Some((db, tb))) => Some(if da <= db { ta } else { tb }),
                        (Some((_, t)), None) | (None, Some((_, t))) => Some(t),
                        (None, None) => None, // file-level remark, no table
                    };

                    if let Some(table) = nearest {
                        claimed.push((table, is_replay_worded));
                    }
                }
            }

            let triggered: Vec<String> = triggered.iter().map(|t| t.to_lowercase()).collect();

            for (table, is_replay_worded) in &claimed {
                if *is_replay_worded {
                    // Protocol prose, not a storage claim -- but only if it is
                    // declared as such. Otherwise it is an undeclared escape
                    // hatch, which is what this branch exists to prevent.
                    protocol_prose.push((store.to_string(), table.clone()));
                    continue;
                }
                if !triggered.contains(table) {
                    unenforced.push((store.to_string(), table.clone()));
                }
            }
        }

        unenforced.sort();
        unenforced.dedup();
        protocol_prose.sort();
        protocol_prose.dedup();

        for (store, table) in &protocol_prose {
            assert!(
                PROTOCOL_PROSE_CLAIMS
                    .iter()
                    .any(|(s, t, _)| s == store && t == table),
                "{store}.{table} carries APPEND-ONLY/IMMUTABLE wording that                  mentions replay, so this lint treated it as protocol prose                  rather than a storage claim -- but it is not declared in                  PROTOCOL_PROSE_CLAIMS. Declare it WITH THE REASON it is not a                  storage claim, or reword it. An undeclared escape hatch is                  where the next false claim will live."
            );
        }

        for (store, table, reason) in PROTOCOL_PROSE_CLAIMS {
            assert!(
                protocol_prose.iter().any(|(s, t)| s == store && t == table),
                "PROTOCOL_PROSE_CLAIMS still lists {store}.{table} ({reason}),                  but no such claim is present any more. Remove the stale entry."
            );
        }

        for (store, table) in &unenforced {
            assert!(
                UNENFORCED_IMMUTABILITY_CLAIMS
                    .iter()
                    .any(|(s, t, _)| s == store && t == table),
                "{store}.{table} is described as APPEND-ONLY or IMMUTABLE in a \
                 contract migration, with no trigger enforcing it. Either add \
                 the trigger, fix the wording, or declare it in \
                 UNENFORCED_IMMUTABILITY_CLAIMS with the reason. A comment \
                 asserting a property nothing verifies is worse than no \
                 comment: it stops the next reader from checking."
            );
        }

        // The other direction: a declared gap that has since been closed must
        // be removed from the list, so the list never drifts into fiction --
        // the same ratchet discipline as the gen_random_uuid baseline.
        for (store, table, reason) in UNENFORCED_IMMUTABILITY_CLAIMS {
            assert!(
                unenforced.iter().any(|(s, t)| s == store && t == table),
                "UNENFORCED_IMMUTABILITY_CLAIMS still lists {store}.{table} \
                 ({reason}), but it is now enforced or no longer claimed. \
                 Remove the entry so the list keeps meaning what it says."
            );
        }
    }

    /// Known `DEFAULT gen_random_uuid()` columns in the PostgreSQL contracts,
    /// all of them in `0001_init.sql`. A RATCHET: this number may go DOWN and
    /// must never go up.
    ///
    /// §74 requires ids be app-generated UUIDv7/ULID, and the contract rubric
    /// forbids DB-side random defaults. A database-side default is not merely
    /// stylistically off: it is a second id authority, it produces UUIDv4
    /// (unsorted, so it indexes badly on the very tables that grow fastest),
    /// and for an edge-authoritative row it is actively wrong, because the id
    /// is minted at the outlet and replayed.
    ///
    /// Retrofitting is one ALTER per table and the app already supplies every
    /// id, but it is not this change's job. Filed in docs/backlog-m2.md with
    /// the trigger "the next migration that touches the table". This lint is
    /// what makes that trigger safe to wait for: the debt cannot grow while it
    /// waits.
    const POSTGRES_DB_SIDE_UUID_DEFAULT_BASELINE: usize = 8;

    /// The ratchet. Fails if a new DB-side random id default appears anywhere
    /// in the PostgreSQL contracts, and fails just as loudly if the baseline
    /// above is stale after a retrofit — so the number tracks reality rather
    /// than drifting into fiction.
    #[test]
    fn postgres_db_side_uuid_defaults_only_ever_decrease() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("packages")
            .join("contracts")
            .join("postgres");

        let mut found = 0usize;
        let mut files: Vec<String> = Vec::new();

        for entry in
            std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        {
            let entry = entry.expect("readable directory entry");
            if !entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("sql"))
            {
                continue;
            }
            let sql = std::fs::read_to_string(entry.path()).expect("readable migration");
            // Comments discussing the pattern are not uses of it -- several
            // migrations, including this milestone's, explain in prose why
            // they do NOT use it.
            let count = sql
                .lines()
                .filter(|line| !line.trim_start().starts_with("--"))
                .filter(|line| line.contains("gen_random_uuid()"))
                .count();
            if count > 0 {
                files.push(format!("{}: {count}", entry.file_name().to_string_lossy()));
                found += count;
            }
        }

        files.sort();

        assert!(
            found <= POSTGRES_DB_SIDE_UUID_DEFAULT_BASELINE,
            "DEFAULT gen_random_uuid() count rose to {found} (baseline \
             {POSTGRES_DB_SIDE_UUID_DEFAULT_BASELINE}). Ids are app-generated \
             UUIDv7 per §74; a DB-side random default is a second id authority \
             and is actively wrong for an edge-authoritative row, whose id is \
             minted at the outlet and replayed. Found in: {files:?}"
        );

        assert_eq!(
            found, POSTGRES_DB_SIDE_UUID_DEFAULT_BASELINE,
            "DEFAULT gen_random_uuid() count fell to {found} -- good. Lower \
             POSTGRES_DB_SIDE_UUID_DEFAULT_BASELINE to {found} so the ratchet \
             holds the new ground instead of allowing a silent regression back \
             up to {POSTGRES_DB_SIDE_UUID_DEFAULT_BASELINE}."
        );
    }

    /// Migrations that exist in ONE store on purpose, each with the reason it
    /// does. Keyed by the stem after the four-digit prefix, because the two
    /// stores have drifted out of numeric step (postgres/0003_refresh_token
    /// has no SQLite twin, so every later pair is offset).
    ///
    /// WHY THIS LIST EXISTS. Before it, the asymmetry was real but undeclared:
    /// `invoice_sequence` is edge-local by an explicit ADR-016 decision, and
    /// **nothing anywhere asserted that it must not gain a PostgreSQL mirror**.
    /// The registration check above reads only the SQLite directory, so a
    /// future author "tidying up" the missing file would have broken a
    /// load-bearing authority decision and no test would have objected.
    ///
    /// The declaration IS the guard: adding the missing counterpart now fails
    /// this test, and the failure names the reason the file is missing.
    const SINGLE_STORE_MIGRATIONS: &[(&str, &str, &str)] = &[
        (
            "sqlite",
            "edge_device_credential_cache.sql",
            "ADR-017: the edge-cached half of device enrollment. The cloud's \
             own device_credential lives in postgres/device_enrollment.sql; \
             this is the cache, and caching is an edge concern.",
        ),
        // `payment_append_only_triggers.sql` was listed here as SQLite-only.
        // It was never a deliberate asymmetry — it was a gap wearing one, and
        // declaring it is what made that visible. postgres/0018 now mirrors
        // it, so the entry is gone and this guard is what would fail if the
        // mirror were ever removed again.
        (
            "sqlite",
            "print_job_invoice_ref.sql",
            "ADR-016 0.4.5: print_job is EDGE-LOCAL and deliberately absent \
             from AggregateType. A spool job never crosses to the cloud, so \
             there is nothing for a mirror to hold.",
        ),
        (
            "sqlite",
            "m4_stock_snapshot.sql",
            "ADR-018 §9: stock_balance_snapshot is an edge-local derived \
             projection. The cloud MAY re-derive its own stock view by summing \
             the ingested ledger; mirroring the edge's projection would make \
             it a second authority on stock, the same mistake mirroring \
             invoice_sequence would make about invoice numbers (§33).",
        ),
        (
            "sqlite",
            "stock_ledger_sequence.sql",
            "ADR-018 0.5.3: the entry_seq counter is EDGE-LOCAL, the              invoice_sequence precedent. Mirroring it would make the cloud a              second minter of ordering marks for a stream the edge owns — and              the mark is what the cloud's own gap detection relies on being              edge-authored. The PostgreSQL twin (0022) carries only the              magnitude bound half of that migration.",
        ),
        (
            "postgres",
            "quantity_magnitude_bound.sql",
            "ADR-018 0.5.3: the SQLite half of this bound lives inside              0021_stock_ledger_sequence.sql, because SQLite cannot ADD              CONSTRAINT and needs triggers instead. Same rule, different file              name, so the stem match cannot pair them.",
        ),
        (
            "sqlite",
            "sync_ledger_cursors.sql",
            "ADR-018 0.5.8: sync_state's two ranged-replay cursors and the              stock_deduction_gap_sequence counter are EDGE-LOCAL, the              invoice_sequence/stock_ledger_sequence precedent. A cursor              records how far THIS outlet has replayed; the cloud derives its              own high-water mark from what it actually stored, and mirroring              the edge's cursor would make it a second authority on the edge's              progress. Note what is NOT in this file:              stock_deduction_gap.entry_seq ships as 0026 in BOTH stores,              because the cloud receives it and checks contiguity against it.",
        ),
        (
            "postgres",
            "ledger_replay_gap.sql",
            "ADR-018 0.5.8: a record of what the CLOUD observed about a              stream it received -- a hole between its high-water mark and an              arriving entry_seq. Cloud-only, the refresh_token precedent: the              edge cannot author it, and an edge reporting on its own losses              would be the wrong authority for the fact.",
        ),
        (
            "postgres",
            "refresh_token.sql",
            "ADR-012: refresh tokens are cloud-only and deliberately not an \
             AggregateType. An edge node never issues or rotates one.",
        ),
        (
            "postgres",
            "device_enrollment.sql",
            "ADR-017: device_credential is cloud-only. The plaintext token is \
             returned once at enrollment; the edge holds only the cache above.",
        ),
        (
            "postgres",
            "device_credential_config_version.sql",
            "ADR-017 0.4.5: per-row config_version on the cloud-only \
             device_credential, so /sync/config's since_version filter reaches \
             it. Amends a cloud-only table.",
        ),
    ];

    /// Every SQLite/PostgreSQL asymmetry must be a DECLARED one.
    ///
    /// This does not require the two stores to match — they legitimately do
    /// not. It requires that each place they differ is listed above with a
    /// reason, so an undeclared divergence fails loudly and a deliberate one
    /// cannot be silently "fixed".
    #[test]
    fn every_single_store_migration_is_declared() {
        fn stems(dir: &std::path::Path) -> Vec<String> {
            std::fs::read_dir(dir)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
                .map(|entry| entry.expect("readable directory entry"))
                .filter(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("sql"))
                })
                .map(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    // Strip the four-digit prefix and its underscore; the two
                    // stores are numbered independently.
                    name.get(5..).unwrap_or(&name).to_string()
                })
                .collect()
        }

        let contracts = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("packages")
            .join("contracts");

        let sqlite = stems(&contracts.join("sqlite"));
        let postgres = stems(&contracts.join("postgres"));

        assert!(
            !sqlite.is_empty() && !postgres.is_empty(),
            "found zero .sql files in one of the contract stores — the path is \
             almost certainly wrong, not the directory empty"
        );

        let declared = |store: &str, stem: &str| {
            SINGLE_STORE_MIGRATIONS
                .iter()
                .any(|(s, name, _)| *s == store && *name == stem)
        };

        for stem in &sqlite {
            assert!(
                postgres.contains(stem) || declared("sqlite", stem),
                "sqlite/*_{stem} has no PostgreSQL counterpart and is not \
                 declared in SINGLE_STORE_MIGRATIONS. Either add the mirror, \
                 or declare the asymmetry WITH THE REASON it is deliberate."
            );
        }

        for stem in &postgres {
            assert!(
                sqlite.contains(stem) || declared("postgres", stem),
                "postgres/*_{stem} has no SQLite counterpart and is not \
                 declared in SINGLE_STORE_MIGRATIONS. Either add the mirror, \
                 or declare the asymmetry WITH THE REASON it is deliberate."
            );
        }

        // The guard in the other direction: a declared single-store migration
        // that has since GAINED its counterpart is a decision that was quietly
        // reversed. Fail, so the reversal is deliberate and the reason above
        // is deleted along with it.
        for (store, stem, reason) in SINGLE_STORE_MIGRATIONS {
            let (own, other) = match *store {
                "sqlite" => (&sqlite, &postgres),
                _ => (&postgres, &sqlite),
            };
            assert!(
                own.contains(&stem.to_string()),
                "SINGLE_STORE_MIGRATIONS lists {store}/*_{stem}, which no \
                 longer exists. Remove the stale entry."
            );
            assert!(
                !other.contains(&stem.to_string()),
                "{store}/*_{stem} was declared single-store because: {reason}\n\
                 A counterpart now exists in the other store. If that is \
                 intended, the authority decision behind it has changed and \
                 the ADR must say so — do not silently delete this entry."
            );
        }
    }

    #[test]
    fn all_milestone_3_billing_tables_exist_after_migration() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");
        apply_all(&conn).expect("apply");

        for table in [
            "compliance_version",
            "tax_profile",
            "tax_rule",
            "outlet_fiscal_profile",
            "invoice_series",
            "discount_definition",
            "invoice_sequence",
            "invoice",
            "invoice_line",
            "cash_shift",
            "payment",
            "payment_allocation",
            "cash_movement",
        ] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "expected table {table} to exist after 0006");
        }

        let mut stmt = conn.prepare("PRAGMA table_info(\"order\")").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            columns.iter().any(|c| c == "display_number"),
            "expected display_number column on \"order\" after 0006"
        );
    }

    /// The edge-cached device credential (0.4.3, ADR-017 amendment). Its
    /// absence would mean a KDS cannot re-authenticate with the uplink down,
    /// which the T4 gate ruled a blocker: ticket visibility is a core
    /// operation and core operations run without internet.
    #[test]
    fn device_credential_cache_exists_after_migration() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");
        apply_all(&conn).expect("apply");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = 'device_credential_cache'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "expected device_credential_cache after 0008");

        let mut stmt = conn
            .prepare("PRAGMA table_info(device_credential_cache)")
            .unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        // Named credential_hash, never token_hash: it holds a verifier to check
        // a presented token against, not a bearer token. The contract drift
        // guards treat token_hash as bearer material and are right to.
        assert!(
            columns.iter().any(|c| c == "credential_hash"),
            "the verifier column must be credential_hash"
        );
        assert!(
            !columns.iter().any(|c| c == "token_hash"),
            "device_credential_cache must not name a column token_hash"
        );
        // A revoked credential still syncs and is still stored: while offline,
        // a missing row is indistinguishable from one not yet synced, so
        // liveness must never be inferred from absence.
        for expected in ["revoked_at", "expires_at"] {
            assert!(
                columns.iter().any(|c| c == expected),
                "expected {expected} so a dead credential can be recognised offline"
            );
        }
    }
}

/// Storage-level enforcement of the ADR-016 money rules.
///
/// These assert the schema itself rejects a malformed bill, which is a
/// stronger guarantee than a tax engine that computes correctly today: the
/// §66 property suite proves the engine produces right answers, this proves
/// the database refuses to hold a wrong one regardless of which caller wrote
/// it. Both halves are needed — an engine can be bypassed, a CHECK cannot.
///
/// Each rejection test names the specific defect it targets, so a constraint
/// silently dropped from the migration fails a test that says why it mattered.
#[cfg(test)]
mod m3_billing_constraints {
    use super::*;
    use crate::pragma::configure_connection;
    use rusqlite::params;

    const SEED: &str = r#"
INSERT INTO outlet (id,brand_id,name,created_at,updated_at) VALUES
 ('out-1','brand-1','Pune','2026-08-12T00:00:00Z','2026-08-12T00:00:00Z'),
 ('out-2','brand-1','Mumbai','2026-08-12T00:00:00Z','2026-08-12T00:00:00Z');
INSERT INTO device (id,outlet_id,kind,name,created_at) VALUES
 ('dev-1','out-1','POS','POS1','2026-08-12T00:00:00Z');
INSERT INTO app_user (id,tenant_id,outlet_id,email,full_name,password_hash,is_active,
 permissions_json,config_version,updated_at) VALUES
 ('usr-1','ten-1','out-1','asha@example.in','Asha','argon2id$dummy',1,'[]',1,
  '2026-08-12T00:00:00Z');
INSERT INTO "order" (id,outlet_id,device_id,order_type,status,created_at,updated_at) VALUES
 ('ord-1','out-1','dev-1','DINE_IN','BILLED','2026-08-12T00:00:00Z','2026-08-12T00:00:00Z'),
 ('ord-2','out-2','dev-1','TAKEAWAY','BILLED','2026-08-12T00:00:00Z','2026-08-12T00:00:00Z');
INSERT INTO compliance_version (id,outlet_id,label,effective_from,config_version) VALUES
 ('cv-1','out-1','GST 2026-04','2026-04-01T00:00:00Z',1);
INSERT INTO invoice_series (id,outlet_id,code,prefix_template,reset_policy,padding_width,
 is_active,config_version) VALUES
 ('ser-1','out-1','SALES','FY{FY}/{OUTLET}/','FY',6,1,1),
 ('ser-2','out-2','SALES','FY{FY}/{OUTLET}/','FY',6,1,1);
INSERT INTO cash_shift (id,outlet_id,device_id,cashier_user_id,status,opened_at,
 opening_cash_paise,business_date,created_at,updated_at) VALUES
 ('cs-1','out-1','dev-1','usr-1','OPEN','2026-08-12T08:00:00Z',200000,'2026-08-12',
  '2026-08-12T08:00:00Z','2026-08-12T08:00:00Z');
"#;

    fn seeded() -> Connection {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");
        apply_all(&conn).expect("apply");
        conn.execute_batch(SEED).expect("seed");
        conn
    }

    /// One invoice insert. Every money field is explicit so a test reads as
    /// the arithmetic it is asserting.
    #[allow(clippy::too_many_arguments)]
    fn insert_invoice(
        conn: &Connection,
        id: &str,
        outlet: &str,
        order_id: &str,
        series: &str,
        number: &str,
        taxable: i64,
        cgst: i64,
        sgst: i64,
        round_off: i64,
        grand_total: i64,
    ) -> rusqlite::Result<usize> {
        conn.execute(
            r#"INSERT INTO invoice
               (id,outlet_id,order_id,series_id,invoice_number,invoice_date,business_date,
                status,place_of_supply_state_code,subtotal_paise,discount_paise,
                taxable_value_paise,cgst_paise,sgst_paise,round_off_paise,grand_total_paise,
                compliance_version_id,tax_snapshot_json,fiscal_profile_json,channel,
                tax_liability_party,created_by_user_id,created_at,updated_at)
               VALUES (?1,?2,?3,?4,?5,'2026-08-12T10:00:00Z','2026-08-12','ISSUED','27',
                       ?6,0,?6,?7,?8,?9,?10,'cv-1','{}','{}','POS','RESTAURANT','usr-1',
                       '2026-08-12T10:00:00Z','2026-08-12T10:00:00Z')"#,
            params![
                id,
                outlet,
                order_id,
                series,
                number,
                taxable,
                cgst,
                sgst,
                round_off,
                grand_total
            ],
        )
    }

    #[test]
    fn a_correct_bill_stores() {
        let conn = seeded();
        // ₹1000 taxable + 2.5% CGST + 2.5% SGST = ₹1050 exactly, no round-off.
        insert_invoice(
            &conn,
            "inv-1",
            "out-1",
            "ord-1",
            "ser-1",
            "FY26/PNQ/000001",
            100_000,
            2_500,
            2_500,
            0,
            105_000,
        )
        .expect("a correctly computed bill must store");
    }

    #[test]
    fn a_correct_bill_with_round_off_stores() {
        let conn = seeded();
        // 99950 + 2499 + 2499 = 104948 → nearest rupee 104900, round_off -48.
        insert_invoice(
            &conn,
            "inv-2",
            "out-1",
            "ord-1",
            "ser-1",
            "FY26/PNQ/000002",
            99_950,
            2_499,
            2_499,
            -48,
            104_900,
        )
        .expect("a bill carrying a legitimate round-off must store");
    }

    /// Targets: a tax engine that computes components correctly but writes a
    /// grand total from a separate code path that drifted from them.
    #[test]
    fn grand_total_must_equal_component_sum_plus_round_off() {
        let conn = seeded();
        let err = insert_invoice(
            &conn,
            "inv-3",
            "out-1",
            "ord-1",
            "ser-1",
            "FY26/PNQ/000003",
            100_000,
            2_500,
            2_500,
            0,
            106_000,
        )
        .expect_err("a grand total that does not equal its parts must be rejected");
        assert!(
            format!("{err}").contains("CHECK"),
            "expected CHECK failure, got {err}"
        );
    }

    /// Targets: round-off used to absorb an arithmetic error. Rounding to the
    /// nearest rupee can never move the total by more than half a rupee, so a
    /// larger value means something else is wrong.
    #[test]
    fn round_off_is_bounded_to_half_a_rupee() {
        let conn = seeded();
        let err = insert_invoice(
            &conn,
            "inv-4",
            "out-1",
            "ord-1",
            "ser-1",
            "FY26/PNQ/000004",
            99_940,
            2_500,
            2_500,
            60,
            105_000,
        )
        .expect_err("round-off beyond ±50 paise must be rejected");
        assert!(
            format!("{err}").contains("CHECK"),
            "expected CHECK failure, got {err}"
        );
    }

    /// Targets: the round-off field being present but never applied — the
    /// total still settling in paise, which a cash drawer cannot pay out.
    #[test]
    fn grand_total_must_be_whole_rupees() {
        let conn = seeded();
        let err = insert_invoice(
            &conn,
            "inv-5",
            "out-1",
            "ord-1",
            "ser-1",
            "FY26/PNQ/000005",
            99_999,
            2_500,
            2_500,
            0,
            104_999,
        )
        .expect_err("a grand total that is not a whole rupee must be rejected");
        assert!(
            format!("{err}").contains("CHECK"),
            "expected CHECK failure, got {err}"
        );
    }

    /// §33: "Never generate duplicate invoice numbers."
    #[test]
    fn invoice_number_is_unique_within_an_outlet_series() {
        let conn = seeded();
        insert_invoice(
            &conn,
            "inv-6",
            "out-1",
            "ord-1",
            "ser-1",
            "FY26/PNQ/000001",
            100_000,
            2_500,
            2_500,
            0,
            105_000,
        )
        .expect("first issue");
        let err = insert_invoice(
            &conn,
            "inv-7",
            "out-1",
            "ord-1",
            "ser-1",
            "FY26/PNQ/000001",
            100_000,
            2_500,
            2_500,
            0,
            105_000,
        )
        .expect_err("a duplicate invoice number must be rejected");
        assert!(
            format!("{err}").contains("UNIQUE"),
            "expected UNIQUE failure, got {err}"
        );
    }

    /// The other half of the uniqueness rule, and the one a global unique
    /// index would silently break: two outlets numbering independently is
    /// correct behaviour (CLAUDE.md rubric — uniqueness is tenant-scoped).
    #[test]
    fn the_same_invoice_number_may_exist_at_another_outlet() {
        let conn = seeded();
        insert_invoice(
            &conn,
            "inv-8",
            "out-1",
            "ord-1",
            "ser-1",
            "FY26/PNQ/000001",
            100_000,
            2_500,
            2_500,
            0,
            105_000,
        )
        .expect("outlet 1 issue");
        insert_invoice(
            &conn,
            "inv-9",
            "out-2",
            "ord-2",
            "ser-2",
            "FY26/PNQ/000001",
            100_000,
            2_500,
            2_500,
            0,
            105_000,
        )
        .expect("the same number at a different outlet must be allowed");
    }

    /// Targets: a shift closed by a UI path that forgot to record the count,
    /// leaving a register that can never be reconciled.
    #[test]
    fn a_closed_shift_must_carry_its_counted_cash() {
        let conn = seeded();
        let err = conn
            .execute(
                r#"INSERT INTO cash_shift (id,outlet_id,device_id,cashier_user_id,status,opened_at,
                opening_cash_paise,business_date,created_at,updated_at)
               VALUES ('cs-bad','out-1','dev-1','usr-1','CLOSED','2026-08-12T08:00:00Z',200000,
                       '2026-08-12','2026-08-12T08:00:00Z','2026-08-12T08:00:00Z')"#,
                [],
            )
            .expect_err("a CLOSED shift without a counted total must be rejected");
        assert!(
            format!("{err}").contains("CHECK"),
            "expected CHECK failure, got {err}"
        );
    }

    /// §39 requires a reason for a variance. Targets a drawer that comes up
    /// short and is closed with no explanation recorded.
    #[test]
    fn a_cash_variance_requires_a_reason() {
        let conn = seeded();
        let err = conn
            .execute(
                r#"INSERT INTO cash_shift (id,outlet_id,device_id,cashier_user_id,status,opened_at,
                opening_cash_paise,closed_at,expected_cash_paise,actual_cash_paise,
                variance_paise,business_date,created_at,updated_at)
               VALUES ('cs-bad2','out-1','dev-1','usr-1','CLOSED','2026-08-12T08:00:00Z',200000,
                       '2026-08-12T23:00:00Z',500000,499000,-1000,'2026-08-12',
                       '2026-08-12T08:00:00Z','2026-08-12T23:00:00Z')"#,
                [],
            )
            .expect_err("a non-zero variance with no reason must be rejected");
        assert!(
            format!("{err}").contains("CHECK"),
            "expected CHECK failure, got {err}"
        );
    }

    /// Targets: cash-drawer fields leaking onto a card or UPI tender, which
    /// would corrupt the expected-cash derivation for the whole shift.
    #[test]
    fn tendered_cash_is_only_meaningful_on_a_cash_tender() {
        let conn = seeded();
        let err = conn
            .execute(
                r#"INSERT INTO payment (id,outlet_id,order_id,cash_shift_id,method,status,
                amount_paise,tendered_paise,created_by_user_id,created_at,updated_at)
               VALUES ('pay-bad','out-1','ord-1','cs-1','UPI','CAPTURED',105000,105000,'usr-1',
                       '2026-08-12T10:30:00Z','2026-08-12T10:30:00Z')"#,
                [],
            )
            .expect_err("tendered_paise on a non-cash tender must be rejected");
        assert!(
            format!("{err}").contains("CHECK"),
            "expected CHECK failure, got {err}"
        );
    }

    /// Targets: a half-populated discount row reaching the tax engine, where
    /// "20% or ₹50?" has no defined answer.
    #[test]
    fn a_percent_discount_cannot_also_carry_an_amount() {
        let conn = seeded();
        let err = conn
            .execute(
                r#"INSERT INTO discount_definition (id,outlet_id,code,name,scope,method,
                value_bps,value_paise,requires_reason,is_active,effective_from,config_version)
               VALUES ('dsc-bad','out-1','STAFF','Staff 20%','BILL','PERCENT',2000,5000,0,1,
                       '2026-04-01T00:00:00Z',1)"#,
                [],
            )
            .expect_err("a PERCENT discount carrying an amount must be rejected");
        assert!(
            format!("{err}").contains("CHECK"),
            "expected CHECK failure, got {err}"
        );
    }
}

/// Contracts 0.5.8 — the ranged-replay migrations, falsified against a
/// database that ALREADY HAS ROWS.
///
/// WHY THAT QUALIFIER IS THE WHOLE TEST. `ADD COLUMN entry_seq NOT NULL
/// DEFAULT 0` under `UNIQUE (outlet_id, entry_seq)` passes on an empty table
/// and fails on the second gap row of any outlet — and it fails at open, so
/// the edge database will not start. A migration test that seeds nothing
/// proves only that the syntax parses.
#[cfg(test)]
mod m4_ranged_sync_migration {
    use super::*;
    use crate::pragma::configure_connection;

    const SEED_OUTLETS: &str = r#"
INSERT INTO outlet (id,brand_id,name,created_at,updated_at) VALUES
 ('out-1','brand-1','Pune','2026-08-12T00:00:00Z','2026-08-12T00:00:00Z'),
 ('out-2','brand-1','Mumbai','2026-08-12T00:00:00Z','2026-08-12T00:00:00Z');
"#;

    /// Applies every migration strictly BEFORE `name`, leaving the database
    /// at exactly the version an outlet upgrading into 0.5.8 would be at.
    fn apply_up_to_but_not(conn: &Connection, name: &str) {
        let stop = MIGRATIONS
            .iter()
            .position(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} is not in MIGRATIONS"));
        for (n, sql) in MIGRATIONS.iter().take(stop) {
            conn.execute_batch(sql)
                .unwrap_or_else(|e| panic!("applying {n}: {e}"));
        }
        conn.pragma_update(None, "user_version", stop as i64)
            .expect("set user_version");
    }

    fn seed_pre_upgrade_gaps(conn: &Connection) {
        conn.execute_batch(SEED_OUTLETS).expect("seed outlets");
        // Two gaps for ONE outlet — the row a constant default breaks on —
        // deliberately inserted out of occurrence order, so the backfill has
        // to sort rather than inherit insertion order.
        conn.execute_batch(
            r#"
INSERT INTO stock_deduction_gap
 (id,outlet_id,order_id,order_item_id,menu_item_id,menu_item_variant_id,
  menu_item_name,quantity,reason,occurred_at,business_date) VALUES
 ('gap-b','out-1','ord-2','oi-2','mi-2',NULL,'Dal Fry',1,'NO_RECIPE',
  '2026-08-20T11:00:00Z','2026-08-20'),
 ('gap-a','out-1','ord-1','oi-1','mi-1',NULL,'Butter Chicken',2,'NO_RECIPE',
  '2026-08-20T09:00:00Z','2026-08-20'),
 ('gap-c','out-2','ord-3','oi-3','mi-3',NULL,'Paneer Tikka',1,'UNKNOWN_UNIT',
  '2026-08-20T10:00:00Z','2026-08-20');
"#,
        )
        .expect("seed gaps");
    }

    #[test]
    fn gap_entry_seq_backfills_in_sequence_on_a_populated_table() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");
        apply_up_to_but_not(&conn, "0026_gap_entry_seq.sql");
        seed_pre_upgrade_gaps(&conn);

        // The upgrade an existing outlet actually performs.
        apply_all(&conn).expect("0026 must survive a table that already has rows");

        let mut stmt = conn
            .prepare("SELECT id, entry_seq FROM stock_deduction_gap ORDER BY outlet_id, entry_seq")
            .unwrap();
        let rows: Vec<(String, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        // 1-based, per outlet, in OCCURRENCE order — not insertion order, and
        // not one counter shared across outlets.
        assert_eq!(
            rows,
            vec![
                ("gap-a".to_string(), 1),
                ("gap-b".to_string(), 2),
                ("gap-c".to_string(), 1),
            ],
            "backfill must number each outlet's stream 1..N, oldest first"
        );
    }

    #[test]
    fn the_gap_counter_is_seeded_so_the_next_mint_cannot_collide() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");
        apply_up_to_but_not(&conn, "0026_gap_entry_seq.sql");
        seed_pre_upgrade_gaps(&conn);
        apply_all(&conn).expect("apply");

        let seeded: Vec<(String, i64)> = conn
            .prepare(
                "SELECT outlet_id, last_value FROM stock_deduction_gap_sequence ORDER BY outlet_id",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            seeded,
            vec![("out-1".to_string(), 2), ("out-2".to_string(), 1)],
            "the counter must resume where the backfill stopped"
        );

        // The failure this seeding prevents: the first unaccounted sale after
        // an upgrade would collide on UNIQUE (outlet_id, entry_seq) INSIDE
        // confirm_order's transaction, which is the one place ADR-018 Rule 2
        // forbids a failure.
        conn.execute_batch(
            r#"
INSERT INTO stock_deduction_gap
 (id,outlet_id,entry_seq,order_id,order_item_id,menu_item_id,
  menu_item_variant_id,menu_item_name,quantity,reason,occurred_at,business_date)
 VALUES ('gap-d','out-1',3,'ord-9','oi-9','mi-9',NULL,'Naan',1,'NO_RECIPE',
  '2026-08-23T09:00:00Z','2026-08-23');
"#,
        )
        .expect("the next mark after the seeded counter must be free");
    }

    #[test]
    fn replay_cursors_start_at_zero_meaning_nothing_acked() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");
        apply_all(&conn).expect("apply");
        conn.execute_batch(SEED_OUTLETS).expect("seed outlets");
        conn.execute_batch("INSERT INTO sync_state (outlet_id) VALUES ('out-1');")
            .expect("init sync_state");

        let (ledger, gap): (i64, i64) = conn
            .query_row(
                "SELECT last_acked_ledger_entry_seq, last_acked_gap_entry_seq \
                 FROM sync_state WHERE outlet_id = 'out-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("cursors exist");

        // 0 pairs with a 1-BASED entry_seq: `entry_seq > 0` selects the whole
        // stream. Were the sequence 0-based, the first entry of every outlet
        // would be unselectable forever — silently, once, at the only moment
        // nobody is watching.
        assert_eq!((ledger, gap), (0, 0));
    }

    #[test]
    fn both_streams_mint_one_first_and_advance_independently() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("pragmas");
        apply_all(&conn).expect("apply");
        conn.execute_batch(SEED_OUTLETS).expect("seed outlets");

        let bump = |table: &str| -> i64 {
            conn.query_row(
                &format!(
                    "INSERT INTO {table} (outlet_id, last_value, updated_at)
                     VALUES ('out-1', 1, '2026-08-23T00:00:00Z')
                     ON CONFLICT(outlet_id) DO UPDATE SET
                        last_value = {table}.last_value + 1,
                        updated_at = excluded.updated_at
                     RETURNING last_value"
                ),
                [],
                |r| r.get(0),
            )
            .expect("mint")
        };

        assert_eq!(bump("stock_ledger_sequence"), 1, "entry_seq is 1-based");
        assert_eq!(
            bump("stock_deduction_gap_sequence"),
            1,
            "entry_seq is 1-based"
        );
        assert_eq!(bump("stock_ledger_sequence"), 2);
        // Two streams, two counters: advancing the ledger must not move the
        // gap stream's position. One mark cannot mean two positions.
        assert_eq!(bump("stock_deduction_gap_sequence"), 2);
        assert_eq!(bump("stock_ledger_sequence"), 3);
    }
}
