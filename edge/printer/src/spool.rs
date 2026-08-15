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
            "INSERT INTO print_job (id, kot_id, printer_id, status, attempt_count, last_error, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'QUEUED', 0, NULL, ?4, ?4)
             ON CONFLICT (kot_id, printer_id) DO NOTHING",
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

pub fn get_by_id(conn: &Connection, id: &str) -> PrinterResult<Option<PrintJob>> {
    conn.query_row(
        "SELECT id, kot_id, printer_id, status, attempt_count, last_error, created_at, updated_at
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
        "SELECT id, kot_id, printer_id, status, attempt_count, last_error, created_at, updated_at
         FROM print_job WHERE kot_id = ?1 AND printer_id = ?2",
        params![kot_id, printer_id],
        row_to_job,
    )
    .optional()
    .map_err(Into::into)
}

fn row_to_job(row: &rusqlite::Row) -> rusqlite::Result<PrintJob> {
    let status_str: String = row.get(3)?;
    let status = PrintJobStatus::from_db_str(&status_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
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
        printer_id: row.get(2)?,
        status,
        attempt_count: row.get(4)?,
        last_error: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

/// Jobs ready to (re)attempt right now: `QUEUED` (never tried), or `FAILED`
/// with `attempt_count < MAX_ATTEMPTS` whose backoff window has elapsed
/// since `updated_at`. Oldest first.
pub fn due_jobs(conn: &Connection, now: DateTime<Utc>) -> PrinterResult<Vec<PrintJob>> {
    let mut stmt = conn.prepare(
        "SELECT id, kot_id, printer_id, status, attempt_count, last_error, created_at, updated_at
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
pub fn list_failed_jobs(conn: &Connection) -> PrinterResult<Vec<FailedPrintJobView>> {
    let mut stmt = conn.prepare(
        "SELECT pj.id, pj.kot_id, pj.printer_id, pj.status, pj.attempt_count, pj.last_error,
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
            printer_name: row.get(8)?,
            kot_station: row.get(9)?,
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
}
