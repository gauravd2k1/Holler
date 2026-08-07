//! The outbox pump (edge→cloud, ADR-007/ADR-009) and its scheduling report.
//! Offline is the normal case (task requirement #8): every expected
//! condition — no connectivity, a rejected envelope, an unroutable
//! (Milestone-2) aggregate, a local authority violation — is captured in
//! [`PumpReport`] rather than raised as an `Err`, so a caller ticking this on
//! a timer never needs to treat "offline" as an exceptional/panicking path.
//! Only a genuine local-database failure propagates as `Err`.

use chrono::Utc;
use holler_edge_database::{model, repo, Db};

use crate::client::{HttpClient, Reply};
use crate::envelope::build_edge_to_cloud_envelope;
use crate::error::{SyncError, SyncResult};
use crate::route::resolve;

/// Static identity of this edge node — set once at enrollment. Not derived
/// from any outbox row: tenant_id in particular has no home in the frozen
/// edge SQLite schema outside `app_user` (ADR-011 note in this crate's
/// report), so the sync worker is the thing that knows it.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub tenant_id: String,
    pub outlet_id: String,
    /// This edge node's own device id. `order` rows carry their own
    /// `device_id` (the till that created them) and that value is used in
    /// preference to this one; `table_session` has no `device_id` column in
    /// the frozen schema, so its envelope uses this node identity instead.
    pub device_id: String,
    pub base_url: String,
}

/// Why [`SyncWorker::pump_outbox`] stopped before draining every pending
/// row. Stopping (rather than skipping ahead) is deliberate: outbox rows are
/// drained in order, and once one has not been acknowledged, sending a later
/// row first would let the cloud observe events out of order for the same
/// aggregate.
#[derive(Debug, PartialEq, Eq)]
pub enum StopReason {
    /// A transport-level failure — no route to the cloud at all. The normal
    /// case for a shop floor with the WAN down.
    Offline,
    /// The cloud was reached but rejected the envelope (4xx/5xx). Reachable,
    /// so not "offline", but this row could not be acknowledged.
    Rejected { status: u16 },
    /// The outbox row's `payload_json` did not have the shape this crate
    /// expects for its `event_type`, or its aggregate row was missing from
    /// the local database. A data-integrity condition, not connectivity.
    MalformedPayload { outbox_id: String, reason: String },
}

#[derive(Debug, Default)]
pub struct PumpReport {
    /// Outbox row ids marked published this call, in the order they were sent.
    pub published: Vec<String>,
    /// Outbox rows skipped because no ingest route exists yet for their
    /// event_type (e.g. `kot` rows before Milestone 2) — left pending,
    /// not an error.
    pub unrouted_skipped: Vec<String>,
    /// Outbox rows skipped because their aggregate_type does not carry
    /// EDGE_TO_CLOUD authority (§50.1) — a data-integrity bug elsewhere, but
    /// handled by refusing to send rather than panicking or crashing sync.
    pub authority_violations: Vec<String>,
    /// Set if draining stopped before exhausting the pending backlog.
    pub stopped: Option<StopReason>,
}

pub struct SyncWorker {
    config: WorkerConfig,
    client: HttpClient,
}

impl SyncWorker {
    pub fn new(config: WorkerConfig) -> Self {
        let client = HttpClient::new(config.base_url.clone());
        Self { config, client }
    }

    /// For tests: inject an already-built client (e.g. pointed at a local
    /// `tiny_http` server) instead of constructing one from `base_url`.
    #[doc(hidden)]
    pub fn with_client(config: WorkerConfig, client: HttpClient) -> Self {
        Self { config, client }
    }

    /// Drains up to `limit` unpublished outbox rows, oldest first, resuming
    /// exactly where a prior call (or a prior process, after a restart) left
    /// off — resumability comes from `published_at` being the only marker of
    /// progress (never deleted, per docs/spec/sync.md), so re-listing
    /// unpublished rows after an interruption picks up mid-backlog with
    /// nothing re-sent and nothing skipped.
    pub fn pump_outbox(&self, db: &mut Db, limit: i64) -> SyncResult<PumpReport> {
        let mut report = PumpReport::default();
        let pending = repo::list_unpublished_outbox(db.connection(), limit)?;
        repo::init_sync_state(db.connection(), &self.config.outlet_id)?;

        for row in pending {
            let event_json: serde_json::Value = match serde_json::from_str(&row.payload_json) {
                Ok(v) => v,
                Err(e) => {
                    self.record_attempt_stop(
                        db,
                        &row.id,
                        false,
                        StopReason::MalformedPayload {
                            outbox_id: row.id.clone(),
                            reason: format!("invalid JSON: {e}"),
                        },
                    )?;
                    report.stopped = Some(StopReason::MalformedPayload {
                        outbox_id: row.id.clone(),
                        reason: format!("invalid JSON: {e}"),
                    });
                    return Ok(report);
                }
            };

            // §50.1 authority check first, before any route mapping — a
            // config aggregate that somehow reached the outbox must never
            // even be considered for sending, regardless of whether this
            // crate happens to recognize its event_type.
            if crate::envelope::authority_for(&row.aggregate_type)
                != Some(crate::envelope::SyncDirection::EdgeToCloud)
            {
                report.authority_violations.push(row.id.clone());
                continue;
            }

            let route = match resolve(
                &row.id,
                &row.aggregate_type,
                &row.event_type,
                &row.aggregate_id,
                &self.config.outlet_id,
                &event_json,
            ) {
                Ok(r) => r,
                Err(SyncError::UnroutedEvent { .. }) => {
                    report.unrouted_skipped.push(row.id.clone());
                    continue;
                }
                Err(SyncError::MalformedPayload { outbox_id, reason }) => {
                    self.record_attempt_stop(
                        db,
                        &row.id,
                        false,
                        StopReason::MalformedPayload {
                            outbox_id: outbox_id.clone(),
                            reason: reason.clone(),
                        },
                    )?;
                    report.stopped = Some(StopReason::MalformedPayload { outbox_id, reason });
                    return Ok(report);
                }
                Err(other) => return Err(other),
            };

            let aggregate = match self.load_aggregate_envelope_fields(db, &row) {
                Ok(Some(fields)) => fields,
                Ok(None) => {
                    let reason = "aggregate row not found locally".to_string();
                    self.record_attempt_stop(
                        db,
                        &row.id,
                        false,
                        StopReason::MalformedPayload {
                            outbox_id: row.id.clone(),
                            reason: reason.clone(),
                        },
                    )?;
                    report.stopped = Some(StopReason::MalformedPayload {
                        outbox_id: row.id.clone(),
                        reason,
                    });
                    return Ok(report);
                }
                Err(e) => return Err(e),
            };

            let envelope = match build_edge_to_cloud_envelope(
                &row.aggregate_type,
                &row.aggregate_id,
                &self.config.tenant_id,
                &aggregate.outlet_id,
                &aggregate.device_id,
                &aggregate.created_at,
                &aggregate.updated_at,
                aggregate.version,
                route.payload,
            ) {
                Ok(e) => e,
                Err(SyncError::AuthorityViolation { .. }) => {
                    report.authority_violations.push(row.id.clone());
                    continue;
                }
                Err(other) => return Err(other),
            };

            let body = serde_json::to_value(&envelope)?;
            match self.client.post_json(&route.path, &body) {
                Ok(Reply::Ok(_)) => {
                    let now = Utc::now().to_rfc3339();
                    repo::mark_outbox_published(db.connection(), &row.id, &now)?;
                    repo::update_sync_cursor(
                        db.connection(),
                        &self.config.outlet_id,
                        Some(&row.id),
                        current_config_version(db, &self.config.outlet_id)?,
                        Some(&now),
                        Some(&now),
                        true,
                    )?;
                    report.published.push(row.id.clone());
                }
                Ok(Reply::Rejected { status }) => {
                    self.record_attempt_stop(db, &row.id, true, StopReason::Rejected { status })?;
                    report.stopped = Some(StopReason::Rejected { status });
                    return Ok(report);
                }
                Err(SyncError::HttpTransport) => {
                    self.record_attempt_stop(db, &row.id, false, StopReason::Offline)?;
                    report.stopped = Some(StopReason::Offline);
                    return Ok(report);
                }
                Err(other) => return Err(other),
            }
        }

        Ok(report)
    }

    fn record_attempt_stop(
        &self,
        db: &mut Db,
        outbox_id: &str,
        online: bool,
        _reason: StopReason,
    ) -> SyncResult<()> {
        let now = Utc::now().to_rfc3339();
        repo::increment_outbox_attempt(db.connection(), outbox_id)?;
        repo::update_sync_cursor(
            db.connection(),
            &self.config.outlet_id,
            None,
            current_config_version(db, &self.config.outlet_id)?,
            Some(&now),
            None,
            online,
        )?;
        Ok(())
    }

    fn load_aggregate_envelope_fields(
        &self,
        db: &Db,
        row: &model::OutboxEntry,
    ) -> SyncResult<Option<AggregateEnvelopeFields>> {
        match row.aggregate_type.as_str() {
            "order" => Ok(db
                .get_order(&row.aggregate_id)?
                .map(|o| AggregateEnvelopeFields {
                    outlet_id: o.outlet_id,
                    device_id: o.device_id,
                    version: o.version,
                    created_at: o.created_at,
                    updated_at: o.updated_at,
                })),
            "table_session" => Ok(db.get_table_session(&row.aggregate_id)?.map(|s| {
                AggregateEnvelopeFields {
                    outlet_id: s.outlet_id,
                    // table_session carries no device_id column in the
                    // frozen schema; use this edge node's own device
                    // identity from WorkerConfig instead.
                    device_id: self.config.device_id.clone(),
                    version: s.version,
                    created_at: s.created_at,
                    updated_at: s.updated_at,
                }
            })),
            _ => Ok(None),
        }
    }
}

struct AggregateEnvelopeFields {
    outlet_id: String,
    device_id: String,
    version: i64,
    created_at: String,
    updated_at: String,
}

fn current_config_version(db: &Db, outlet_id: &str) -> SyncResult<i64> {
    Ok(repo::get_sync_state(db.connection(), outlet_id)?
        .map(|s| s.last_applied_config_version)
        .unwrap_or(0))
}
