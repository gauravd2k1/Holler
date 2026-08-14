//! Invoice issuance, offline numbering and split bills (Milestone 3, track
//! T7b, ADR-016). Builds on T7a's tax engine (`crate::tax`) for every money
//! field — this module never computes tax itself, only assembles an
//! `invoice` + its `invoice_line` rows from an order and persists them
//! atomically alongside the `InvoiceCreated` outbox event (ADR-016 §1:
//! `invoice` is EDGE_TO_CLOUD).
//!
//! Module layout:
//!   - `numbering` — ADR-016 §2: renders a series' `prefix_template`,
//!     derives the `invoice_sequence` reset bucket from `reset_policy`, and
//!     mints the next number atomically with the invoice insert it backs.
//!   - `assemble` — tasks 1/3: turns order lines (or one split's share of
//!     them) into computed `invoice_line` rows via `tax::compute_invoice`,
//!     resolves the §31 reproducibility snapshot, and persists everything.
//!
//! [`crate::Db::issue_invoice_with_outbox`] and
//! [`crate::Db::issue_split_invoices_with_outbox`] (in `src/lib.rs`) are the
//! only entry points that call into this module — like every other
//! operational write in this crate, there is no lower-level API that lets a
//! caller insert an `invoice` without its outbox row or without going
//! through the numbering step.

pub(crate) mod assemble;
pub(crate) mod numbering;
