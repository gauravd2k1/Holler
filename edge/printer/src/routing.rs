//! Station -> printer routing (`station_printer`, ADR-014 §2). A KOT stores
//! its station's stable `code` (not `station_id`), so resolving "where does
//! this ticket print" is: `station` row for `(outlet_id, code)`, then every
//! active `printer` joined through `station_printer` for that station's id.
//! Many-to-many both ways — one station can fan out to several printers, one
//! printer can serve several stations (docs/spec/hardware-printing.md
//! routing example: Tandoor -> Printer A, Bar -> Printer B).

use rusqlite::{params, Connection};

use crate::error::PrinterResult;
use crate::model::{ConnectionKind, Printer, Station};

pub fn get_station_by_code(
    conn: &Connection,
    outlet_id: &str,
    code: &str,
) -> PrinterResult<Option<Station>> {
    let mut stmt = conn.prepare(
        "SELECT id, outlet_id, code, name, sort_order, is_active, config_version
         FROM station WHERE outlet_id = ?1 AND code = ?2",
    )?;
    let mut rows = stmt.query(params![outlet_id, code])?;
    if let Some(row) = rows.next()? {
        Ok(Some(Station {
            id: row.get(0)?,
            outlet_id: row.get(1)?,
            code: row.get(2)?,
            name: row.get(3)?,
            sort_order: row.get(4)?,
            is_active: row.get::<_, i64>(5)? != 0,
            config_version: row.get(6)?,
        }))
    } else {
        Ok(None)
    }
}

/// Every active printer routed to `station_id`, in `printer.name` order for
/// deterministic fan-out (matters for tests and for staff reading a spool
/// list in a stable order).
pub fn printers_for_station(conn: &Connection, station_id: &str) -> PrinterResult<Vec<Printer>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.outlet_id, p.name, p.connection_kind, p.address,
                p.paper_width_mm, p.is_active, p.config_version
         FROM printer p
         JOIN station_printer sp ON sp.printer_id = p.id
         WHERE sp.station_id = ?1 AND p.is_active = 1
         ORDER BY p.name",
    )?;

    // Iterated by hand (not `query_map`) because decoding `connection_kind`
    // can fail with a domain error (`UnsupportedConnectionKind`), and
    // `query_map`'s closure can only return `rusqlite::Error`.
    let mut printers = Vec::new();
    let mut rows = stmt.query(params![station_id])?;
    while let Some(row) = rows.next()? {
        let kind_str: String = row.get(3)?;
        let connection_kind = ConnectionKind::from_db_str(&kind_str).ok_or_else(|| {
            crate::error::PrinterError::UnsupportedConnectionKind(kind_str.clone())
        })?;
        printers.push(Printer {
            id: row.get(0)?,
            outlet_id: row.get(1)?,
            name: row.get(2)?,
            connection_kind,
            address: row.get(4)?,
            paper_width_mm: row.get(5)?,
            is_active: row.get::<_, i64>(6)? != 0,
            config_version: row.get(7)?,
        });
    }
    Ok(printers)
}

/// The printers eligible to print a BILL for `outlet_id` (`printer_role`,
/// contracts 0.4.7), in `printer.name` order for the same determinism
/// [`printers_for_station`] wants.
///
/// This is the answer to the question `queue_invoice_for_print`'s doc
/// comment recorded as unanswerable: a KOT routes station -> station_printer,
/// but a bill has no station, and the frozen `printer` table carried nothing
/// marking a device as the bill printer. Matching on `printer.name` would
/// have been the magic-value violation CLAUDE.md forbids; `printer_role` is
/// the contract answer, so resolution is now a real join rather than a
/// convention.
///
/// Empty, not an error, when the outlet has configured no BILL printer —
/// callers decide whether that is fatal (`adapter::queue_invoice_for_bill_
/// printers` treats it as [`crate::error::PrinterError::NoPrinterRouted`],
/// per 0012's own rule that absence must never be read as "sure, print bills
/// to it").
pub fn resolve_bill_printers(conn: &Connection, outlet_id: &str) -> PrinterResult<Vec<Printer>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.outlet_id, p.name, p.connection_kind, p.address,
                p.paper_width_mm, p.is_active, p.config_version
         FROM printer p
         JOIN printer_role pr ON pr.printer_id = p.id
         WHERE p.outlet_id = ?1 AND pr.role = 'BILL' AND p.is_active = 1
         ORDER BY p.name",
    )?;

    // Hand-iterated for the same reason `printers_for_station` is: decoding
    // `connection_kind` can fail with a domain error `query_map`'s closure
    // cannot return.
    let mut printers = Vec::new();
    let mut rows = stmt.query(params![outlet_id])?;
    while let Some(row) = rows.next()? {
        let kind_str: String = row.get(3)?;
        let connection_kind = ConnectionKind::from_db_str(&kind_str).ok_or_else(|| {
            crate::error::PrinterError::UnsupportedConnectionKind(kind_str.clone())
        })?;
        printers.push(Printer {
            id: row.get(0)?,
            outlet_id: row.get(1)?,
            name: row.get(2)?,
            connection_kind,
            address: row.get(4)?,
            paper_width_mm: row.get(5)?,
            is_active: i64_to_bool(row.get(6)?),
            config_version: row.get(7)?,
        });
    }
    Ok(printers)
}

fn i64_to_bool(v: i64) -> bool {
    v != 0
}

/// Resolves `station_code` (as stored on `kot.station`) to its printer set
/// for `outlet_id`. Empty, not an error, if the station has no printer
/// routed — callers decide whether that is fatal (e.g. `spool::enqueue_for_kot`
/// treats it as [`crate::error::PrinterError::NoPrinterRouted`]).
pub fn resolve_station_printers(
    conn: &Connection,
    outlet_id: &str,
    station_code: &str,
) -> PrinterResult<Vec<Printer>> {
    match get_station_by_code(conn, outlet_id, station_code)? {
        Some(station) if station.is_active => printers_for_station(conn, &station.id),
        _ => Ok(Vec::new()),
    }
}
