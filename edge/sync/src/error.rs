use thiserror::Error;

/// All errors this crate can return. Deliberately typed — no `String`-only
/// errors baked into the public surface, no `unwrap`/`expect` in library
/// paths. `Display` for every variant here is proven (by test) never to
/// contain credential material (ADR-011): none of these variants ever wrap
/// an `AppUser`/`EdgeUserCacheEntry` value or its fields.
#[derive(Debug, Error)]
pub enum SyncError {
    #[error("edge database error: {0}")]
    Db(#[from] holler_edge_database::DbError),

    /// The outbox carried an aggregate_type/direction pair that violates the
    /// §50.1 authority rule. This must never be sent to the cloud — it is
    /// refused locally rather than left for the server to reject.
    #[error("authority violation: aggregate '{aggregate_type}' may not sync {attempted}")]
    AuthorityViolation {
        aggregate_type: String,
        attempted: &'static str,
    },

    /// The outbox row's `event_type` has no known ingest route yet. Not a
    /// failure of the row itself (e.g. `kot` rows are valid outbox entries
    /// under ADR-007/M2 scope but have no contracted ingest route until
    /// Milestone 2) — the pump leaves such rows unpublished and moves on.
    #[error("no ingest route for aggregate '{aggregate_type}' event '{event_type}'")]
    UnroutedEvent {
        aggregate_type: String,
        event_type: String,
    },

    #[error("malformed outbox payload for outbox row {outbox_id}: {reason}")]
    MalformedPayload { outbox_id: String, reason: String },

    #[error("http transport error contacting cloud")]
    HttpTransport,

    #[error("cloud rejected the request: status {status}")]
    HttpStatus { status: u16 },

    #[error("json (de)serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("worker not configured: {0}")]
    Config(&'static str),
}

pub type SyncResult<T> = Result<T, SyncError>;
