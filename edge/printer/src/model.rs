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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintJob {
    pub id: String,
    pub kot_id: String,
    pub printer_id: String,
    pub status: PrintJobStatus,
    pub attempt_count: i64,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A failed job joined with the printer name, for the staff-visible failure
/// view (`docs/spec/hardware-printing.md`: "Print failures must be visible
/// to staff").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedPrintJobView {
    pub job: PrintJob,
    pub printer_name: String,
    pub kot_station: String,
}
