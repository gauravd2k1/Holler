//! Payments and cash shift at the edge (Milestone 3, track T7c, ADR-016).
//!
//! Two aggregates, two different write disciplines, both driven by
//! `docs/spec/payments.md`:
//!
//!   - `payment` (§34, §Conflict policy) is APPEND-ONLY. A tender, once
//!     captured, is never updated or deleted — a void or refund is a NEW
//!     row carrying `reverses_payment_id`, with a non-positive amount.
//!     [`tender::record_payment`] is the only writer, and it enforces the
//!     amount-sign/already-fully-reversed rules before anything reaches
//!     SQL.
//!   - `cash_shift` (§39) is a workflow row with exactly one legal in-place
//!     transition, OPEN -> CLOSED. [`cash_shift::close_cash_shift`] is the
//!     only place this crate updates a `cash_shift` row after insert, and
//!     it REJECTS the close outright — no write at all — when the derived
//!     variance is non-zero and no reason was supplied.
//!
//! [`crate::Db::record_payment_with_outbox`],
//! [`crate::Db::open_cash_shift_with_outbox`],
//! [`crate::Db::close_cash_shift_with_outbox`] and
//! [`crate::Db::record_paid_in_out_with_outbox`] (in `src/lib.rs`) are the
//! only entry points that call into this module, matching the invoice
//! module's shape: there is no lower-level API that lets a caller insert a
//! payment or shift row without going through this module's validation.

pub(crate) mod cash_shift;
pub(crate) mod tender;
