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
        include_str!(
            "../../../packages/contracts/sqlite/0005_m2_kitchen_stations_printers.sql"
        ),
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

        assert_eq!(
            MIGRATIONS.len(),
            8,
            "expected exactly 8 registered migrations after contracts 0.4.3"
        );
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
            params![id, outlet, order_id, series, number, taxable, cgst, sgst, round_off, grand_total],
        )
    }

    #[test]
    fn a_correct_bill_stores() {
        let conn = seeded();
        // ₹1000 taxable + 2.5% CGST + 2.5% SGST = ₹1050 exactly, no round-off.
        insert_invoice(&conn, "inv-1", "out-1", "ord-1", "ser-1", "FY26/PNQ/000001",
                       100_000, 2_500, 2_500, 0, 105_000)
            .expect("a correctly computed bill must store");
    }

    #[test]
    fn a_correct_bill_with_round_off_stores() {
        let conn = seeded();
        // 99950 + 2499 + 2499 = 104948 → nearest rupee 104900, round_off -48.
        insert_invoice(&conn, "inv-2", "out-1", "ord-1", "ser-1", "FY26/PNQ/000002",
                       99_950, 2_499, 2_499, -48, 104_900)
            .expect("a bill carrying a legitimate round-off must store");
    }

    /// Targets: a tax engine that computes components correctly but writes a
    /// grand total from a separate code path that drifted from them.
    #[test]
    fn grand_total_must_equal_component_sum_plus_round_off() {
        let conn = seeded();
        let err = insert_invoice(&conn, "inv-3", "out-1", "ord-1", "ser-1", "FY26/PNQ/000003",
                                 100_000, 2_500, 2_500, 0, 106_000)
            .expect_err("a grand total that does not equal its parts must be rejected");
        assert!(format!("{err}").contains("CHECK"), "expected CHECK failure, got {err}");
    }

    /// Targets: round-off used to absorb an arithmetic error. Rounding to the
    /// nearest rupee can never move the total by more than half a rupee, so a
    /// larger value means something else is wrong.
    #[test]
    fn round_off_is_bounded_to_half_a_rupee() {
        let conn = seeded();
        let err = insert_invoice(&conn, "inv-4", "out-1", "ord-1", "ser-1", "FY26/PNQ/000004",
                                 99_940, 2_500, 2_500, 60, 105_000)
            .expect_err("round-off beyond ±50 paise must be rejected");
        assert!(format!("{err}").contains("CHECK"), "expected CHECK failure, got {err}");
    }

    /// Targets: the round-off field being present but never applied — the
    /// total still settling in paise, which a cash drawer cannot pay out.
    #[test]
    fn grand_total_must_be_whole_rupees() {
        let conn = seeded();
        let err = insert_invoice(&conn, "inv-5", "out-1", "ord-1", "ser-1", "FY26/PNQ/000005",
                                 99_999, 2_500, 2_500, 0, 104_999)
            .expect_err("a grand total that is not a whole rupee must be rejected");
        assert!(format!("{err}").contains("CHECK"), "expected CHECK failure, got {err}");
    }

    /// §33: "Never generate duplicate invoice numbers."
    #[test]
    fn invoice_number_is_unique_within_an_outlet_series() {
        let conn = seeded();
        insert_invoice(&conn, "inv-6", "out-1", "ord-1", "ser-1", "FY26/PNQ/000001",
                       100_000, 2_500, 2_500, 0, 105_000).expect("first issue");
        let err = insert_invoice(&conn, "inv-7", "out-1", "ord-1", "ser-1", "FY26/PNQ/000001",
                                 100_000, 2_500, 2_500, 0, 105_000)
            .expect_err("a duplicate invoice number must be rejected");
        assert!(format!("{err}").contains("UNIQUE"), "expected UNIQUE failure, got {err}");
    }

    /// The other half of the uniqueness rule, and the one a global unique
    /// index would silently break: two outlets numbering independently is
    /// correct behaviour (CLAUDE.md rubric — uniqueness is tenant-scoped).
    #[test]
    fn the_same_invoice_number_may_exist_at_another_outlet() {
        let conn = seeded();
        insert_invoice(&conn, "inv-8", "out-1", "ord-1", "ser-1", "FY26/PNQ/000001",
                       100_000, 2_500, 2_500, 0, 105_000).expect("outlet 1 issue");
        insert_invoice(&conn, "inv-9", "out-2", "ord-2", "ser-2", "FY26/PNQ/000001",
                       100_000, 2_500, 2_500, 0, 105_000)
            .expect("the same number at a different outlet must be allowed");
    }

    /// Targets: a shift closed by a UI path that forgot to record the count,
    /// leaving a register that can never be reconciled.
    #[test]
    fn a_closed_shift_must_carry_its_counted_cash() {
        let conn = seeded();
        let err = conn.execute(
            r#"INSERT INTO cash_shift (id,outlet_id,device_id,cashier_user_id,status,opened_at,
                opening_cash_paise,business_date,created_at,updated_at)
               VALUES ('cs-bad','out-1','dev-1','usr-1','CLOSED','2026-08-12T08:00:00Z',200000,
                       '2026-08-12','2026-08-12T08:00:00Z','2026-08-12T08:00:00Z')"#,
            [],
        )
        .expect_err("a CLOSED shift without a counted total must be rejected");
        assert!(format!("{err}").contains("CHECK"), "expected CHECK failure, got {err}");
    }

    /// §39 requires a reason for a variance. Targets a drawer that comes up
    /// short and is closed with no explanation recorded.
    #[test]
    fn a_cash_variance_requires_a_reason() {
        let conn = seeded();
        let err = conn.execute(
            r#"INSERT INTO cash_shift (id,outlet_id,device_id,cashier_user_id,status,opened_at,
                opening_cash_paise,closed_at,expected_cash_paise,actual_cash_paise,
                variance_paise,business_date,created_at,updated_at)
               VALUES ('cs-bad2','out-1','dev-1','usr-1','CLOSED','2026-08-12T08:00:00Z',200000,
                       '2026-08-12T23:00:00Z',500000,499000,-1000,'2026-08-12',
                       '2026-08-12T08:00:00Z','2026-08-12T23:00:00Z')"#,
            [],
        )
        .expect_err("a non-zero variance with no reason must be rejected");
        assert!(format!("{err}").contains("CHECK"), "expected CHECK failure, got {err}");
    }

    /// Targets: cash-drawer fields leaking onto a card or UPI tender, which
    /// would corrupt the expected-cash derivation for the whole shift.
    #[test]
    fn tendered_cash_is_only_meaningful_on_a_cash_tender() {
        let conn = seeded();
        let err = conn.execute(
            r#"INSERT INTO payment (id,outlet_id,order_id,cash_shift_id,method,status,
                amount_paise,tendered_paise,created_by_user_id,created_at,updated_at)
               VALUES ('pay-bad','out-1','ord-1','cs-1','UPI','CAPTURED',105000,105000,'usr-1',
                       '2026-08-12T10:30:00Z','2026-08-12T10:30:00Z')"#,
            [],
        )
        .expect_err("tendered_paise on a non-cash tender must be rejected");
        assert!(format!("{err}").contains("CHECK"), "expected CHECK failure, got {err}");
    }

    /// Targets: a half-populated discount row reaching the tax engine, where
    /// "20% or ₹50?" has no defined answer.
    #[test]
    fn a_percent_discount_cannot_also_carry_an_amount() {
        let conn = seeded();
        let err = conn.execute(
            r#"INSERT INTO discount_definition (id,outlet_id,code,name,scope,method,
                value_bps,value_paise,requires_reason,is_active,effective_from,config_version)
               VALUES ('dsc-bad','out-1','STAFF','Staff 20%','BILL','PERCENT',2000,5000,0,1,
                       '2026-04-01T00:00:00Z',1)"#,
            [],
        )
        .expect_err("a PERCENT discount carrying an amount must be rejected");
        assert!(format!("{err}").contains("CHECK"), "expected CHECK failure, got {err}");
    }
}
