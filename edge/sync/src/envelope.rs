//! The §50.1 authority rule (ADR-009), encoded mechanically. This mirrors
//! `packages/contracts/src/types/sync.ts` `AGGREGATE_AUTHORITY` / `go/sync.go`
//! `AggregateAuthority` exactly — do not redesign (docs/spec/sync.md).

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    EdgeToCloud,
    CloudToEdge,
}

impl SyncDirection {
    pub fn as_wire(self) -> &'static str {
        match self {
            SyncDirection::EdgeToCloud => "EDGE_TO_CLOUD",
            SyncDirection::CloudToEdge => "CLOUD_TO_EDGE",
        }
    }
}

/// The aggregate types this crate knows about, restricted to what
/// Milestone 1 exercises through the outbox (`order`, `table_session`) plus
/// `kot`, which may already appear as outbox rows (Milestone 2 KOT sync is
/// out of scope, but the row must not panic the pump — see
/// [`crate::error::SyncError::UnroutedEvent`]).
///
/// Returns `None` for any string outside the contracted `AggregateType` enum
/// (`packages/contracts/src/types/sync.ts`) — an unrecognized aggregate_type
/// is not sent, ever, rather than guessed at.
pub fn authority_for(aggregate_type: &str) -> Option<SyncDirection> {
    match aggregate_type {
        "order" | "kot" | "payment" | "table_session" => Some(SyncDirection::EdgeToCloud),
        "menu_item" | "app_user" | "role" | "restaurant_table" => Some(SyncDirection::CloudToEdge),
        _ => None,
    }
}

/// Wire shape for `packages/contracts` `SyncEnvelope` (`src/types/sync.ts`,
/// `go/sync.go`). Field order/names match the OpenAPI schema exactly.
#[derive(Debug, Clone, Serialize)]
pub struct SyncEnvelope {
    pub record_id: String,
    pub tenant_id: String,
    pub outlet_id: String,
    pub device_id: String,
    pub aggregate_type: String,
    pub direction: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub sync_status: String,
    pub payload: serde_json::Value,
}

/// Builds an envelope, refusing (rather than sending) any aggregate_type
/// whose authority does not match `EDGE_TO_CLOUD` — every route this worker
/// calls is edge→cloud, so this is the single mechanical gate every outbound
/// envelope passes through.
#[allow(clippy::too_many_arguments)]
pub fn build_edge_to_cloud_envelope(
    aggregate_type: &str,
    record_id: &str,
    tenant_id: &str,
    outlet_id: &str,
    device_id: &str,
    created_at: &str,
    updated_at: &str,
    version: i64,
    payload: serde_json::Value,
) -> Result<SyncEnvelope, crate::error::SyncError> {
    let direction = authority_for(aggregate_type).ok_or_else(|| {
        crate::error::SyncError::AuthorityViolation {
            aggregate_type: aggregate_type.to_string(),
            attempted: "EDGE_TO_CLOUD (unknown aggregate_type)",
        }
    })?;
    if direction != SyncDirection::EdgeToCloud {
        return Err(crate::error::SyncError::AuthorityViolation {
            aggregate_type: aggregate_type.to_string(),
            attempted: "EDGE_TO_CLOUD",
        });
    }
    Ok(SyncEnvelope {
        record_id: record_id.to_string(),
        tenant_id: tenant_id.to_string(),
        outlet_id: outlet_id.to_string(),
        device_id: device_id.to_string(),
        aggregate_type: aggregate_type.to_string(),
        direction: direction.as_wire().to_string(),
        created_at: created_at.to_string(),
        updated_at: updated_at.to_string(),
        version,
        sync_status: "PENDING".to_string(),
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_is_edge_to_cloud() {
        assert_eq!(authority_for("order"), Some(SyncDirection::EdgeToCloud));
    }

    #[test]
    fn app_user_is_cloud_to_edge() {
        assert_eq!(authority_for("app_user"), Some(SyncDirection::CloudToEdge));
    }

    #[test]
    fn unknown_aggregate_has_no_authority() {
        assert_eq!(authority_for("nonsense"), None);
    }

    #[test]
    fn building_envelope_for_config_aggregate_is_refused() {
        let err = build_edge_to_cloud_envelope(
            "app_user",
            "r1",
            "t1",
            "o1",
            "d1",
            "2026-08-07T00:00:00Z",
            "2026-08-07T00:00:00Z",
            1,
            serde_json::json!({}),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            crate::error::SyncError::AuthorityViolation { .. }
        ));
    }

    #[test]
    fn building_envelope_for_order_succeeds() {
        let env = build_edge_to_cloud_envelope(
            "order",
            "r1",
            "t1",
            "o1",
            "d1",
            "2026-08-07T00:00:00Z",
            "2026-08-07T00:00:00Z",
            1,
            serde_json::json!({}),
        )
        .expect("order is edge-to-cloud");
        assert_eq!(env.direction, "EDGE_TO_CLOUD");
    }
}
