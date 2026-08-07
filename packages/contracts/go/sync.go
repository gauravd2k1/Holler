// Sync envelope — mirrors src/types/sync.ts. See docs/domain/SYNC_PROTOCOL.md
// and the §50.1 authority rule (ADR-009).
package contracts

import "time"

type AggregateType string

const (
	AggregateTypeOrder    AggregateType = "order"
	AggregateTypeKot      AggregateType = "kot"
	AggregateTypeMenuItem AggregateType = "menu_item"
	AggregateTypePayment  AggregateType = "payment"
)

type SyncDirection string

const (
	SyncDirectionEdgeToCloud SyncDirection = "EDGE_TO_CLOUD"
	SyncDirectionCloudToEdge SyncDirection = "CLOUD_TO_EDGE"
)

type SyncStatus string

const (
	SyncStatusPending SyncStatus = "PENDING"
	SyncStatusSynced  SyncStatus = "SYNCED"
	SyncStatusFailed  SyncStatus = "FAILED"
)

// AggregateAuthority is the single place the §50.1 authority rule is encoded
// for Go-side validation — must match src/types/sync.ts AGGREGATE_AUTHORITY
// exactly; contract-drift tests enforce this.
var AggregateAuthority = map[AggregateType]SyncDirection{
	AggregateTypeOrder:    SyncDirectionEdgeToCloud,
	AggregateTypeKot:      SyncDirectionEdgeToCloud,
	AggregateTypePayment:  SyncDirectionEdgeToCloud,
	AggregateTypeMenuItem: SyncDirectionCloudToEdge,
}

type SyncEnvelope struct {
	RecordID      string          `json:"record_id"`
	TenantID      string          `json:"tenant_id"`
	OutletID      string          `json:"outlet_id"`
	DeviceID      string          `json:"device_id"`
	AggregateType AggregateType   `json:"aggregate_type"`
	Direction     SyncDirection   `json:"direction"`
	CreatedAt     time.Time       `json:"created_at"`
	UpdatedAt     time.Time       `json:"updated_at"`
	Version       int             `json:"version"`
	SyncStatus    SyncStatus      `json:"sync_status"`
	Payload       interface{}     `json:"payload"`
}
