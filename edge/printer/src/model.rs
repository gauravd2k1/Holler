//! Typed rows mirroring `packages/contracts/sqlite/0005_m2_kitchen_stations_printers.sql`
//! exactly — no column added or renamed here. This crate's counterpart to
//! `edge/database/src/model.rs`, kept here (rather than added to that
//! crate's own `model.rs`) because these rows are `edge/printer`'s own
//! concern; `edge/database` applies the migration that creates the
//! underlying tables but does not need typed accessors for them.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Station {
    pub id: String,
    pub outlet_id: String,
    pub code: String,
    pub name: String,
    pub sort_order: i64,
    pub is_active: bool,
    pub config_version: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionKind {
    Network,
    Usb,
    Bluetooth,
}

impl ConnectionKind {
    pub fn as_db_str(self) -> &'static str {
        match self {
            ConnectionKind::Network => "ESCPOS_NETWORK",
            ConnectionKind::Usb => "ESCPOS_USB",
            ConnectionKind::Bluetooth => "ESCPOS_BLUETOOTH",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "ESCPOS_NETWORK" => Some(ConnectionKind::Network),
            "ESCPOS_USB" => Some(ConnectionKind::Usb),
            "ESCPOS_BLUETOOTH" => Some(ConnectionKind::Bluetooth),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Printer {
    pub id: String,
    pub outlet_id: String,
    pub name: String,
    pub connection_kind: ConnectionKind,
    pub address: String,
    pub paper_width_mm: i64,
    pub is_active: bool,
    pub config_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StationPrinter {
    pub station_id: String,
    pub printer_id: String,
    pub config_version: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintJobStatus {
    Queued,
    Printing,
    Printed,
    Failed,
}

impl PrintJobStatus {
    pub fn as_db_str(self) -> &'static str {
        match self {
            PrintJobStatus::Queued => "QUEUED",
            PrintJobStatus::Printing => "PRINTING",
            PrintJobStatus::Printed => "PRINTED",
            PrintJobStatus::Failed => "FAILED",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "QUEUED" => Some(PrintJobStatus::Queued),
            "PRINTING" => Some(PrintJobStatus::Printing),
            "PRINTED" => Some(PrintJobStatus::Printed),
            "FAILED" => Some(PrintJobStatus::Failed),
            _ => None,
        }
    }
}

/// `kot_id`/`invoice_id`: exactly one is `Some`, mirroring the CHECK on
/// `print_job` (`0010_print_job_invoice_ref.sql`) — a job prints a KOT or an
/// invoice, never both, never neither. Both `Option` rather than an enum so
/// this struct stays a plain field-for-field mirror of the row (the pattern
/// every other model in this file already follows); [`PrintJob::kind`] is
/// the typed view callers should match on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintJob {
    pub id: String,
    pub kot_id: Option<String>,
    pub invoice_id: Option<String>,
    pub printer_id: String,
    pub status: PrintJobStatus,
    pub attempt_count: i64,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// What a [`PrintJob`] prints, decoded once from its `kot_id`/`invoice_id`
/// pair so callers never have to re-derive the CHECK's meaning themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrintJobTarget<'a> {
    Kot(&'a str),
    Invoice(&'a str),
}

impl PrintJob {
    /// Decodes which document this job prints. Errors if the row violates
    /// the CHECK it should be impossible to violate through this crate's own
    /// write paths — a defensive, not a expected, error.
    pub fn target(&self) -> Result<PrintJobTarget<'_>, &'static str> {
        match (self.kot_id.as_deref(), self.invoice_id.as_deref()) {
            (Some(kot_id), None) => Ok(PrintJobTarget::Kot(kot_id)),
            (None, Some(invoice_id)) => Ok(PrintJobTarget::Invoice(invoice_id)),
            (Some(_), Some(_)) => Err("print_job row has both kot_id and invoice_id set"),
            (None, None) => Err("print_job row has neither kot_id nor invoice_id set"),
        }
    }
}

/// A failed job joined with the printer name and whichever of its two
/// possible parents (`kot`/`invoice`) it actually has, for the staff-visible
/// failure view (`docs/spec/hardware-printing.md`: "Print failures must be
/// visible to staff", §64).
///
/// Exactly one of `kot_station`/`invoice_number` is `Some`, mirroring
/// [`PrintJob::target`] — this type is deliberately not a second, parallel
/// notion of "what kind of job is this". [`FailedPrintJobView::target`]
/// delegates straight to `self.job.target()` so a caller matches on the same
/// enum everywhere rather than re-deriving the CHECK's meaning by testing
/// which display field is populated.
///
/// A KOT job carries its station (a cook needs to know where the ticket was
/// headed). An invoice job carries its invoice number (a cashier needs to
/// know which bill failed) — there is no station for a bill to carry; the
/// frozen `printer` table has no "bill printer" role, and inventing one here
/// would be a contract change this fix does not make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedPrintJobView {
    pub job: PrintJob,
    pub printer_name: String,
    pub kot_station: Option<String>,
    pub invoice_number: Option<String>,
}

impl FailedPrintJobView {
    /// What this failed job prints, decoded the same way [`PrintJob::target`]
    /// is. The one place a caller should ask "is this a KOT or an invoice
    /// job?" — never by checking which display field is `Some`.
    pub fn target(&self) -> Result<PrintJobTarget<'_>, &'static str> {
        self.job.target()
    }
}
