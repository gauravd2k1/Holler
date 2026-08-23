//! The outbox pump (edge→cloud, ADR-007/ADR-009) and its scheduling report.
//! Offline is the normal case (task requirement #8): every expected
//! condition — no connectivity, a rejected envelope, an unroutable
//! (Milestone-2) aggregate, a local authority violation, an unverifiable
//! device credential (ADR-017) — is captured in [`PumpReport`] rather than
//! raised as an `Err`, so a caller ticking this on a timer never needs to
//! treat "offline" or "not enrolled" as an exceptional/panicking path. Only
//! a genuine local-database failure propagates as `Err`.

use std::cell::Cell;

use chrono::Utc;
use holler_edge_database::{model, repo, Db};

use crate::client::{HttpClient, Reply};
use crate::envelope::build_edge_to_cloud_envelope;
use crate::error::{SyncError, SyncResult};
use crate::route::resolve;

/// Verification query parameter for [`SyncWorker::verify_enrollment`]: set
/// high enough that the response's filtered arrays (tables/categories/items,
/// and `users` if the caller ever decoded it) come back empty, so the ping
/// costs one small HTTP round trip rather than a full config bundle. Not
/// `i64::MAX` — Go's `strconv.Atoi` targets platform `int`, and staying
/// within `i32`'s range keeps this correct even if the backend ever runs on
/// a 32-bit target.
const VERIFY_SINCE_VERSION: i64 = i32::MAX as i64;

/// Static identity of this edge node — set once at enrollment. Not derived
/// from any outbox row: tenant_id in particular has no home in the frozen
/// edge SQLite schema outside `app_user` (ADR-011 note in this crate's
/// report), so the sync worker is the thing that knows it.
///
/// ADR-017 hole 1: `tenant_id`/`outlet_id`/`device_id` here are still
/// supplied by whoever constructs a `WorkerConfig` — this crate has no way
/// to mint them itself — but they are no longer trusted blind.
/// `device_token` is the enrolled credential (`POST /devices/enroll`,
/// `<credential_id>.<secret>`) that [`SyncWorker`] presents on every cloud
/// request, and before the first envelope of a session is ever sent,
/// [`SyncWorker::verify_enrollment`] confirms the credential actually
/// resolves to `outlet_id` — a locally mis-typed or mis-enrolled `outlet_id`
/// now fails loudly (the cloud 404s: `backend/cmd/api/syncconfig.go`)
/// instead of silently mislabelling every outbound envelope. `tenant_id`
/// remains unverifiable against any contracted wire field — neither
/// `POST /devices/enroll` nor `GET /sync/config` ever echoes a device's
/// resolved `tenant_id` back to the caller — which is a real contract gap
/// this crate cannot close by itself (see this track's report).
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
    /// This edge node's enrolled device credential (ADR-017), presented as
    /// `Authorization: Bearer <device_token>` on every request `SyncWorker`
    /// makes. Never logged, never placed in an error, never persisted by
    /// this crate — storage protection is the caller's responsibility (see
    /// this track's report on where it is kept at rest).
    pub device_token: String,
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

/// What one attempt to reach the cloud produced. Separates "this node is not
/// enrolled" from "the cloud rejected this request", because the two demand
/// different responses from a caller that keeps a per-entry retry budget.
#[derive(Debug)]
pub enum SendOutcome {
    Ok(serde_json::Value),
    /// The cloud was reached and refused this request.
    Rejected { status: u16 },
    /// This node's credential is invalid or does not resolve to its
    /// configured outlet (ADR-017 hole 1). Nothing was sent.
    NotEnrolled { status: u16 },
}

pub struct SyncWorker {
    config: WorkerConfig,
    client: HttpClient,
    /// Set once [`Self::verify_enrollment`] has succeeded this process
    /// lifetime, so a long-running worker pays the extra round trip once per
    /// session rather than once per `pump_outbox` call. `Cell`, not
    /// `Mutex`: every method here takes `&self` but this crate has no
    /// cross-thread sharing requirement (`SyncWorker` is driven by one
    /// caller on a timer, per this module's own doc comment).
    enrollment_verified: Cell<bool>,
}

impl SyncWorker {
    pub fn new(config: WorkerConfig) -> Self {
        let client =
            HttpClient::new(config.base_url.clone()).with_bearer_token(config.device_token.clone());
        Self {
            config,
            client,
            enrollment_verified: Cell::new(false),
        }
    }

    // Accessors for the ranged-replay pump (crate::ranged), which is the
    // same worker's second edge→cloud flow and needs this node's identity and
    // its authenticated client without duplicating either.
    pub(crate) fn tenant_id(&self) -> &str {
        &self.config.tenant_id
    }

    pub(crate) fn outlet_id(&self) -> &str {
        &self.config.outlet_id
    }

    pub(crate) fn device_id(&self) -> &str {
        &self.config.device_id
    }

    /// **The only way anything in this crate reaches the cloud.** Verifies
    /// this node's enrollment once per session, then posts.
    ///
    /// WHY IT IS A CHOKE POINT AND NOT A STEP EACH PUMP REMEMBERS. ADR-017
    /// hole 1 applies to every outbound flow: a node whose credential does
    /// not resolve to `config.outlet_id` must be stopped before it sends,
    /// not after it has mislabelled a run of records. The outbox pump did
    /// that check inline; the ranged pump — written later, against the same
    /// struct — simply did not, and nothing failed, because a dropped check
    /// is invisible until the day it was the check that mattered.
    ///
    /// A second implementation omitting a predecessor's check is not a
    /// discipline problem, it is a structure problem. With `client` private
    /// and this the only path to it, a third flow cannot skip the check
    /// without deleting code that is plainly load-bearing. Making the
    /// omission impossible beats making it detectable.
    ///
    /// A failed VERIFY is reported as [`SendOutcome::NotEnrolled`], distinct
    /// from a rejected POST. The outbox pump treats the two alike, but the
    /// ranged pump must not: it spends a per-entry retry budget on rejections
    /// that are the row's fault, and a credential this node cannot present is
    /// not the row's fault. Collapsing them would let a mis-enrolled device
    /// burn through and abandon a run of perfectly good entries.
    pub(crate) fn post_verified(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> SyncResult<SendOutcome> {
        if !self.enrollment_verified.get() {
            match self.verify_enrollment() {
                Ok(()) => self.enrollment_verified.set(true),
                Err(SyncError::HttpStatus { status }) => {
                    return Ok(SendOutcome::NotEnrolled { status });
                }
                Err(other) => return Err(other),
            }
        }
        Ok(match self.client.post_json(path, body)? {
            Reply::Ok(v) => SendOutcome::Ok(v),
            Reply::Rejected { status } => SendOutcome::Rejected { status },
        })
    }

    /// For tests: inject an already-built client (e.g. pointed at a local
    /// `tiny_http` server) instead of constructing one from `base_url`. The
    /// caller decides whether that client carries a bearer token — this
    /// exists to test transport/retry behaviour independent of auth, so it
    /// deliberately does not force one on.
    #[doc(hidden)]
    pub fn with_client(config: WorkerConfig, client: HttpClient) -> Self {
        Self {
            config,
            client,
            enrollment_verified: Cell::new(false),
        }
    }

    /// Confirms this worker's `device_token` is a currently-valid credential
    /// that resolves to `config.outlet_id` (ADR-017 hole 1). Pings
    /// `GET /sync/config` — the one route that already enforces
    /// `DeviceAuthenticate` and 404s a caller-supplied `outlet_id` that does
    /// not match the credential's own (`backend/cmd/api/syncconfig.go`) — and
    /// discards the body; this is an identity check, not a config pull, so it
    /// deliberately does not go through [`crate::config::apply_bundle`] and
    /// never touches local SQLite. A locally mis-typed or mis-enrolled
    /// `outlet_id` therefore fails loudly here, before this worker ever
    /// builds an envelope carrying it, rather than silently mislabelling
    /// every outbound record.
    fn verify_enrollment(&self) -> SyncResult<()> {
        let _: serde_json::Value = self.client.get_json(&format!(
            "/sync/config?outlet_id={}&since_version={VERIFY_SINCE_VERSION}",
            self.config.outlet_id
        ))?;
        Ok(())
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

            // Enrollment is verified inside `post_verified`, once per
            // session, and only for a row that actually needs the network —
            // a row that never reaches this point (unrouted, an authority
            // violation, a local parse failure) proves those checks work
            // without ever requiring connectivity, which is what
            // `authority_violation_is_refused_locally_and_never_sent` pins.
            // A rejected credential arrives below as `Reply::Rejected`
            // (401/404), which is what a mis-enrolled node must produce:
            // stopped before it sends anything, not after.
            let body = serde_json::to_value(&envelope)?;
            match self.post_verified(&route.path, &body) {
                Ok(SendOutcome::Ok(_)) => {
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
                // A mis-enrolled node and a rejected envelope are handled
                // alike here: both mean the cloud was reached and this row
                // was not accepted, and the outbox keeps no per-row budget
                // that the distinction would change.
                Ok(SendOutcome::Rejected { status } | SendOutcome::NotEnrolled { status }) => {
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
