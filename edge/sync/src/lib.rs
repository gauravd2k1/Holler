//! Edge sync worker (Milestone 1, task T7).
//!
//! Two flows, both driven by the §50.1 authority rule (ADR-009,
//! docs/spec/sync.md):
//!
//! - [`worker::SyncWorker::pump_outbox`] — edge→cloud. Drains
//!   `local_outbox` in order, wraps each row in a [`envelope::SyncEnvelope`],
//!   posts it to its contracted route, and marks it published (never
//!   deletes) only on a 2xx ack.
//! - [`config::pull_and_apply_config`] — cloud→edge. Pulls the config bundle
//!   and applies it as a wholesale replace, transactionally, only at a
//!   strictly newer `config_version`.
//!
//! Offline is the normal case: both flows report failure through typed
//! values ([`worker::PumpReport`], `Result<bool, _>`) rather than panicking,
//! and neither busy-loops — scheduling the next attempt is the caller's
//! responsibility, using [`backoff::backoff_ms`].

pub mod backoff;
pub mod client;
pub mod config;
pub mod envelope;
pub mod error;
pub mod route;
pub mod worker;

pub use client::HttpClient;
pub use error::{SyncError, SyncResult};
pub use worker::{PumpReport, StopReason, SyncWorker, WorkerConfig};
