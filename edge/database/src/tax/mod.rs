//! The edge-side tax engine (Milestone 3, track T7a, ADR-016).
//!
//! `backend/internal/compliance` is the REFERENCE implementation and CONFIG
//! authority (`tax_profile`/`tax_rule`/`compliance_version` are
//! CLOUD_TO_EDGE, ADR-016 §1). The `invoice` that USES these rules is
//! edge-authoritative (§50.1) — the edge bills a customer with the uplink
//! down, so this module must produce byte-identical money to the Go engine
//! or the two stores disagree about what a customer paid. See
//! `tests/tax_parity.rs` for the cross-engine fixture assertion.
//!
//! No floating point anywhere, at any intermediate step. Every function in
//! this module operates on `i64` paise/basis-point arithmetic only
//! (CLAUDE.md forbids float for money, and a rate that multiplies money
//! inherits the rule).
//!
//! Module layout mirrors `backend/internal/compliance` file-for-file so a
//! reviewer can diff the two directly:
//!   - `rounding` <-> `rounding.go`
//!   - `domain`   <-> `domain.go`
//!   - `resolve`  <-> `resolve.go` (Task 2: effective-dated resolution)
//!   - `engine`   <-> `engine.go`  (Task 3/4: compute, both pricing modes)
//!   - `snapshot` <-> `snapshot.go` (Task 5: the tax_snapshot an invoice stores)

mod domain;
mod engine;
mod resolve;
mod rounding;
mod snapshot;

pub use domain::{Line, LineComputation, ResolvedRate, TaxComponent, PricingMode, InvoiceTotals, COMPONENT_ORDER};
pub use engine::compute_invoice;
pub use resolve::{parse_utc, resolve_compliance_version, resolve_rates, resolve_tax_profile};
pub use snapshot::{build_tax_snapshot, build_tax_snapshots, render_tax_snapshots, TaxSnapshot};

// `rounding`'s functions (round_half_up_div, round_component_paise,
// round_to_nearest_rupee, largest_remainder_split) are deliberately NOT
// re-exported: they are the engine's internal arithmetic primitives, used
// only by `engine.rs`, and exposing them would invite a caller to reimplement
// the ADR-016 §3 policy piecemeal instead of going through `compute_invoice`.
