//! Wastage recording, physical stock counts, variance and snapshot sealing
//! (Milestone 4, track T3, ADR-018 §9/§10.1/§11). Builds on `crate::deduction`
//! (T2, ledger writes and `business_date`) and `crate::inventory` (T1, unit
//! constructors) — this module writes no `recipe`/`inventory_item` config,
//! only the operational rows ADR-018 assigns to this track.
//!
//! Module layout:
//!   - `wastage`  — one `stock_ledger_entry` row per wastage event
//!     (`entry_type='WASTAGE'`, `origin='WASTAGE'`); there is no dedicated
//!     wastage table (0016's own `entry_type` enumeration).
//!   - `count`    — open a `stock_count`, add/correct lines while it is
//!     `OPEN`, complete it — which posts `COUNT_ADJUSTMENT` ledger entries
//!     so the ledger stays the single source of stock.
//!   - `variance` — Actual (counted) vs Theoretical (expected) for a
//!     completed count, as quantity and a basis-point percentage, plus the
//!     "N sales unaccounted" named term (ADR-018 §10.1) — never folded into
//!     shrinkage.
//!   - `snapshot` — `stock_balance_snapshot` sealing: idempotent, lazily
//!     caught up on every `Db::open`, never dependent on an operator; and
//!     the bounded current-stock read every other function in this module
//!     (and T5) reads through.
//!
//! **Permission gating is NOT enforced in this crate.** ADR-018 §11 gates
//! wastage recording on `inventory.manage` and count entry on
//! `inventory.count`, but no permission-check mechanism exists anywhere in
//! `edge/database` today — every existing gate (order amendment rules, cash
//! variance reasons, KOT transition legality) is a data-shape invariant
//! this crate enforces itself, never a role/permission lookup. That lookup
//! happens one layer up, in the Tauri command handlers
//! (`apps/pos/src-tauri`), which is a different crate this task does not
//! own. Every public function below is documented with the permission its
//! caller must have already checked.

pub(crate) mod count;
pub(crate) mod snapshot;
pub(crate) mod variance;
pub(crate) mod wastage;
