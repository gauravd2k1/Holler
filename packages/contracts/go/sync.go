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

	// Milestone 1 additions (ADR-011).
	AggregateTypeTableSession    AggregateType = "table_session"
	AggregateTypeAppUser         AggregateType = "app_user"
	AggregateTypeRole            AggregateType = "role"
	AggregateTypeRestaurantTable AggregateType = "restaurant_table"

	// Milestone 2 additions (ADR-014). Only the two entities are aggregates:
	// menu_item_station and station_printer are routing rows travelling inside
	// their parent's config bundle, as menu_item_variant and menu_item_modifier
	// already do. print_job and kot_status_history are deliberately absent —
	// see printer.go and sqlite/0005 for why.
	AggregateTypeStation AggregateType = "station"
	AggregateTypePrinter AggregateType = "printer"

	// Milestone 3 additions (ADR-016). tax_rule, invoice_line,
	// payment_allocation, cash_movement and outlet_fiscal_profile are
	// deliberately absent: they are child rows travelling inside their parent's
	// payload or config bundle, as menu_item_variant and station_printer do.
	// invoice_sequence is absent for the opposite reason — it is edge-local and
	// must never sync, the print_job precedent (see invoice.go).
	AggregateTypeInvoice            AggregateType = "invoice"
	AggregateTypeCashShift          AggregateType = "cash_shift"
	AggregateTypeTaxProfile         AggregateType = "tax_profile"
	AggregateTypeComplianceVersion  AggregateType = "compliance_version"
	AggregateTypeInvoiceSeries      AggregateType = "invoice_series"
	AggregateTypeDiscountDefinition AggregateType = "discount_definition"
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

	AggregateTypeTableSession:    SyncDirectionEdgeToCloud,
	AggregateTypeAppUser:         SyncDirectionCloudToEdge,
	AggregateTypeRole:            SyncDirectionCloudToEdge,
	AggregateTypeRestaurantTable: SyncDirectionCloudToEdge,

	// The station's definition is config; its live ticket is a kot. The
	// printer's definition is config; its live work is an edge-local print_job.
	AggregateTypeStation: SyncDirectionCloudToEdge,
	AggregateTypePrinter: SyncDirectionCloudToEdge,

	// Milestone 3 (ADR-016). The outlet issues bills and takes money with the
	// uplink down, so both are edge-authoritative and the cloud only replays.
	AggregateTypeInvoice:   SyncDirectionEdgeToCloud,
	AggregateTypeCashShift: SyncDirectionEdgeToCloud,
	// Tax rules, numbering format and discount policy are management
	// decisions, so they are cloud-owned config. The same cut as station/kot:
	// the series' definition is config, the number it issued lives on an
	// edge-authoritative invoice, and the counter between them never syncs.
	AggregateTypeTaxProfile:         SyncDirectionCloudToEdge,
	AggregateTypeComplianceVersion:  SyncDirectionCloudToEdge,
	AggregateTypeInvoiceSeries:      SyncDirectionCloudToEdge,
	AggregateTypeDiscountDefinition: SyncDirectionCloudToEdge,
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
