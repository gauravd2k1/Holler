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

	// Milestone 4 additions (ADR-018). item_unit_conversion,
	// recipe_ingredient, modifier_ingredient_delta and stock_count_line are
	// deliberately absent — child rows travelling inside their parent's
	// payload or config bundle. stock_balance_snapshot is absent for the
	// invoice_sequence reason: it is an edge-local derived projection and must
	// never sync. The cloud may re-derive its own stock view from the ledger;
	// it may never mirror the edge's.
	AggregateTypeInventoryItem     AggregateType = "inventory_item"
	AggregateTypeRecipe            AggregateType = "recipe"
	AggregateTypeStockLedgerEntry  AggregateType = "stock_ledger_entry"
	AggregateTypeStockCount        AggregateType = "stock_count"
	AggregateTypeStockDeductionGap AggregateType = "stock_deduction_gap"

	// Milestone 5 additions (ADR-019). SupplierItem, PurchaseOrderLine,
	// GrnLine, PurchaseReturnLine and StockTransferLine are deliberately
	// absent — child rows travelling inside their parent's payload or config
	// bundle. grn_sequence is absent for the invoice_sequence reason
	// (edge-local counter). supplier_invoice and supplier_credit are absent
	// for the refresh_token reason (cloud-only).
	AggregateTypeSupplier         AggregateType = "supplier"
	AggregateTypePurchaseOrder    AggregateType = "purchase_order"
	AggregateTypeGoodsReceiptNote AggregateType = "goods_receipt_note"
	AggregateTypeGrnGap           AggregateType = "grn_gap"
	AggregateTypePurchaseReturn   AggregateType = "purchase_return"
	AggregateTypeStockTransferOut AggregateType = "stock_transfer_out"
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

	// Milestone 4 (ADR-018). The same cut as every milestone before it: a raw
	// material's definition and a recipe are management decisions, while
	// consuming, wasting and counting stock are shop-floor transactions the
	// outlet performs with the uplink down.
	AggregateTypeInventoryItem:    SyncDirectionCloudToEdge,
	AggregateTypeRecipe:           SyncDirectionCloudToEdge,
	AggregateTypeStockLedgerEntry: SyncDirectionEdgeToCloud,
	AggregateTypeStockCount:       SyncDirectionEdgeToCloud,
	// A signal, not a correction — cloud-visible because the person who can
	// see it and the person who can fix it are different people in different
	// places. Shares the ledger ingest route rather than taking its own.
	AggregateTypeStockDeductionGap: SyncDirectionEdgeToCloud,

	// Milestone 5 (ADR-019). Who we buy from and what we ordered are
	// management decisions; what physically arrived at the door, what went
	// back, and what was dispatched are shop-floor transactions the outlet
	// performs with the uplink down.
	AggregateTypeSupplier: SyncDirectionCloudToEdge,
	// NO RECEIPT STATE on this aggregate. Receipt progress is derived on both
	// sides and the two derivations legitimately differ — the edge sees only
	// its own GRN lines, the cloud sees every outlet's. Storing it would make
	// the outlet a second writer of a cloud row (§50.1).
	AggregateTypePurchaseOrder: SyncDirectionCloudToEdge,
	// The outlet receives goods with the uplink down and the cloud replays.
	AggregateTypeGoodsReceiptNote: SyncDirectionEdgeToCloud,
	// The stock_deduction_gap argument, inbound. PLAIN ENVELOPE OUTBOX, not a
	// ranged stream: a gap is a discrete event a buyer acts on, not a per-sale
	// row arriving all day, so it has no entry_seq and needs none of the 0.5.8
	// contiguity machinery.
	AggregateTypeGrnGap:         SyncDirectionEdgeToCloud,
	AggregateTypePurchaseReturn: SyncDirectionEdgeToCloud,
	// Outbound half only. TRANSFER_IN and goods-in-transit are M8.
	AggregateTypeStockTransferOut: SyncDirectionEdgeToCloud,
}

type SyncEnvelope struct {
	RecordID      string        `json:"record_id"`
	TenantID      string        `json:"tenant_id"`
	OutletID      string        `json:"outlet_id"`
	DeviceID      string        `json:"device_id"`
	AggregateType AggregateType `json:"aggregate_type"`
	Direction     SyncDirection `json:"direction"`
	CreatedAt     time.Time     `json:"created_at"`
	UpdatedAt     time.Time     `json:"updated_at"`
	Version       int           `json:"version"`
	SyncStatus    SyncStatus    `json:"sync_status"`
	Payload       interface{}   `json:"payload"`
}
