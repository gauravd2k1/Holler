//! Ties routing, the spool and template rendering together: given a KOT,
//! find its printers, queue one job per printer, and (on a sweep) render and
//! send. This is the module `T5`/the POS integration point calls; nothing
//! outside this crate touches `print_job` SQL directly.

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::error::{PrinterError, PrinterResult};
use crate::model::PrintJob;
use crate::template::KotPrintContext;
use crate::transport::build_transport;
use crate::{kot_repo, routing, spool, template};

/// Human-facing context the `kot` row does not itself carry (it only has
/// `order_id`). Supplied by the caller, which already has the order loaded.
/// Owned (not borrowed) so the resolver closure below can be a plain
/// `Fn(&str) -> PrinterResult<KotOrderContext>` without lifetime gymnastics.
#[derive(Debug, Clone)]
pub struct KotOrderContext {
    pub order_display_number: String,
    pub table_label: Option<String>,
}

/// `#132` for the first ticket at a sequence, `#132-A`/`#132-B`/... for
/// every one after (docs/spec/kitchen.md: "KOT #132 → #132-A (addition) →
/// #132-C (cancellation)"). `sequence` is 1-based; 1 has no letter suffix.
pub fn sequence_marker(order_display_number: &str, sequence: i64) -> String {
    if sequence <= 1 {
        format!("#{order_display_number}")
    } else {
        let letter = (b'A' + ((sequence - 2) as u8)) as char;
        format!("#{order_display_number}-{letter}")
    }
}

/// Resolves `kot`'s station to its printer set and enqueues one `print_job`
/// per active printer. Returns the (possibly pre-existing, per
/// [`spool::enqueue_job`]) jobs. Errors with [`PrinterError::NoPrinterRouted`]
/// if the station has no active printer — callers surface that to staff
/// immediately rather than silently losing the ticket to an empty spool.
pub fn queue_kot_for_print(
    conn: &Connection,
    outlet_id: &str,
    kot_id: &str,
    now: &str,
    id_gen: impl Fn() -> String,
) -> PrinterResult<Vec<PrintJob>> {
    let kot = kot_repo::get_kot_by_id(conn, kot_id)?
        .ok_or(PrinterError::NotFound("kot not found for print queueing"))?;

    let printers = routing::resolve_station_printers(conn, outlet_id, &kot.station)?;
    if printers.is_empty() {
        return Err(PrinterError::NoPrinterRouted {
            station_code: kot.station.clone(),
        });
    }

    printers
        .iter()
        .map(|printer| spool::enqueue_job(conn, &id_gen(), &kot.id, &printer.id, now))
        .collect()
}

/// Runs one sweep: renders and sends every due job, updating its status.
/// Returns how many jobs were attempted and how many succeeded, for
/// observability (a caller logs/metrics this; this crate does not itself
/// own a logging framework choice).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SweepReport {
    pub attempted: usize,
    pub printed: usize,
    pub failed: usize,
}

pub fn sweep_due_jobs(
    conn: &Connection,
    now: DateTime<Utc>,
    order_ctx_for_kot: impl Fn(&str) -> PrinterResult<KotOrderContext>,
) -> PrinterResult<SweepReport> {
    let mut report = SweepReport::default();
    let now_str = now.to_rfc3339();

    for job in spool::due_jobs(conn, now)? {
        report.attempted += 1;
        match attempt_print(conn, &job, &now_str, &order_ctx_for_kot) {
            Ok(()) => {
                spool::mark_printed(conn, &job.id, &now_str)?;
                report.printed += 1;
            }
            Err(e) => {
                spool::mark_failed(conn, &job.id, &e.to_string(), &now_str)?;
                report.failed += 1;
            }
        }
    }
    Ok(report)
}

fn attempt_print(
    conn: &Connection,
    job: &PrintJob,
    now_str: &str,
    order_ctx_for_kot: &impl Fn(&str) -> PrinterResult<KotOrderContext>,
) -> PrinterResult<()> {
    let kot = kot_repo::get_kot_by_id(conn, &job.kot_id)?
        .ok_or(PrinterError::NotFound("kot not found for print attempt"))?;
    let printer = get_printer(conn, &job.printer_id)?.ok_or(PrinterError::NotFound(
        "printer not found for print attempt",
    ))?;

    spool::mark_printing(conn, &job.id, now_str)?;

    let order_ctx = order_ctx_for_kot(&kot.order_id)?;
    let marker = sequence_marker(&order_ctx.order_display_number, kot.sequence);
    let ctx = KotPrintContext {
        station_name: &kot.station,
        order_display_number: &order_ctx.order_display_number,
        table_label: order_ctx.table_label.as_deref(),
        sequence_marker: &marker,
    };

    let bytes = template::render_kot(
        &kot.items_json,
        &kot.station,
        &ctx,
        now_str,
        printer.paper_width_mm,
    )?;

    let mut transport = build_transport(&printer);
    transport.send(&bytes)
}

fn get_printer(conn: &Connection, id: &str) -> PrinterResult<Option<crate::model::Printer>> {
    use rusqlite::{params, OptionalExtension};
    conn.query_row(
        "SELECT id, outlet_id, name, connection_kind, address, paper_width_mm, is_active, config_version
         FROM printer WHERE id = ?1",
        params![id],
        |row| {
            let kind_str: String = row.get(3)?;
            let connection_kind = crate::model::ConnectionKind::from_db_str(&kind_str)
                .unwrap_or(crate::model::ConnectionKind::Network);
            Ok(crate::model::Printer {
                id: row.get(0)?,
                outlet_id: row.get(1)?,
                name: row.get(2)?,
                connection_kind,
                address: row.get(4)?,
                paper_width_mm: row.get(5)?,
                is_active: row.get::<_, i64>(6)? != 0,
                config_version: row.get(7)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_marker_first_ticket_has_no_suffix() {
        assert_eq!(sequence_marker("A184", 1), "#A184");
    }

    #[test]
    fn sequence_marker_addition_gets_letter_suffix() {
        assert_eq!(sequence_marker("132", 2), "#132-A");
        assert_eq!(sequence_marker("132", 3), "#132-B");
    }
}
