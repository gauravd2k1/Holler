//! Procurement replay (Milestone 5, track T3): goods receipts, purchase
//! returns, outbound stock transfers and GRN gaps, edge→cloud.
//!
//! # Why these ride the PLAIN outbox and not a ranged cursor
//!
//! `grn_gap` is a **plain outbox** row — no `entry_seq`, no private counter,
//! no cursor, no contiguity check (ADR-019 §2), and the same is true of the
//! three receipt-shaped aggregates beside it (contracts 0.6.1). That is not an
//! omission, it is the transport rule from ADR-018's addendum applying itself:
//! **ranged sync is for streams; discrete events use the outbox.**
//! `stock_deduction_gap` earned 0.5.8's machinery because it is a per-sale
//! stream at ~5M rows a year. A goods receipt is a business event a buyer acts
//! on — a handful a week — and giving it a private sequence, a cursor and a
//! contiguity check would import 0.5.8's entire failure surface to protect a
//! volume that does not need protecting. Cargo-culting the machinery is a
//! cost, not a safety margin.
//!
//! # What these rows DO take from 0.5.8: the per-entry retry budget
//!
//! The general outbox pump ([`crate::worker::SyncWorker::pump_outbox`]) stops
//! at the first row the cloud will not take, and retries it forever. For
//! `order` that is deliberate — events for one order must reach the cloud in
//! the order they happened. **For procurement it would be an outage**: each
//! receipt is a self-contained fact, and one permanently-rejected GRN would
//! strand every later receipt, return and dispatch behind it, indefinitely and
//! silently. That is the head-of-line failure 0.5.8 named at the edge end, and
//! it does not care which transport it appears on.
//!
//! So this pump spends its budget **PER ENTRY, NOT PER STREAM**:
//!
//! - A **permanent** rejection (the row's own fault — see
//!   [`crate::ranged::is_permanent_rejection`], the single classifier both
//!   pumps share) charges one attempt to that row and **moves on to the next
//!   row in the same pass**. Nothing behind it is stranded, ever.
//! - After [`MAX_PROCUREMENT_REPLAY_ATTEMPTS`] such rejections, on that many
//!   separate passes, the row is **abandoned**: it is no longer sent, and it
//!   becomes visible to a human through
//!   [`holler_edge_database::Db::list_over_budget_procurement_replays`]. It is
//!   never deleted and never marked published, so a fixed cloud and a manual
//!   retry can still land it.
//! - A **transient** condition — offline, 5xx, 401/403, 404, 408, 429 — spends
//!   **nothing**, and stops the pass where it stands. Retrying those forever is
//!   safe precisely because nothing at the outlet depends on the uplink
//!   (ADR-013), and charging them to a row would abandon good receipts during
//!   an outage: data loss dressed as resilience.
//!
//! Halting replay for one row is survivable. Halting it silently is not.

use chrono::Utc;
use holler_edge_database::{repo, Db};
use serde_json::Value;

use crate::envelope::build_edge_to_cloud_envelope;
use crate::error::{SyncError, SyncResult};
use crate::ranged::is_permanent_rejection;
use crate::worker::{SendOutcome, StopReason, SyncWorker};

/// See [`repo::OUTBOX_PERMANENT_REJECTION_BUDGET`] — defined in
/// `edge/database` because the POS surface that shows an abandoned receipt to
/// a human reads it through that crate and does not depend on this one.
pub const MAX_PROCUREMENT_REPLAY_ATTEMPTS: i64 = repo::OUTBOX_PERMANENT_REJECTION_BUDGET;

/// How many procurement rows one pass sends. Bounded so a long-offline outlet
/// reconnecting does not attempt its whole backlog in one call.
pub const PROCUREMENT_BATCH_LIMIT: i64 = 100;

/// A row this pass gave up on. The durable record is the outbox row itself,
/// still unpublished with `attempt_count >= MAX_PROCUREMENT_REPLAY_ATTEMPTS`;
/// this is the in-call report.
#[derive(Debug, PartialEq, Eq)]
pub struct BlockedProcurementEntry {
    pub outbox_id: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub status: Option<u16>,
}

#[derive(Debug, Default)]
pub struct ProcurementReport {
    /// Outbox ids marked published this call, in send order.
    pub published: Vec<String>,
    /// Rows whose budget was spent THIS call.
    pub blocked: Vec<BlockedProcurementEntry>,
    /// Rows already over budget on entry to this call: not sent, not retried,
    /// and reported every pass rather than skipped in silence.
    pub over_budget: Vec<String>,
    /// Rows refused locally because their aggregate_type does not carry
    /// EDGE_TO_CLOUD authority (§50.1), or whose payload did not have the
    /// shape its event_type requires. A data-integrity condition elsewhere;
    /// handled by not sending, never by panicking.
    pub refused_locally: Vec<String>,
    /// Set if the pass stopped before draining its batch. Costs no row any
    /// part of its budget.
    pub stopped: Option<StopReason>,
    /// What the cloud ECHOED for each accepted row, in send order, paired with
    /// the outbox id. An ack and an ack-plus-the-row-it-became are different
    /// claims, and only the second can be checked (M5 acceptance criterion 6).
    pub acked_echo: Vec<(String, Value)>,
}

/// Maps a procurement outbox row to its contracted ingest route and payload
/// (`packages/contracts/openapi/openapi.yaml`, 0.6.0).
///
/// `/procurement/goods-receipts` takes **two** aggregate types for the reason
/// `/inventory/ledger-entries` does: a gap records what could not be matched
/// *about this receipt* and belongs beside the receipt it explains. A gap
/// arriving by another path could not be joined to it.
///
/// Every arm matches an explicit `(aggregate_type, event_type)` pair — no
/// wildcard. An unrecognised pair is [`SyncError::UnroutedEvent`], never
/// replayed against a route that happens to also fit.
fn resolve_procurement(
    outbox_id: &str,
    aggregate_type: &str,
    event_type: &str,
    event_json: &Value,
) -> Result<(&'static str, Value), SyncError> {
    let (path, key) = match (aggregate_type, event_type) {
        ("goods_receipt_note", "GoodsReceived") => {
            ("/procurement/goods-receipts", "goods_receipt_note")
        }
        ("grn_gap", "GrnGapRecorded") => ("/procurement/goods-receipts", "grn_gap"),
        ("purchase_return", "PurchaseReturned") => {
            ("/procurement/purchase-returns", "purchase_return")
        }
        ("stock_transfer_out", "StockDispatched") => {
            ("/procurement/stock-transfers-out", "stock_transfer_out")
        }
        _ => {
            return Err(SyncError::UnroutedEvent {
                aggregate_type: aggregate_type.to_string(),
                event_type: event_type.to_string(),
            })
        }
    };
    let payload = event_json
        .get("data")
        .and_then(|d| d.get(key))
        .ok_or_else(|| SyncError::MalformedPayload {
            outbox_id: outbox_id.to_string(),
            reason: format!("missing data.{key}"),
        })?
        .clone();
    Ok((path, payload))
}

impl SyncWorker {
    /// Replays this outlet's procurement backlog, oldest first, **never
    /// stopping on a row the cloud will not take**.
    ///
    /// See the module doc comment for the whole rule. The one-line version:
    /// a permanent rejection charges one attempt to that row and the pass
    /// continues; a transient one charges nothing and the pass stops.
    pub fn pump_procurement(&self, db: &mut Db, limit: i64) -> SyncResult<ProcurementReport> {
        let mut report = ProcurementReport::default();
        repo::init_sync_state(db.connection(), self.outlet_id())?;

        // Enrollment (ADR-017 hole 1) is checked inside `post_verified`, the
        // only path this crate has to the cloud — not re-checked here. A third
        // flow forgetting a predecessor's check is a structure problem, and
        // the structure is what stops it.
        let pending = repo::list_unpublished_procurement_outbox(db.connection(), limit)?;

        for row in pending {
            // Already abandoned. Reported every pass — an abandoned receipt
            // nothing mentions again is the silent halt this whole design
            // exists to avoid — but not retried: that is what "the budget is
            // finite" means.
            if row.attempt_count >= MAX_PROCUREMENT_REPLAY_ATTEMPTS {
                report.over_budget.push(row.id.clone());
                continue;
            }

            let event_json: Value = match serde_json::from_str(&row.payload_json) {
                Ok(v) => v,
                Err(_) => {
                    report.refused_locally.push(row.id.clone());
                    continue;
                }
            };

            let (path, payload) = match resolve_procurement(
                &row.id,
                &row.aggregate_type,
                &row.event_type,
                &event_json,
            ) {
                Ok(v) => v,
                // Unroutable or misshapen: a local condition, and one no
                // number of retries fixes. Refuse it here rather than
                // stopping the pass — the rows behind it are innocent.
                Err(_) => {
                    report.refused_locally.push(row.id.clone());
                    continue;
                }
            };

            // These four aggregates are IMMUTABLE once written (ADR-019): a
            // receipt is corrected by an appended movement, never a mutation,
            // so `version` is 1 by construction and created_at == updated_at.
            // The instant is the event's own `occurred_at`, which the row
            // carries — no second read, and no chance of the envelope
            // disagreeing with the payload about when this happened.
            let occurred_at = event_json
                .get("occurred_at")
                .and_then(Value::as_str)
                .unwrap_or(&row.created_at)
                .to_string();

            let envelope = match build_edge_to_cloud_envelope(
                &row.aggregate_type,
                &row.aggregate_id,
                self.tenant_id(),
                self.outlet_id(),
                self.device_id(),
                &occurred_at,
                &occurred_at,
                1,
                payload,
            ) {
                Ok(e) => e,
                // §50.1 violation: refuse locally, never send. Not the
                // cloud's business and not chargeable to a retry budget.
                Err(SyncError::AuthorityViolation { .. }) => {
                    report.refused_locally.push(row.id.clone());
                    continue;
                }
                Err(other) => return Err(other),
            };

            let body = serde_json::to_value(&envelope)?;
            match self.post_verified(path, &body) {
                Ok(SendOutcome::Ok(echo)) => {
                    let now = Utc::now().to_rfc3339();
                    // Marked published, never deleted (docs/spec/sync.md).
                    repo::mark_outbox_published(db.connection(), &row.id, &now)?;
                    report.published.push(row.id.clone());
                    report.acked_echo.push((row.id.clone(), echo));
                }

                // Nothing was sent and this node cannot send anything. Not
                // the row's fault; charge it to nothing and stop.
                Ok(SendOutcome::NotEnrolled { status }) => {
                    report.stopped = Some(StopReason::Rejected { status });
                    return Ok(report);
                }

                Ok(SendOutcome::Rejected { status }) if is_permanent_rejection(status) => {
                    repo::increment_outbox_attempt(db.connection(), &row.id)?;
                    let attempts = row.attempt_count + 1;
                    if attempts >= MAX_PROCUREMENT_REPLAY_ATTEMPTS {
                        report.blocked.push(BlockedProcurementEntry {
                            outbox_id: row.id.clone(),
                            aggregate_type: row.aggregate_type.clone(),
                            aggregate_id: row.aggregate_id.clone(),
                            status: Some(status),
                        });
                    }
                    // THE NON-WEDGING PROPERTY, and the only line in this file
                    // that really matters: the pass CONTINUES. A receipt the
                    // cloud will never accept must not strand the receipts
                    // behind it — detection is the goal, blocking is the side
                    // effect nobody wants.
                    continue;
                }

                Ok(SendOutcome::Rejected { status }) => {
                    // Transient or device-level. Costs no budget, and the
                    // whole pass stops: the next row would get the same
                    // answer, and spending attempts on it would abandon good
                    // rows during an outage.
                    report.stopped = Some(StopReason::Rejected { status });
                    return Ok(report);
                }

                Err(SyncError::HttpTransport) => {
                    report.stopped = Some(StopReason::Offline);
                    return Ok(report);
                }
                Err(other) => return Err(other),
            }
        }

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(event_type: &str, key: &str) -> Value {
        serde_json::json!({
            "event_type": event_type,
            "occurred_at": "2026-08-30T06:00:00Z",
            "data": { key: { "id": "rec-1" } },
        })
    }

    #[test]
    fn each_procurement_aggregate_routes_to_its_contracted_path() {
        let cases = [
            (
                "goods_receipt_note",
                "GoodsReceived",
                "goods_receipt_note",
                "/procurement/goods-receipts",
            ),
            (
                "grn_gap",
                "GrnGapRecorded",
                "grn_gap",
                "/procurement/goods-receipts",
            ),
            (
                "purchase_return",
                "PurchaseReturned",
                "purchase_return",
                "/procurement/purchase-returns",
            ),
            (
                "stock_transfer_out",
                "StockDispatched",
                "stock_transfer_out",
                "/procurement/stock-transfers-out",
            ),
        ];
        for (aggregate, event_type, key, path) in cases {
            let (resolved, payload) =
                resolve_procurement("ob1", aggregate, event_type, &event(event_type, key))
                    .expect("routes");
            assert_eq!(resolved, path);
            assert_eq!(payload["id"], "rec-1");
        }
    }

    /// A gap and the receipt it explains share one route, deliberately: a gap
    /// arriving by another path could not be joined to the receipt.
    #[test]
    fn a_gap_travels_to_the_same_route_as_the_receipt_it_explains() {
        let (receipt_path, _) = resolve_procurement(
            "ob1",
            "goods_receipt_note",
            "GoodsReceived",
            &event("GoodsReceived", "goods_receipt_note"),
        )
        .expect("routes");
        let (gap_path, _) = resolve_procurement(
            "ob2",
            "grn_gap",
            "GrnGapRecorded",
            &event("GrnGapRecorded", "grn_gap"),
        )
        .expect("routes");
        assert_eq!(receipt_path, gap_path);
    }

    #[test]
    fn an_unknown_event_type_for_a_known_aggregate_is_unrouted_not_swallowed() {
        let err = resolve_procurement(
            "ob1",
            "goods_receipt_note",
            "NotAFrozenEvent",
            &event("NotAFrozenEvent", "goods_receipt_note"),
        )
        .unwrap_err();
        assert!(matches!(err, SyncError::UnroutedEvent { .. }));
    }

    #[test]
    fn a_payload_missing_its_aggregate_is_malformed_not_sent_empty() {
        let err = resolve_procurement(
            "ob1",
            "purchase_return",
            "PurchaseReturned",
            &serde_json::json!({ "data": {} }),
        )
        .unwrap_err();
        assert!(matches!(err, SyncError::MalformedPayload { .. }));
    }
}
