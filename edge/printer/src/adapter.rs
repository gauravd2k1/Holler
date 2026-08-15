//! Ties routing, the spool and template rendering together: given a KOT,
//! find its printers, queue one job per printer, and (on a sweep) render and
//! send. This is the module `T5`/the POS integration point calls; nothing
//! outside this crate touches `print_job` SQL directly.

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use holler_edge_database::repo as db_repo;

use crate::error::{PrinterError, PrinterResult};
use crate::model::{PrintJob, PrintJobTarget};
use crate::template::{InvoicePrintContext, KotPrintContext};
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

/// Human-facing context the `invoice` row does not itself carry, mirroring
/// [`KotOrderContext`]'s split for the same reason (`invoice` only has
/// `order_id`; `order.display_number`, the table and the tender summary
/// live elsewhere and the caller already has them loaded).
#[derive(Debug, Clone)]
pub struct InvoiceOrderContext {
    pub order_display_number: String,
    pub table_label: Option<String>,
    pub payment_summary: Option<String>,
}

/// Enqueues a `print_job` for one issued invoice at one printer. Idempotent
/// per `(invoice_id, printer_id)` via [`spool::enqueue_invoice_job`] — a
/// second "print bill" tap, or a retried enqueue after a crash, returns the
/// existing job rather than spooling a duplicate copy of the bill.
///
/// **Printer resolution is a contract gap, not a decision made here.** The
/// KOT path resolves a printer from a ticket by `kot.station` ->
/// `station_printer` (`routing::resolve_station_printers`); a bill has no
/// station. The frozen `printer` table (`0005_m2_kitchen_stations_printers.sql`)
/// carries only `name`, `connection_kind`, `address`, `paper_width_mm`,
/// `is_active` — nothing that marks a printer as "the receipt/bill printer"
/// for an outlet, and no default-printer concept on `outlet`, either. Coding
/// a convention here (e.g. matching `printer.name` against a hard-coded
/// string like "Receipt" or "Bill") would be exactly the magic-value
/// violation CLAUDE.md forbids, so this function does not attempt to guess:
/// **the caller supplies `printer_id`** (the operator's own choice, or a
/// future config value once the contract has a field for it), and this
/// function only validates it names a real, active printer in the same
/// outlet as the invoice before enqueuing. The orchestrator should decide
/// whether `printer` gains a `role` column (e.g. `KITCHEN`/`BILLING`) or
/// `outlet` gains a `default_bill_printer_id`; either is additive.
pub fn queue_invoice_for_print(
    conn: &Connection,
    invoice_id: &str,
    printer_id: &str,
    now: &str,
    id_gen: impl Fn() -> String,
) -> PrinterResult<PrintJob> {
    let invoice = db_repo::get_invoice(conn, invoice_id)
        .map_err(PrinterError::Db)?
        .ok_or(PrinterError::NotFound(
            "invoice not found for print queueing",
        ))?;

    let printer = get_printer(conn, printer_id)?.ok_or(PrinterError::NotFound(
        "printer not found for print queueing",
    ))?;
    if !printer.is_active {
        return Err(PrinterError::NoPrinterRouted {
            station_code: format!("invoice printer {printer_id} is not active"),
        });
    }
    if printer.outlet_id != invoice.outlet_id {
        return Err(PrinterError::InvalidInput(format!(
            "printer {printer_id} belongs to outlet {}, not invoice {invoice_id}'s outlet {}",
            printer.outlet_id, invoice.outlet_id
        )));
    }

    spool::enqueue_invoice_job(conn, &id_gen(), &invoice.id, &printer.id, now)
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
    order_ctx_for_invoice: impl Fn(&str) -> PrinterResult<InvoiceOrderContext>,
) -> PrinterResult<SweepReport> {
    let mut report = SweepReport::default();
    let now_str = now.to_rfc3339();

    for job in spool::due_jobs(conn, now)? {
        report.attempted += 1;
        match attempt_print(
            conn,
            &job,
            &now_str,
            &order_ctx_for_kot,
            &order_ctx_for_invoice,
        ) {
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
    order_ctx_for_invoice: &impl Fn(&str) -> PrinterResult<InvoiceOrderContext>,
) -> PrinterResult<()> {
    let printer = get_printer(conn, &job.printer_id)?.ok_or(PrinterError::NotFound(
        "printer not found for print attempt",
    ))?;

    spool::mark_printing(conn, &job.id, now_str)?;

    let bytes = match job
        .target()
        .map_err(|e| PrinterError::InvalidInput(e.to_string()))?
    {
        PrintJobTarget::Kot(kot_id) => {
            render_kot_job(conn, kot_id, now_str, &printer, order_ctx_for_kot)?
        }
        PrintJobTarget::Invoice(invoice_id) => {
            render_invoice_job(conn, invoice_id, &printer, order_ctx_for_invoice)?
        }
    };

    let mut transport = build_transport(&printer);
    transport.send(&bytes)
}

fn render_kot_job(
    conn: &Connection,
    kot_id: &str,
    now_str: &str,
    printer: &crate::model::Printer,
    order_ctx_for_kot: &impl Fn(&str) -> PrinterResult<KotOrderContext>,
) -> PrinterResult<Vec<u8>> {
    let kot = kot_repo::get_kot_by_id(conn, kot_id)?
        .ok_or(PrinterError::NotFound("kot not found for print attempt"))?;

    let order_ctx = order_ctx_for_kot(&kot.order_id)?;
    let marker = sequence_marker(&order_ctx.order_display_number, kot.sequence);
    let ctx = KotPrintContext {
        station_name: &kot.station,
        order_display_number: &order_ctx.order_display_number,
        table_label: order_ctx.table_label.as_deref(),
        sequence_marker: &marker,
    };

    template::render_kot(
        &kot.items_json,
        &kot.station,
        &ctx,
        now_str,
        printer.paper_width_mm,
    )
}

/// Renders the invoice half of [`attempt_print`]. Loads the invoice and its
/// lines fresh from `holler_edge_database::repo` (never cached from queue
/// time) so a reprint after a config change still reflects what was
/// actually issued — `render_invoice` itself only reads columns already
/// snapshotted on the `invoice`/`invoice_line` rows (§31 reproducibility;
/// see `template.rs`'s module doc), never live `outlet_fiscal_profile`.
fn render_invoice_job(
    conn: &Connection,
    invoice_id: &str,
    printer: &crate::model::Printer,
    order_ctx_for_invoice: &impl Fn(&str) -> PrinterResult<InvoiceOrderContext>,
) -> PrinterResult<Vec<u8>> {
    let invoice = db_repo::get_invoice(conn, invoice_id)
        .map_err(PrinterError::Db)?
        .ok_or(PrinterError::NotFound(
            "invoice not found for print attempt",
        ))?;
    let lines = db_repo::list_invoice_lines(conn, invoice_id).map_err(PrinterError::Db)?;

    let order_ctx = order_ctx_for_invoice(&invoice.order_id)?;
    let ctx = InvoicePrintContext {
        order_display_number: template::require_order_display_number(Some(
            order_ctx.order_display_number.as_str(),
        ))?,
        table_label: order_ctx.table_label.as_deref(),
        payment_summary: order_ctx.payment_summary.as_deref(),
    };

    template::render_invoice(&invoice, &lines, &ctx, printer.paper_width_mm)
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
    use holler_edge_database::{crypto::EncryptionKey, Db};

    #[test]
    fn sequence_marker_first_ticket_has_no_suffix() {
        assert_eq!(sequence_marker("A184", 1), "#A184");
    }

    #[test]
    fn sequence_marker_addition_gets_letter_suffix() {
        assert_eq!(sequence_marker("132", 2), "#132-A");
        assert_eq!(sequence_marker("132", 3), "#132-B");
    }

    fn open_test_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().expect("tempdir");
        let sealed = dir.path().join("edge.db.sealed");
        let plaintext = dir.path().join("edge.db");
        let key = EncryptionKey::new([7u8; 32]);
        let db = Db::open(&sealed, &plaintext, key).expect("open db");
        (dir, db)
    }

    /// Seeds a real, migration-runner-built database with everything one
    /// issued invoice needs: outlet, device, app_user, a billed order
    /// carrying a `display_number` (the guard `render_invoice` refuses to
    /// bypass), the menu/order_item/tax_profile chain `invoice_line`'s FKs
    /// require, compliance_version, invoice_series, invoice, invoice_line —
    /// and a printer whose `connection_kind` is `ESCPOS_USB` so
    /// `build_transport` resolves to [`crate::transport::PathTransport`],
    /// which writes the rendered bytes to `receipt_path` where the test can
    /// read them back — proving the renderer actually ran, not merely that
    /// enqueue succeeded.
    fn seed_invoice(conn: &Connection, invoice_id: &str, printer_id: &str, receipt_path: &str) {
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
             VALUES ('{printer_id}', 'outlet-1', 'Front Counter Printer', 'ESCPOS_USB', '{receipt_path}', 80, 1, 1);"
        ))
        .expect("seed");
    }

    /// The end-to-end proof this track exists to deliver: an invoice is
    /// enqueued via [`queue_invoice_for_print`], a sweep renders and
    /// dispatches it through a real (file-backed) transport, and the bytes
    /// that land contain the short order number and never a UUID —
    /// `template::render_invoice`'s `require_order_display_number` guard
    /// actually ran on the dispatch path, not just in `template.rs`'s own
    /// unit tests.
    #[test]
    fn queued_invoice_is_rendered_and_dispatched_with_display_number_not_uuid() {
        let (dir, db) = open_test_db();
        let conn = db.connection();
        let receipt_path = dir.path().join("receipt.bin");
        std::fs::write(&receipt_path, b"").expect("create fake device path");
        let receipt_path_str = receipt_path.to_str().unwrap().replace('\\', "\\\\");
        seed_invoice(conn, "invoice-1", "printer-1", &receipt_path_str);

        let job = queue_invoice_for_print(
            conn,
            "invoice-1",
            "printer-1",
            "2026-08-07T10:05:00Z",
            || "job-1".to_string(),
        )
        .expect("enqueue");
        assert_eq!(job.invoice_id.as_deref(), Some("invoice-1"));
        assert_eq!(job.kot_id, None);

        let now: DateTime<Utc> = "2026-08-07T10:05:01Z".parse().unwrap();
        let report = sweep_due_jobs(
            conn,
            now,
            |_order_id| panic!("no KOT jobs queued; the KOT resolver must not be called"),
            |order_id| {
                assert_eq!(order_id, "order-1");
                Ok(InvoiceOrderContext {
                    order_display_number: "A184".to_string(),
                    table_label: Some("T-04".to_string()),
                    payment_summary: Some("Cash".to_string()),
                })
            },
        )
        .expect("sweep");

        assert_eq!(report.attempted, 1);
        assert_eq!(report.printed, 1, "invoice job must reach PRINTED");
        assert_eq!(report.failed, 0);

        let printed = spool::get_by_id(conn, &job.id).unwrap().unwrap();
        assert_eq!(printed.status, crate::model::PrintJobStatus::Printed);

        let bytes = std::fs::read(&receipt_path).expect("printer wrote a receipt file");
        assert!(!bytes.is_empty(), "render produced no bytes");
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("A184"),
            "rendered receipt must contain the short display number: {text}"
        );
        assert!(
            !text.contains("invoice-1") && !text.contains("order-1"),
            "rendered receipt must never contain an internal id: {text}"
        );
    }

    #[test]
    fn queue_invoice_for_print_rejects_printer_in_a_different_outlet() {
        let (dir, db) = open_test_db();
        let conn = db.connection();
        let receipt_path = dir.path().join("receipt.bin");
        let receipt_path_str = receipt_path.to_str().unwrap().replace('\\', "\\\\");
        seed_invoice(conn, "invoice-1", "printer-1", &receipt_path_str);
        conn.execute_batch(
            "INSERT INTO outlet (id, brand_id, name, timezone, config_version, created_at, updated_at)
             VALUES ('outlet-2', 'brand-1', 'Other Outlet', 'Asia/Kolkata', 1, '2026-08-07T00:00:00Z', '2026-08-07T00:00:00Z');
             INSERT INTO printer (id, outlet_id, name, connection_kind, address, paper_width_mm, is_active, config_version)
             VALUES ('printer-2', 'outlet-2', 'Wrong Outlet Printer', 'ESCPOS_NETWORK', '192.168.1.99:9100', 80, 1, 1);",
        )
        .expect("seed second outlet");

        let err = queue_invoice_for_print(
            conn,
            "invoice-1",
            "printer-2",
            "2026-08-07T10:05:00Z",
            || "job-1".to_string(),
        );
        assert!(
            err.is_err(),
            "must refuse to route a bill to a printer in a different outlet"
        );
    }
}
