//! Stock ledger writes and automatic deduction inside `confirm_order`
//! (Milestone 4, track T2, ADR-018). Builds on `crate::inventory` (T1) for
//! recipe resolution — this module never resolves a recipe itself, only
//! turns what T1's resolver returns into persisted rows.
//!
//! Module layout:
//!   - `business_date` — ADR-018 §9.2: `business_date`, computed once from
//!     `outlet.timezone`/`outlet.day_start_time` at write time and stored,
//!     never recomputed on read.
//!   - `ledger`        — `stock_ledger_entry`/`stock_deduction_gap` inserts,
//!     `entry_seq` assignment, and [`ledger::deduct_stock_for_confirmed_order`],
//!     the one entry point [`crate::Db::confirm_order_with_outbox`] calls.
//!
//! **This module must never be able to abort `confirm_order`'s transaction
//! for a business/config reason** — see `ledger`'s module doc comment for
//! the full statement of that rule and what it does and does not cover.

pub(crate) mod business_date;
pub(crate) mod ledger;
