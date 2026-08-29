//! Ranged replay of the two high-volume stock streams (contracts 0.5.8,
//! ADR-018's transport rule).
//!
//! `stock_ledger_entry` and `stock_deduction_gap` are rows in a stream, not
//! individually meaningful business events, so they do not go on the outbox:
//! 15,000 ledger rows a day would mean 15,000 outbox rows a day on top, on a
//! 4GB spinning disk. Instead each stream carries a per-outlet monotone
//! `entry_seq`, the edge sends `entry_seq > cursor` in order, and the cursor
//! advances on ack. The envelope is unchanged — ranged sync changes the
//! cursor, not the wrapper.
//!
//! # The two ways this could become an outage, and what stops each
//!
//! **At the cloud.** Rejecting an entry because its `entry_seq` is beyond the
//! cloud's high-water mark turns one lost row into a permanent halt: nothing
//! after the hole is ever accepted. The cloud therefore accepts the entry and
//! records the hole (`ledger_replay_gap`). Detection is the goal; blocking was
//! a side effect nobody wanted.
//!
//! **At the edge — this file.** The mirror image: if the cloud permanently
//! rejects entry 7 and this pump retries 7 forever, entries 8..N never leave
//! the outlet. Same outage, opposite end. So the retry budget here is spent
//! PER ENTRY, not per stream: after [`MAX_ENTRY_REPLAY_ATTEMPTS`] permanent
//! rejections the entry is recorded in `sync_replay_block`, the cursor moves
//! past it, and the rest of the stream replays. The skipped mark then reaches
//! the cloud as a hole, so the same fact is visible from both ends.
//!
//! Halting sync is survivable — no core outlet path depends on the uplink
//! (ADR-013). Halting it silently is not.

use chrono::Utc;
use holler_edge_database::{repo, repo::ReplayStream, Db};

use crate::envelope::build_edge_to_cloud_envelope;
use crate::error::{SyncError, SyncResult};
use crate::worker::{SendOutcome, StopReason, SyncWorker};

/// The contracted ingest route for both streams. A gap belongs beside the
/// movements it failed to produce, so ADR-018 §10.1 pins a SET of aggregate
/// types on one route rather than giving the gap its own.
const INGEST_PATH: &str = "/inventory/ledger-entries";

/// How many PERMANENT rejections one entry gets before the stream is allowed
/// to move past it.
///
/// Not a tuning knob so much as a statement that the number is finite.
/// Transient conditions never spend this budget (see
/// [`is_permanent_rejection`]), so reaching it means the cloud has said no to
/// this specific row five times on five separate passes — retrying a sixth
/// costs the whole rest of the stream.
pub const MAX_ENTRY_REPLAY_ATTEMPTS: i64 = 5;

/// How many entries one pass sends per stream. Bounded so a long-offline
/// outlet reconnecting does not attempt its entire backlog in one call.
pub const RANGED_BATCH_LIMIT: i64 = 200;

/// An entry this pass gave up on. Also written to `sync_replay_block`, which
/// is the durable, human-visible record; this is the in-call report.
#[derive(Debug, PartialEq, Eq)]
pub struct BlockedEntry {
    pub stream: &'static str,
    pub entry_seq: i64,
    pub record_id: String,
    pub status: Option<u16>,
}

#[derive(Debug, Default)]
pub struct RangedReport {
    /// `entry_seq` values acknowledged by the cloud this call, per stream, in
    /// send order.
    pub ledger_acked: Vec<i64>,
    pub gap_acked: Vec<i64>,
    /// Entries abandoned this call after exhausting their budget. Never
    /// silent: each one is a row in `sync_replay_block` with `blocked_at`
    /// set, and a hole the cloud will record at its end.
    pub blocked: Vec<BlockedEntry>,
    /// Set if a stream stopped before draining its send set.
    pub stopped: Option<StopReason>,
    /// What the cloud ECHOED for each accepted entry, in send order, paired
    /// with the mark it acknowledged.
    ///
    /// The ingest route returns the stored row, so this is the cloud's own
    /// account of what it now holds -- not the edge's account of what it
    /// sent. Keeping it makes an ack answerable: "acked 7" and "acked 7, and
    /// here is the row it became" are different claims, and only the second
    /// one can be checked. M4 acceptance criterion 6 checks it; a support
    /// question about a divergent outlet needs exactly the same thing.
    ///
    /// Bounded by the batch limit and dropped with the report.
    pub acked_echo: Vec<(i64, serde_json::Value)>,
}

/// Whether a rejection is the ENTRY's fault, and so spends its budget.
///
/// **The single classifier for both edge→cloud pumps that keep a budget** —
/// [`crate::procurement`] shares it rather than restating it. Two copies of
/// this decision would drift, and the drift would be invisible until the day
/// one of them abandoned a row during an outage.
///
/// Transient and device-level conditions must not: the uplink being down, the
/// cloud restarting, a rate limit, an expired credential — none of those are
/// caused by this row, and spending a per-entry budget on them would abandon
/// perfectly good entries during an outage, which is data loss dressed as
/// resilience. Those stop the stream and are retried indefinitely, which is
/// safe precisely because nothing at the outlet depends on the uplink.
pub(crate) fn is_permanent_rejection(status: u16) -> bool {
    match status {
        // Unauthorized / forbidden: this device's credential, not this row.
        401 | 403 => false,
        // The route is not there. A deployment or version problem — every
        // entry would get the same answer, so charging it to this one
        // abandons rows for a reason that has nothing to do with them.
        404 => false,
        // Timeout, rate limit: come back later, unchanged.
        408 | 429 => false,
        // The cloud said this row is wrong and will say so again.
        400..=499 => true,
        // 5xx and anything else: the cloud is unwell, not the row.
        _ => false,
    }
}

impl SyncWorker {
    /// Replays both ranged streams for this worker's outlet, oldest mark
    /// first, advancing each stream's cursor independently.
    ///
    /// The two cursors are separate because the two counters are separate:
    /// both mint 1, 2, 3… and one mark cannot mean two positions. A stream
    /// that stops does not stop the other.
    pub fn pump_ranged_streams(&self, db: &mut Db, limit: i64) -> SyncResult<RangedReport> {
        let mut report = RangedReport::default();
        repo::init_sync_state(db.connection(), self.outlet_id())?;

        // Enrollment (ADR-017 hole 1) is not checked here: it is checked
        // inside `SyncWorker::post_verified`, which is the only path this
        // crate has to the cloud. This pump forgetting that check is exactly
        // what happened when it was written, and why the check moved into
        // the send path rather than staying a step each pump remembers.

        self.pump_one_stream(db, ReplayStream::Ledger, limit, &mut report)?;
        // Deliberately continues even if the ledger stream stopped: a gap row
        // is the signal that a sale went unaccounted, and holding it back
        // because a movement was rejected would suppress the one record that
        // explains the missing movements.
        self.pump_one_stream(db, ReplayStream::DeductionGap, limit, &mut report)?;

        Ok(report)
    }

    fn pump_one_stream(
        &self,
        db: &mut Db,
        stream: ReplayStream,
        limit: i64,
        report: &mut RangedReport,
    ) -> SyncResult<()> {
        let outlet_id = self.outlet_id().to_string();
        let cursor = repo::get_replay_cursor(db.connection(), &outlet_id, stream)?;

        // `entry_seq > cursor`, ordered by the mark. A row that arrives late
        // still carries a mark above the cursor, so it is picked up on the
        // next pass rather than being lost to a date predicate — the same
        // self-healing property the sealed snapshot relies on.
        let pending = self.ranged_batch(db, &outlet_id, stream, cursor, limit)?;

        for (entry_seq, record_id, payload, occurred_at) in pending {
            let envelope = build_edge_to_cloud_envelope(
                aggregate_type_for(stream),
                &record_id,
                self.tenant_id(),
                &outlet_id,
                self.device_id(),
                &occurred_at,
                &occurred_at,
                // Stream rows are append-only and never amended, so their
                // version is 1 by construction. There is no update path that
                // could make it anything else.
                1,
                payload,
            )?;

            let body = serde_json::to_value(&envelope)?;
            let now = Utc::now().to_rfc3339();

            match self.post_verified(INGEST_PATH, &body) {
                Ok(SendOutcome::Ok(echo)) => {
                    // An entry that failed earlier and has now been accepted
                    // leaves no block behind — a surface full of resolved
                    // alarms stops being read, which is the outcome a table
                    // was chosen over a log line to avoid.
                    repo::clear_replay_failure(db.connection(), &outlet_id, stream, entry_seq)?;
                    repo::advance_replay_cursor(db.connection(), &outlet_id, stream, entry_seq)?;
                    match stream {
                        ReplayStream::Ledger => report.ledger_acked.push(entry_seq),
                        ReplayStream::DeductionGap => report.gap_acked.push(entry_seq),
                    }
                    report.acked_echo.push((entry_seq, echo));
                }

                // Nothing was sent and this node cannot send anything: stop,
                // and charge it to no entry. A credential problem is not the
                // row's fault, and abandoning good entries over one would be
                // data loss dressed as resilience.
                Ok(SendOutcome::NotEnrolled { status }) => {
                    report.stopped = Some(StopReason::Rejected { status });
                    return Ok(());
                }

                Ok(SendOutcome::Rejected { status }) if is_permanent_rejection(status) => {
                    let attempts = repo::record_replay_failure(
                        db.connection(),
                        &outlet_id,
                        stream,
                        entry_seq,
                        &record_id,
                        Some(status),
                        &format!(
                            "cloud rejected {} with status {status}",
                            aggregate_type_for(stream)
                        ),
                        &now,
                    )?;

                    if attempts < MAX_ENTRY_REPLAY_ATTEMPTS {
                        // Still inside the budget: stop this stream and try
                        // the same entry again next pass. Order matters, so
                        // we do not skip ahead while an entry may still land.
                        report.stopped = Some(StopReason::Rejected { status });
                        return Ok(());
                    }

                    // Budget spent. Move PAST the entry rather than retrying
                    // it forever — the whole point of a per-entry bound.
                    repo::mark_replay_blocked(
                        db.connection(),
                        &outlet_id,
                        stream,
                        entry_seq,
                        &now,
                    )?;
                    repo::advance_replay_cursor(db.connection(), &outlet_id, stream, entry_seq)?;
                    report.blocked.push(BlockedEntry {
                        stream: stream.as_str(),
                        entry_seq,
                        record_id,
                        status: Some(status),
                    });
                }

                Ok(SendOutcome::Rejected { status }) => {
                    // Transient or device-level. Costs no budget.
                    report.stopped = Some(StopReason::Rejected { status });
                    return Ok(());
                }

                Err(SyncError::HttpTransport) => {
                    report.stopped = Some(StopReason::Offline);
                    return Ok(());
                }
                Err(other) => return Err(other),
            }
        }

        Ok(())
    }

    /// One stream's send set, normalised to `(entry_seq, record_id, payload,
    /// occurred_at)` so the send loop above is identical for both.
    fn ranged_batch(
        &self,
        db: &Db,
        outlet_id: &str,
        stream: ReplayStream,
        cursor: i64,
        limit: i64,
    ) -> SyncResult<Vec<(i64, String, serde_json::Value, String)>> {
        Ok(match stream {
            ReplayStream::Ledger => {
                repo::list_ledger_entries_after(db.connection(), outlet_id, cursor, limit)?
                    .into_iter()
                    .map(|e| {
                        let occurred_at = e.occurred_at.clone();
                        let id = e.id.clone();
                        (e.entry_seq, id, ledger_entry_payload(&e), occurred_at)
                    })
                    .collect()
            }
            ReplayStream::DeductionGap => {
                repo::list_deduction_gaps_after(db.connection(), outlet_id, cursor, limit)?
                    .into_iter()
                    .map(|g| {
                        let occurred_at = g.occurred_at.clone();
                        let id = g.id.clone();
                        (g.entry_seq, id, deduction_gap_payload(&g), occurred_at)
                    })
                    .collect()
            }
        })
    }
}

fn aggregate_type_for(stream: ReplayStream) -> &'static str {
    match stream {
        ReplayStream::Ledger => "stock_ledger_entry",
        ReplayStream::DeductionGap => "stock_deduction_gap",
    }
}

/// `StockLedgerEntry` on the wire (`packages/contracts` `StockLedgerEntry`).
/// Written out field by field rather than derived, because the edge model and
/// the contract type are allowed to diverge and a `#[derive(Serialize)]`
/// would let them do it silently.
fn ledger_entry_payload(e: &holler_edge_database::model::StockLedgerEntry) -> serde_json::Value {
    serde_json::json!({
        "id": e.id,
        "outlet_id": e.outlet_id,
        "entry_seq": e.entry_seq,
        "inventory_item_id": e.inventory_item_id,
        "inventory_item_name": e.inventory_item_name,
        "dimension": e.dimension,
        "entry_type": e.entry_type,
        "origin": e.origin,
        "quantity_applied_micro": e.quantity_applied_micro,
        "recipe_id": e.recipe_id,
        "recipe_version": e.recipe_version,
        "recipe_name": e.recipe_name,
        "source_order_id": e.source_order_id,
        "source_order_item_id": e.source_order_item_id,
        "reason_code": e.reason_code,
        "note": e.note,
        "occurred_at": e.occurred_at,
        "business_date": e.business_date,
        "created_by_user_id": e.created_by_user_id,
        "modifier_delta_id": e.modifier_delta_id,
        "modifier_name": e.modifier_name,
        "modifier_delta_version": e.modifier_delta_version,
        "unit_cost_paise": e.unit_cost_paise,
        "source_stock_count_id": e.source_stock_count_id,
        "schema_version": 1,
    })
}

fn deduction_gap_payload(g: &holler_edge_database::model::StockDeductionGap) -> serde_json::Value {
    serde_json::json!({
        "id": g.id,
        "outlet_id": g.outlet_id,
        "entry_seq": g.entry_seq,
        "order_id": g.order_id,
        "order_item_id": g.order_item_id,
        "menu_item_id": g.menu_item_id,
        "menu_item_variant_id": g.menu_item_variant_id,
        "menu_item_name": g.menu_item_name,
        "quantity": g.quantity,
        "reason": g.reason,
        "occurred_at": g.occurred_at,
        "business_date": g.business_date,
        "schema_version": 1,
    })
}
