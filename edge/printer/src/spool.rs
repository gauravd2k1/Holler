//! The print spool: `print_job` (edge-local, ADR-014 §3). Queue, retry with
//! backoff, attempt tracking, and the two hard requirements from
//! `docs/spec/hardware-printing.md`:
//!
//! 1. A late printer ack must never cause a duplicate KOT. `UNIQUE (kot_id,
//!    printer_id)` makes a second row unrepresentable; [`enqueue_job`] treats
//!    a conflict as "already spooled" and returns the existing row rather
//!    than resetting it back to `QUEUED` (an `ON CONFLICT ... DO UPDATE`
//!    that reset status would defeat the very guarantee the index exists
//!    for — see the module doc on ADR-014 §3).
//! 2. Print failures must be visible to staff: [`list_failed_jobs`].

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::PrinterResult;
use crate::model::{FailedPrintJobView, PrintJob, PrintJobStatus};

/// A job is retried at most this many times before it is left `FAILED` for
/// staff to see and act on manually (docs/spec/hardware-printing.md: visible,
/// not silently retried forever and losing the ticket).
pub const MAX_ATTEMPTS: i64 = 5;

/// Exponential backoff base. After failed attempt 1, waits 4s; after attempt
/// 2, 8s; and so on, capped below. A thermal printer that is merely out of
/// paper for a minute should not be hammered every retry tick, but a ticket
/// sitting for minutes unretried is also a kitchen problem.
const BACKOFF_BASE_SECS: i64 = 2;
const BACKOFF_MAX_SECS: i64 = 120;

fn backoff_for_attempt(attempt_count: i64) -> chrono::Duration {
    let exponent = attempt_count.clamp(0, 20);
    let secs = BACKOFF_BASE_SECS.saturating_mul(1i64 << exponent);
    chrono::Duration::seconds(secs.min(BACKOFF_MAX_SECS))
}

/// Enqueues one `print_job` per `(kot_id, printer_id)`. Idempotent: if a job
/// for this pair already exists (any status, including a late-arriving
/// `PRINTED`), returns the existing row unchanged rather than creating a
/// second one or resetting it to `QUEUED`. This is what makes "a late
/// printer ack must never cause a duplicate KOT" true regardless of caller
/// retries.
pub fn enqueue_job(
    conn: &Connection,
    id: &str,
    kot_id: &str,
    printer_id: &str,
    now: &str,
) -> PrinterResult<PrintJob> {
    let inserted = conn
        .execute(
            "INSERT INTO print_job (id, kot_id, invoice_id, printer_id, status, attempt_count, last_error, created_at, updated_at)
             VALUES (?1, ?2, NULL, ?3, 'QUEUED', 0, NULL, ?4, ?4)
             ON CONFLICT (kot_id, printer_id) WHERE kot_id IS NOT NULL DO NOTHING",
            params![id, kot_id, printer_id, now],
        )?;

    if inserted == 0 {
        // Row already existed for this (kot_id, printer_id) — fetch and
        // return it as-is. Deliberately no UPDATE here: see module doc.
        return get_by_kot_and_printer(conn, kot_id, printer_id)?.ok_or_else(|| {
            crate::error::PrinterError::NotFound("print_job vanished after DO NOTHING conflict")
        });
    }

    get_by_id(conn, id)?
        .ok_or_else(|| crate::error::PrinterError::NotFound("print_job not found after insert"))
}

/// Invoice twin of [`enqueue_job`]: one `print_job` per `(invoice_id,
/// printer_id)`, `kot_id` left `NULL` (the CHECK on `print_job` requires
/// exactly one of the pair, and this is the invoice half). Idempotent for
/// the same reason: a double-tap "print bill" or a retried enqueue after a
/// crash must not queue a second copy of the same bill at the same printer.
/// The `ON CONFLICT` target must name the partial index's predicate exactly
/// (`WHERE invoice_id IS NOT NULL`, matching `idx_print_job_invoice_printer`
/// in `0010_print_job_invoice_ref.sql`) — an unqualified target fails at
/// runtime with "ON CONFLICT clause does not match any PRIMARY KEY or UNIQUE
/// constraint" (the bug fixed at `c65600b` for the KOT index; the same class
/// of mistake here would be silent until the first real double-enqueue).
pub fn enqueue_invoice_job(
    conn: &Connection,
    id: &str,
    invoice_id: &str,
    printer_id: &str,
    now: &str,
) -> PrinterResult<PrintJob> {
    let inserted = conn
        .execute(
            "INSERT INTO print_job (id, kot_id, invoice_id, printer_id, status, attempt_count, last_error, created_at, updated_at)
             VALUES (?1, NULL, ?2, ?3, 'QUEUED', 0, NULL, ?4, ?4)
             ON CONFLICT (invoice_id, printer_id) WHERE invoice_id IS NOT NULL DO NOTHING",
            params![id, invoice_id, printer_id, now],
        )?;

    if inserted == 0 {
        return get_by_invoice_and_printer(conn, invoice_id, printer_id)?.ok_or_else(|| {
            crate::error::PrinterError::NotFound("print_job vanished after DO NOTHING conflict")
        });
    }

    get_by_id(conn, id)?
        .ok_or_else(|| crate::error::PrinterError::NotFound("print_job not found after insert"))
}

pub fn get_by_id(conn: &Connection, id: &str) -> PrinterResult<Option<PrintJob>> {
    conn.query_row(
        "SELECT id, kot_id, invoice_id, printer_id, status, attempt_count, last_error, created_at, updated_at
         FROM print_job WHERE id = ?1",
        params![id],
        row_to_job,
    )
    .optional()
    .map_err(Into::into)
}

pub fn get_by_kot_and_printer(
    conn: &Connection,
    kot_id: &str,
    printer_id: &str,
) -> PrinterResult<Option<PrintJob>> {
    conn.query_row(
        "SELECT id, kot_id, invoice_id, printer_id, status, attempt_count, last_error, created_at, updated_at
         FROM print_job WHERE kot_id = ?1 AND printer_id = ?2",
        params![kot_id, printer_id],
        row_to_job,
    )
    .optional()
    .map_err(Into::into)
}

pub fn get_by_invoice_and_printer(
    conn: &Connection,
    invoice_id: &str,
    printer_id: &str,
) -> PrinterResult<Option<PrintJob>> {
    conn.query_row(
        "SELECT id, kot_id, invoice_id, printer_id, status, attempt_count, last_error, created_at, updated_at
         FROM print_job WHERE invoice_id = ?1 AND printer_id = ?2",
        params![invoice_id, printer_id],
        row_to_job,
    )
    .optional()
    .map_err(Into::into)
}

fn row_to_job(row: &rusqlite::Row) -> rusqlite::Result<PrintJob> {
    let status_str: String = row.get(4)?;
    let status = PrintJobStatus::from_db_str(&status_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown print_job status {status_str}"),
            )),
        )
    })?;
    Ok(PrintJob {
        id: row.get(0)?,
        kot_id: row.get(1)?,
        invoice_id: row.get(2)?,
        printer_id: row.get(3)?,
        status,
        attempt_count: row.get(5)?,
        last_error: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

/// Jobs ready to (re)attempt right now: `QUEUED` (never tried), or `FAILED`
/// with `attempt_count < MAX_ATTEMPTS` whose backoff window has elapsed
/// since `updated_at`. Oldest first.
pub fn due_jobs(conn: &Connection, now: DateTime<Utc>) -> PrinterResult<Vec<PrintJob>> {
    let mut stmt = conn.prepare(
        "SELECT id, kot_id, invoice_id, printer_id, status, attempt_count, last_error, created_at, updated_at
         FROM print_job
         WHERE status IN ('QUEUED', 'FAILED')
         ORDER BY created_at",
    )?;
    let mut rows = stmt.query([])?;
    let mut due = Vec::new();
    while let Some(row) = rows.next()? {
        let job = row_to_job(row)?;
        let ready = match job.status {
            PrintJobStatus::Queued => true,
            PrintJobStatus::Failed => {
                if job.attempt_count >= MAX_ATTEMPTS {
                    false
                } else {
                    let updated: DateTime<Utc> = job.updated_at.parse().map_err(|_| {
                        crate::error::PrinterError::InvalidInput(format!(
                            "malformed print_job.updated_at: {}",
                            job.updated_at
                        ))
                    })?;
                    now >= updated + backoff_for_attempt(job.attempt_count)
                }
            }
            _ => false,
        };
        if ready {
            due.push(job);
        }
    }
    Ok(due)
}

pub fn mark_printing(conn: &Connection, job_id: &str, now: &str) -> PrinterResult<()> {
    conn.execute(
        "UPDATE print_job SET status = 'PRINTING', updated_at = ?2 WHERE id = ?1",
        params![job_id, now],
    )?;
    Ok(())
}

/// Records a successful print. Idempotent against a late ack: if the job is
/// already `PRINTED` this is a no-op (no attempt_count bump), which is the
/// other half of "a late printer ack must never cause a duplicate KOT" — an
/// ack arriving after the spool already moved on must not re-fire anything.
pub fn mark_printed(conn: &Connection, job_id: &str, now: &str) -> PrinterResult<()> {
    conn.execute(
        "UPDATE print_job
         SET status = 'PRINTED', attempt_count = attempt_count + 1, last_error = NULL, updated_at = ?2
         WHERE id = ?1 AND status != 'PRINTED'",
        params![job_id, now],
    )?;
    Ok(())
}

pub fn mark_failed(conn: &Connection, job_id: &str, error: &str, now: &str) -> PrinterResult<()> {
    conn.execute(
        "UPDATE print_job
         SET status = 'FAILED', attempt_count = attempt_count + 1, last_error = ?3, updated_at = ?2
         WHERE id = ?1",
        params![job_id, now, error],
    )?;
    Ok(())
}

/// Failed jobs, for the staff-visible failure view T5 renders in the POS
/// (docs/spec/hardware-printing.md "Print failures must be visible to
/// staff"). Joined with the printer name and the kot's station so the view
/// does not need a second round trip per row.
///
/// **KOT jobs only** — the `JOIN kot` is deliberately inner, so an invoice
/// job (whose `kot_id` is `NULL`) is excluded rather than joined away to
/// nothing. Surfacing failed invoice print jobs to staff is real scope this
/// track does not close: [`FailedPrintJobView`] has no invoice-shaped
/// equivalent yet, and adding one means deciding what "station" means for a
/// bill (see `queue_invoice_for_print`'s doc comment on the same open
/// question). Left as a reported gap, not silently dropped.
pub fn list_failed_jobs(conn: &Connection) -> PrinterResult<Vec<FailedPrintJobView>> {
    let mut stmt = conn.prepare(
        "SELECT pj.id, pj.kot_id, pj.invoice_id, pj.printer_id, pj.status, pj.attempt_count, pj.last_error,
                pj.created_at, pj.updated_at, p.name, k.station
         FROM print_job pj
         JOIN printer p ON p.id = pj.printer_id
         JOIN kot k ON k.id = pj.kot_id
         WHERE pj.status = 'FAILED'
         ORDER BY pj.updated_at DESC",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let job = row_to_job(row)?;
        out.push(FailedPrintJobView {
            job,
            printer_name: row.get(9)?,
            kot_station: row.get(10)?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use holler_edge_database::{crypto::EncryptionKey, Db};

    /// `Db::open` alone now applies every migration including
    /// `0005_m2_kitchen_stations_printers.sql` (`edge/database`'s own
    /// migration list, not a bootstrap owned by this crate) — a future
    /// regression there fails these tests loudly rather than being masked.
    fn open_test_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().expect("tempdir");
        let sealed = dir.path().join("edge.db.sealed");
        let plaintext = dir.path().join("edge.db");
        let key = EncryptionKey::new([9u8; 32]);
        let db = Db::open(&sealed, &plaintext, key).expect("open db");
        (dir, db)
    }

    /// Seeds the minimal parent rows a `print_job` foreign-keys against:
    /// outlet, device, order, kot, printer.
    fn seed_kot_and_printer(conn: &Connection, kot_id: &str, printer_id: &str) {
        conn.execute_batch(&format!(
            "INSERT INTO outlet (id, brand_id, name, timezone, config_version, created_at, updated_at)
             VALUES ('outlet-1', 'brand-1', 'Test Outlet', 'Asia/Kolkata', 1, '2026-08-07T00:00:00Z', '2026-08-07T00:00:00Z');
             INSERT INTO device (id, outlet_id, kind, name, created_at)
             VALUES ('device-1', 'outlet-1', 'POS', 'Till 1', '2026-08-07T00:00:00Z');
             INSERT INTO \"order\" (id, outlet_id, device_id, order_type, status, subtotal_paise, discount_paise, taxes_paise, total_paise, source, payment_status, schema_version, version, sync_status, created_at, updated_at)
             VALUES ('order-1', 'outlet-1', 'device-1', 'DINE_IN', 'DRAFT', 0, 0, 0, 0, 'POS', 'UNPAID', 1, 1, 'PENDING', '2026-08-07T00:00:00Z', '2026-08-07T00:00:00Z');
             INSERT INTO kot (id, order_id, station, sequence, status, items_json, created_by_device_id, created_at, updated_at)
             VALUES ('{kot_id}', 'order-1', 'MAIN_KITCHEN', 1, 'NEW', '[]', 'device-1', '2026-08-07T00:00:00Z', '2026-08-07T00:00:00Z');
             INSERT INTO printer (id, outlet_id, name, connection_kind, address, paper_width_mm, is_active, config_version)
             VALUES ('{printer_id}', 'outlet-1', 'Tandoor Printer', 'ESCPOS_NETWORK', '192.168.1.50:9100', 80, 1, 1);"
        ))
        .expect("seed");
    }

    /// Seeds the minimal parent rows an invoice `print_job` foreign-keys
    /// against: outlet, device, app_user, order (with a display_number, the
    /// M2 UUID-on-ticket guard's precondition), compliance_version,
    /// invoice_series, invoice, invoice_line, printer.
    fn seed_invoice_and_printer(conn: &Connection, invoice_id: &str, printer_id: &str) {
        conn.execute_batch(&format!(
            "INSERT INTO outlet (id, brand_id, name, timezone, config_version, created_at, updated_at)
             VALUES ('outlet-1', 'brand-1', 'Test Outlet', 'Asia/Kolkata', 1, '2026-08-07T00:00:00Z', '2026-08-07T00:00:00Z');
             INSERT INTO device (id, outlet_id, kind, name, created_at)
             VALUES ('device-1', 'outlet-1', 'POS', 'Till 1', '2026-08-07T00:00:00Z');
             INSERT INTO app_user (id, tenant_id, outlet_id, email, full_name, password_hash, is_active, permissions_json, config_version, updated_at)
             VALUES ('user-1', 'tenant-1', 'outlet-1', 'asha@example.in', 'Asha', 'argon2id$dummy', 1, '[]', 1, '2026-08-07T00:00:00Z');
             INSERT INTO \"order\" (id, outlet_id, device_id, order_type, status, display_number, subtotal_paise, discount_paise, taxes_paise, total_paise, source, payment_status, schema_version, version, sync_status, created_at, updated_at)
             VALUES ('order-1', 'outlet-1', 'device-1', 'DINE_IN', 'BILLED', 'A184', 50000, 0, 2500, 52500, 'POS', 'PAID', 1, 1, 'PENDING', '2026-08-07T00:00:00Z', '2026-08-07T00:00:00Z');
             INSERT INTO menu_category (id, outlet_id, name, sort_order, config_version)
             VALUES ('category-1', 'outlet-1', 'Mains', 0, 1);
             INSERT INTO menu_item (id, outlet_id, category_id, name, base_price_paise, is_available, config_version)
             VALUES ('menuitem-1', 'outlet-1', 'category-1', 'Butter Chicken', 25000, 1, 1);
             INSERT INTO order_item (id, order_id, menu_item_id, quantity, unit_price_paise, line_total_paise, created_at)
             VALUES ('item-1', 'order-1', 'menuitem-1', 2, 25000, 50000, '2026-08-07T09:55:00Z');
             INSERT INTO tax_profile (id, outlet_id, code, name, pricing_mode, is_default, is_active, config_version)
             VALUES ('taxprofile-1', 'outlet-1', 'GST_5_RESTAURANT', 'GST 5% Restaurant', 'EXCLUSIVE', 1, 1, 1);
             INSERT INTO compliance_version (id, outlet_id, label, effective_from, config_version)
             VALUES ('cv-1', 'outlet-1', 'GST 2026-04', '2026-04-01T00:00:00Z', 1);
             INSERT INTO invoice_series (id, outlet_id, code, prefix_template, reset_policy, padding_width, is_active, config_version)
             VALUES ('series-1', 'outlet-1', 'SALES', 'FY{{FY}}/{{OUTLET}}/', 'FY', 6, 1, 1);
             INSERT INTO invoice
                (id, outlet_id, order_id, series_id, invoice_number, invoice_date, business_date, status,
                 place_of_supply_state_code, subtotal_paise, taxable_value_paise, cgst_paise, sgst_paise,
                 grand_total_paise, compliance_version_id, tax_snapshot_json, fiscal_profile_json,
                 channel, tax_liability_party, created_by_user_id, created_at, updated_at)
             VALUES
                ('{invoice_id}', 'outlet-1', 'order-1', 'series-1', 'FY26/PNQ/000001', '2026-08-07T10:00:00Z', '2026-08-07',
                 'ISSUED', '27', 50000, 50000, 1250, 1250, 52500, 'cv-1', '{{}}',
                 '{{\"legal_name\":\"Holler Hospitality Pvt Ltd\",\"address_line1\":\"12 MG Road\",\"city\":\"Pune\",\"state_code\":\"27\",\"pincode\":\"411001\",\"gstin\":\"27ABCDE1234F1Z5\"}}',
                 'POS', 'RESTAURANT', 'user-1', '2026-08-07T10:00:00Z', '2026-08-07T10:00:00Z');
             INSERT INTO invoice_line
                (id, invoice_id, order_item_id, line_no, description, hsn_sac, quantity, unit_price_paise,
                 gross_paise, discount_paise, taxable_value_paise, tax_profile_id, cgst_rate_bps, cgst_paise,
                 sgst_rate_bps, sgst_paise, igst_rate_bps, igst_paise, cess_rate_bps, cess_paise, total_paise)
             VALUES
                ('line-1', '{invoice_id}', 'item-1', 1, 'Butter Chicken', '996331', 2, 25000, 50000, 0, 50000,
                 'taxprofile-1', 250, 1250, 250, 1250, 0, 0, 0, 0, 52500);
             INSERT INTO printer (id, outlet_id, name, connection_kind, address, paper_width_mm, is_active, config_version)
             VALUES ('{printer_id}', 'outlet-1', 'Front Counter Printer', 'ESCPOS_NETWORK', '192.168.1.60:9100', 80, 1, 1);"
        ))
        .expect("seed");
    }

    #[test]
    fn enqueue_creates_a_queued_job() {
        let (_dir, db) = open_test_db();
        seed_kot_and_printer(db.connection(), "kot-1", "printer-1");
        let job = enqueue_job(
            db.connection(),
            "job-1",
            "kot-1",
            "printer-1",
            "2026-08-07T10:00:00Z",
        )
        .expect("enqueue");
        assert_eq!(job.status, PrintJobStatus::Queued);
        assert_eq!(job.attempt_count, 0);
    }

    #[test]
    fn late_ack_does_not_duplicate_kot() {
        // The core guarantee: enqueue, print to completion, then simulate a
        // late ack / retry attempting to enqueue the same (kot, printer)
        // pair again. Must stay one row, must stay PRINTED.
        let (_dir, db) = open_test_db();
        let conn = db.connection();
        seed_kot_and_printer(conn, "kot-1", "printer-1");

        let job = enqueue_job(conn, "job-1", "kot-1", "printer-1", "2026-08-07T10:00:00Z").unwrap();
        mark_printing(conn, &job.id, "2026-08-07T10:00:01Z").unwrap();
        mark_printed(conn, &job.id, "2026-08-07T10:00:02Z").unwrap();

        // A retry path (or a duplicate send-to-kitchen action) tries to
        // enqueue the same pair again with a *different* id, simulating a
        // caller that did not know a job already existed.
        let again =
            enqueue_job(conn, "job-2", "kot-1", "printer-1", "2026-08-07T10:05:00Z").unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM print_job", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "a late enqueue must not create a second row");
        assert_eq!(
            again.id, "job-1",
            "must return the original job, not spawn job-2"
        );
        assert_eq!(again.status, PrintJobStatus::Printed);

        // And a late printer ack for the *original* job after it is already
        // PRINTED must not bump attempt_count or flip status again.
        mark_printed(conn, &job.id, "2026-08-07T10:06:00Z").unwrap();
        let after_late_ack = get_by_id(conn, &job.id).unwrap().unwrap();
        assert_eq!(after_late_ack.status, PrintJobStatus::Printed);
        assert_eq!(
            after_late_ack.attempt_count, 1,
            "late ack must not re-bump attempt_count"
        );
    }

    #[test]
    fn mark_failed_records_error_and_increments_attempts() {
        let (_dir, db) = open_test_db();
        let conn = db.connection();
        seed_kot_and_printer(conn, "kot-1", "printer-1");
        let job = enqueue_job(conn, "job-1", "kot-1", "printer-1", "2026-08-07T10:00:00Z").unwrap();

        mark_failed(conn, &job.id, "connect refused", "2026-08-07T10:00:05Z").unwrap();

        let after = get_by_id(conn, &job.id).unwrap().unwrap();
        assert_eq!(after.status, PrintJobStatus::Failed);
        assert_eq!(after.attempt_count, 1);
        assert_eq!(after.last_error.as_deref(), Some("connect refused"));
    }

    #[test]
    fn due_jobs_respects_backoff_window() {
        let (_dir, db) = open_test_db();
        let conn = db.connection();
        seed_kot_and_printer(conn, "kot-1", "printer-1");
        let job = enqueue_job(conn, "job-1", "kot-1", "printer-1", "2026-08-07T10:00:00Z").unwrap();
        mark_failed(conn, &job.id, "timeout", "2026-08-07T10:00:00Z").unwrap();

        // Immediately after failing (attempt_count=1 -> backoff 4s), the job
        // is not yet due.
        let just_after: DateTime<Utc> = "2026-08-07T10:00:01Z".parse().unwrap();
        assert!(due_jobs(conn, just_after).unwrap().is_empty());

        // After the backoff window elapses, it is due again.
        let later: DateTime<Utc> = "2026-08-07T10:00:10Z".parse().unwrap();
        let due = due_jobs(conn, later).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, "job-1");
    }

    #[test]
    fn jobs_stop_retrying_after_max_attempts() {
        let (_dir, db) = open_test_db();
        let conn = db.connection();
        seed_kot_and_printer(conn, "kot-1", "printer-1");
        let job = enqueue_job(conn, "job-1", "kot-1", "printer-1", "2026-08-07T10:00:00Z").unwrap();

        for i in 0..MAX_ATTEMPTS {
            mark_failed(
                conn,
                &job.id,
                "still down",
                &format!("2026-08-07T10:0{i}:00Z"),
            )
            .unwrap();
        }

        let far_future: DateTime<Utc> = "2026-08-08T10:00:00Z".parse().unwrap();
        assert!(
            due_jobs(conn, far_future).unwrap().is_empty(),
            "a job at MAX_ATTEMPTS must not be retried further"
        );
    }

    #[test]
    fn double_enqueue_of_same_kot_and_printer_is_idempotent() {
        // Regression for the partial-index ON CONFLICT mismatch: contracts
        // 0.4.5 (0010_print_job_invoice_ref.sql) made idx_print_job_kot_printer
        // a partial unique index (`WHERE kot_id IS NOT NULL`), and the
        // enqueue's ON CONFLICT target has to name that same predicate or
        // SQLite rejects the statement outright rather than silently
        // ignoring it. This runs against a database migrated by the real
        // migration runner (open_test_db -> Db::open), not a hand-built
        // schema, because the bug only reproduces against the true partial
        // index.
        let (_dir, db) = open_test_db();
        let conn = db.connection();
        seed_kot_and_printer(conn, "kot-1", "printer-1");

        let first =
            enqueue_job(conn, "job-1", "kot-1", "printer-1", "2026-08-07T10:00:00Z").unwrap();
        let second =
            enqueue_job(conn, "job-2", "kot-1", "printer-1", "2026-08-07T10:00:05Z").unwrap();

        assert_eq!(
            first.id, second.id,
            "second enqueue must return the same job, not a new one"
        );

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM print_job", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 1,
            "double enqueue must not create a duplicate print_job row"
        );
    }

    #[test]
    fn list_failed_jobs_is_visible_for_staff() {
        let (_dir, db) = open_test_db();
        let conn = db.connection();
        seed_kot_and_printer(conn, "kot-1", "printer-1");
        let job = enqueue_job(conn, "job-1", "kot-1", "printer-1", "2026-08-07T10:00:00Z").unwrap();
        mark_failed(conn, &job.id, "out of paper", "2026-08-07T10:00:05Z").unwrap();

        let failed = list_failed_jobs(conn).unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].printer_name, "Tandoor Printer");
        assert_eq!(failed[0].kot_station, "MAIN_KITCHEN");
        assert_eq!(failed[0].job.last_error.as_deref(), Some("out of paper"));
    }

    #[test]
    fn double_enqueue_of_same_invoice_and_printer_is_idempotent() {
        // The invoice twin of `double_enqueue_of_same_kot_and_printer_is_idempotent`:
        // proves `enqueue_invoice_job`'s `ON CONFLICT (invoice_id, printer_id)
        // WHERE invoice_id IS NOT NULL` target actually matches
        // `idx_print_job_invoice_printer` in the real, migration-runner-built
        // schema. Falsified by temporarily dropping the `WHERE invoice_id IS
        // NOT NULL` predicate from this function's ON CONFLICT clause: SQLite
        // then rejects the INSERT outright with "ON CONFLICT clause does not
        // match any PRIMARY KEY or UNIQUE constraint" and this test fails to
        // compile-run at all (the statement errors on the first enqueue, not
        // just the second) — confirmed by hand, then reverted to the
        // qualified form below, which passes.
        let (_dir, db) = open_test_db();
        let conn = db.connection();
        seed_invoice_and_printer(conn, "invoice-1", "printer-1");

        let first = enqueue_invoice_job(
            conn,
            "job-1",
            "invoice-1",
            "printer-1",
            "2026-08-07T10:00:00Z",
        )
        .unwrap();
        let second = enqueue_invoice_job(
            conn,
            "job-2",
            "invoice-1",
            "printer-1",
            "2026-08-07T10:00:05Z",
        )
        .unwrap();

        assert_eq!(
            first.id, second.id,
            "second enqueue must return the same job, not a new one"
        );
        assert_eq!(first.invoice_id.as_deref(), Some("invoice-1"));
        assert_eq!(first.kot_id, None);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM print_job", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 1,
            "double enqueue must not create a duplicate print_job row (one job, no duplicate bill)"
        );
    }

    /// Adds only the invoice-specific rows (app_user, compliance_version,
    /// invoice_series, invoice, invoice_line) on top of an outlet/device/
    /// order already seeded by [`seed_kot_and_printer`] — for tests that
    /// need a valid kot_id AND a valid invoice_id in the same database
    /// without re-inserting outlet-1/device-1/order-1 and hitting a
    /// duplicate primary key.
    fn seed_invoice_only_on_existing_order(conn: &Connection, invoice_id: &str) {
        conn.execute_batch(&format!(
            "INSERT INTO app_user (id, tenant_id, outlet_id, email, full_name, password_hash, is_active, permissions_json, config_version, updated_at)
             VALUES ('user-1', 'tenant-1', 'outlet-1', 'asha@example.in', 'Asha', 'argon2id$dummy', 1, '[]', 1, '2026-08-07T00:00:00Z');
             INSERT INTO compliance_version (id, outlet_id, label, effective_from, config_version)
             VALUES ('cv-1', 'outlet-1', 'GST 2026-04', '2026-04-01T00:00:00Z', 1);
             INSERT INTO invoice_series (id, outlet_id, code, prefix_template, reset_policy, padding_width, is_active, config_version)
             VALUES ('series-1', 'outlet-1', 'SALES', 'FY{{FY}}/{{OUTLET}}/', 'FY', 6, 1, 1);
             INSERT INTO invoice
                (id, outlet_id, order_id, series_id, invoice_number, invoice_date, business_date, status,
                 place_of_supply_state_code, subtotal_paise, taxable_value_paise, cgst_paise, sgst_paise,
                 grand_total_paise, compliance_version_id, tax_snapshot_json, fiscal_profile_json,
                 channel, tax_liability_party, created_by_user_id, created_at, updated_at)
             VALUES
                ('{invoice_id}', 'outlet-1', 'order-1', 'series-1', 'FY26/PNQ/000001', '2026-08-07T10:00:00Z', '2026-08-07',
                 'ISSUED', '27', 0, 0, 0, 0, 0, 'cv-1', '{{}}', '{{}}',
                 'POS', 'RESTAURANT', 'user-1', '2026-08-07T10:00:00Z', '2026-08-07T10:00:00Z');"
        ))
        .expect("seed");
    }

    #[test]
    fn print_job_check_rejects_both_kot_and_invoice_set() {
        // The CHECK on `print_job` (0010_print_job_invoice_ref.sql) must hold
        // through raw SQL too, not just through this crate's own enqueue
        // functions — proving the constraint itself, not just that our code
        // happens to respect it. Falsified: commenting out the CHECK clause
        // in the contract migration makes this INSERT succeed and the test
        // fail; restoring the CHECK (the shipped state) makes it error as
        // asserted below. Verified by hand against the migration file, not
        // left in the tree as a mutation.
        let (_dir, db) = open_test_db();
        let conn = db.connection();
        seed_kot_and_printer(conn, "kot-1", "printer-1");
        seed_invoice_only_on_existing_order(conn, "invoice-1");

        let err = conn.execute(
            "INSERT INTO print_job (id, kot_id, invoice_id, printer_id, status, attempt_count, last_error, created_at, updated_at)
             VALUES ('bad-job', 'kot-1', 'invoice-1', 'printer-1', 'QUEUED', 0, NULL, '2026-08-07T10:00:00Z', '2026-08-07T10:00:00Z')",
            [],
        );
        assert!(
            err.is_err(),
            "a print_job with both kot_id and invoice_id set must be rejected by the CHECK"
        );
    }

    #[test]
    fn print_job_check_rejects_neither_kot_nor_invoice_set() {
        let (_dir, db) = open_test_db();
        let conn = db.connection();
        seed_kot_and_printer(conn, "kot-1", "printer-1");

        let err = conn.execute(
            "INSERT INTO print_job (id, kot_id, invoice_id, printer_id, status, attempt_count, last_error, created_at, updated_at)
             VALUES ('bad-job', NULL, NULL, 'printer-1', 'QUEUED', 0, NULL, '2026-08-07T10:00:00Z', '2026-08-07T10:00:00Z')",
            [],
        );
        assert!(
            err.is_err(),
            "a print_job with neither kot_id nor invoice_id set must be rejected by the CHECK"
        );
    }

    #[test]
    fn enqueue_invoice_job_leaves_kot_id_null() {
        let (_dir, db) = open_test_db();
        let conn = db.connection();
        seed_invoice_and_printer(conn, "invoice-1", "printer-1");

        let job = enqueue_invoice_job(
            conn,
            "job-1",
            "invoice-1",
            "printer-1",
            "2026-08-07T10:00:00Z",
        )
        .unwrap();

        assert_eq!(job.kot_id, None);
        assert_eq!(job.invoice_id.as_deref(), Some("invoice-1"));
        assert_eq!(job.status, PrintJobStatus::Queued);
        assert_eq!(
            job.target().unwrap(),
            crate::model::PrintJobTarget::Invoice("invoice-1")
        );
    }
}
