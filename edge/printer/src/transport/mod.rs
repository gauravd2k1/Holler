//! The printer adapter boundary. Hardware specifics live behind
//! [`PrinterTransport`] and never leak into `spool.rs`/`template.rs`
//! (docs/spec/hardware-printing.md "Hardware code must never leak into
//! domain services"). Adding a vendor is an adapter detail; adding a
//! *transport* is a contract change (`printer.connection_kind`), which is
//! why exactly the three frozen kinds are implemented here.

mod file_sink;
mod network;
mod path;

pub use file_sink::{FileSinkTransport, FILE_SINK_DIR_ENV};
pub use network::NetworkTransport;
pub use path::PathTransport;

use crate::error::PrinterResult;
use crate::model::{ConnectionKind, Printer};

/// One physical (or virtual, for tests) printer connection. `send` must
/// deliver the whole byte stream or return an error — partial writes are a
/// transport bug, not a spool concern.
pub trait PrinterTransport {
    fn send(&mut self, bytes: &[u8]) -> PrinterResult<()>;
}

/// Builds the real adapter for a `printer` row's `connection_kind`.
///
/// `ESCPOS_USB` and `ESCPOS_BLUETOOTH` both resolve to [`PathTransport`]:
/// on the Windows 10 baseline (ADR-013) this crate ships on, ESC/POS USB
/// printers and paired Bluetooth SPP printers both surface as a writable
/// device path (a USB-to-serial COM port, or the virtual COM port Windows
/// creates for a paired Bluetooth serial profile) — the *transport* really
/// is "open path, write bytes" in both cases, and treating them as separate
/// byte-stream implementations would duplicate code review has no reason to
/// duplicate. The `connection_kind` distinction stays meaningful at the
/// config layer (what a restaurant owner picks when adding a printer); it
/// collapses only here, at the actual wire.
/// Development/acceptance override, checked before `connection_kind`: when
/// [`FILE_SINK_DIR_ENV`] names a directory, every print is written there as a
/// file instead of being sent to a device (see `file_sink.rs` for why this is
/// an environment setting rather than a fourth `connection_kind`, and for
/// what a file-backed run does and does not establish).
///
/// Unset — every production install — this returns `None` and
/// `build_transport` behaves exactly as it did before the sink existed.
fn file_sink_override(printer: &Printer) -> Option<Box<dyn PrinterTransport>> {
    let dir = std::env::var(FILE_SINK_DIR_ENV).ok()?;
    if dir.trim().is_empty() {
        return None;
    }
    Some(Box::new(FileSinkTransport::new(
        std::path::PathBuf::from(dir),
        printer.id.clone(),
        printer.name.clone(),
    )))
}

pub fn build_transport(printer: &Printer) -> Box<dyn PrinterTransport> {
    if let Some(sink) = file_sink_override(printer) {
        return sink;
    }
    match printer.connection_kind {
        ConnectionKind::Network => Box::new(NetworkTransport::new(printer.address.clone())),
        ConnectionKind::Usb | ConnectionKind::Bluetooth => {
            Box::new(PathTransport::new(printer.address.clone()))
        }
    }
}
