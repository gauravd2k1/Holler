//! Edge printer service (ADR-014, docs/spec/hardware-printing.md): the ESC/POS
//! adapter boundary, KOT template rendering, the print spool, and station ->
//! printer routing.
//!
//! Hardware specifics live behind [`transport::PrinterTransport`] and never
//! leak past it — `spool` and `adapter` know only that a transport can
//! `send` bytes, not how (docs/spec/hardware-printing.md "Hardware code
//! must never leak into domain services").
//!
//! `print_job` is edge-local and deliberately not an `AggregateType`
//! (ADR-014 §3) — nothing in this crate ever pushes it to the sync outbox.

pub mod adapter;
pub mod error;
pub mod escpos;
mod kot_repo;
pub mod model;
pub mod routing;
pub mod spool;
pub mod template;
pub mod transport;

pub use error::{PrinterError, PrinterResult};
