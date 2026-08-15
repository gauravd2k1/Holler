//! Hand-mirrored wire types for `packages/contracts/src/types/lan.ts` and
//! `packages/contracts/src/types/kot.ts`.
//!
//! There is no Rust binding for `packages/contracts` (ADR-011 addendum), so
//! this module is the Rust half of a contract that also has a TypeScript
//! implementation in `apps/kds`. Keep every field name, JSON tag and enum
//! value byte-for-byte identical to the `.ts` source — `scripts/check-event-
//! type-drift.mjs` scans this directory for the message `type` literals, and
//! nothing else keeps the two ends agreeing on the rest of the shape.
//!
//! This is NOT a sync contract: nothing here is a `SyncEnvelope` or touches
//! `AGGREGATE_AUTHORITY`. It is the wire format for one LAN hop (ADR-014 §6).

use serde::{Deserialize, Serialize};

/// Mirrors `KotStatusSchema` (`packages/contracts/src/types/kot.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KotStatus {
    #[serde(rename = "NEW")]
    New,
    #[serde(rename = "ACKNOWLEDGED")]
    Acknowledged,
    #[serde(rename = "PREPARING")]
    Preparing,
    #[serde(rename = "READY")]
    Ready,
    #[serde(rename = "SERVED")]
    Served,
    #[serde(rename = "CANCELLED")]
    Cancelled,
}

impl KotStatus {
    /// The exact string stored in `holler_edge_database`'s `kot.status`
    /// column / passed to `Db::transition_kot_status_with_outbox`.
    pub fn as_db_str(self) -> &'static str {
        match self {
            KotStatus::New => "NEW",
            KotStatus::Acknowledged => "ACKNOWLEDGED",
            KotStatus::Preparing => "PREPARING",
            KotStatus::Ready => "READY",
            KotStatus::Served => "SERVED",
            KotStatus::Cancelled => "CANCELLED",
        }
    }

    /// A KOT that has left the active set — the trigger for `kot_removed`
    /// rather than `kot_upserted` (ADR-014 §6, lan.ts `kot_removed` comment).
    pub fn is_terminal(self) -> bool {
        matches!(self, KotStatus::Served | KotStatus::Cancelled)
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        Some(match s {
            "NEW" => KotStatus::New,
            "ACKNOWLEDGED" => KotStatus::Acknowledged,
            "PREPARING" => KotStatus::Preparing,
            "READY" => KotStatus::Ready,
            "SERVED" => KotStatus::Served,
            "CANCELLED" => KotStatus::Cancelled,
            _ => return None,
        })
    }
}

/// Mirrors `KotTicketItemSchema`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KotTicketItem {
    pub order_item_id: String,
    pub name: String,
    pub quantity: i64,
    #[serde(default)]
    pub modifiers: Vec<String>,
    pub notes: Option<String>,
}

/// Mirrors `KotSchema`. `schema_version` is always `1` on the wire, matching
/// the frozen `z.literal(1)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kot {
    pub id: String,
    pub order_id: String,
    pub station: String,
    pub sequence: i64,
    pub status: KotStatus,
    pub items: Vec<KotTicketItem>,
    pub created_by_device_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub schema_version: u8,
}

/// Error converting a `holler_edge_database::model::Kot` row (whose
/// `status`/`items_json` are untyped strings) into the wire `Kot`.
#[derive(Debug, thiserror::Error)]
pub enum KotConvertError {
    #[error("kot {kot_id} has unknown status {status:?}")]
    UnknownStatus { kot_id: String, status: String },
    #[error("kot {kot_id} has malformed items_json: {source}")]
    MalformedItems {
        kot_id: String,
        #[source]
        source: serde_json::Error,
    },
}

impl Kot {
    /// Converts the database row shape into the frozen wire shape. Fails
    /// closed (rather than guessing) on a status this crate does not
    /// recognise or items JSON that does not parse — either indicates a
    /// schema drift this crate must not paper over.
    pub fn from_db(row: &holler_edge_database::model::Kot) -> Result<Self, KotConvertError> {
        let status =
            KotStatus::from_db_str(&row.status).ok_or_else(|| KotConvertError::UnknownStatus {
                kot_id: row.id.clone(),
                status: row.status.clone(),
            })?;
        let items: Vec<KotTicketItem> =
            serde_json::from_str(&row.items_json).map_err(|source| {
                KotConvertError::MalformedItems {
                    kot_id: row.id.clone(),
                    source,
                }
            })?;
        Ok(Kot {
            id: row.id.clone(),
            order_id: row.order_id.clone(),
            station: row.station.clone(),
            sequence: row.sequence,
            status,
            items,
            created_by_device_id: row.created_by_device_id.clone(),
            created_at: row.created_at.clone(),
            updated_at: row.updated_at.clone(),
            schema_version: 1,
        })
    }
}

/// Mirrors `KdsLanMessageSchema` (edge -> KDS).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum KdsLanMessage {
    #[serde(rename = "snapshot")]
    Snapshot {
        outlet_id: String,
        sent_at: String,
        kots: Vec<Kot>,
    },
    #[serde(rename = "kot_upserted")]
    KotUpserted {
        outlet_id: String,
        sent_at: String,
        kot: Kot,
    },
    #[serde(rename = "kot_removed")]
    KotRemoved {
        outlet_id: String,
        sent_at: String,
        kot_id: String,
    },
    #[serde(rename = "heartbeat")]
    Heartbeat { outlet_id: String, sent_at: String },
}

/// Mirrors `KdsLanCommandSchema` (KDS -> edge). Intent only: `device_id` here
/// is carried for parsing completeness but MUST NOT be trusted for
/// authorization — the edge uses the identity established on the connection
/// (ADR-014 §6, lan.ts comment). See `server::Connection::device_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum KdsLanCommand {
    #[serde(rename = "set_kot_status")]
    SetKotStatus {
        kot_id: String,
        status: KotStatus,
        device_id: String,
        requested_at: String,
    },
}
