// Package procurement implements the Milestone 5 procurement bounded context
// (ADR-019, contracts v0.6.0): suppliers and their price lists, the purchase
// order lifecycle with role-scoped approval limits, and the cloud side of
// edge-recorded goods receipts, purchase returns and outbound inter-outlet
// transfers.
//
// Per docs/spec/sync.md §50.1 (ADR-009), restated by ADR-019:
//
//	supplier, purchase_order                     CLOUD_TO_EDGE aggregates
//	goods_receipt_note, grn_gap, purchase_return,
//	stock_transfer_out                           EDGE_TO_CLOUD aggregates
//	supplier_item, purchase_order_line, grn_line,
//	purchase_return_line, stock_transfer_line    child rows, no direction
//	supplier_invoice, supplier_credit            CLOUD-ONLY, not AggregateTypes
//
// This package never touches grn_sequence: it is edge-local (SQLite only, no
// Postgres mirror, no AggregateType — the invoice_sequence precedent) and the
// issued grn_number travels on the receipt while the counter that produced it
// never leaves the outlet.
//
// ---------------------------------------------------------------------------
// A GRN NEVER BLOCKS ON A PURCHASE ORDER, AND NEITHER DOES THIS PACKAGE.
// ---------------------------------------------------------------------------
//
// GoodsReceiptNote.PurchaseOrderID, .SupplierID and GrnLine.PurchaseOrderLineID
// are nullable, no CHECK ties a receipt to an order, and NOTHING IN THE INGEST
// PATH BELOW VALIDATES ONE INTO EXISTENCE. Goods arrive against a PO that never
// synced, against one amended after dispatch, and with no PO at all; the edge
// records a grn_gap and accepts the receipt, and a cloud-side rejection here
// would refuse the replay of a receipt the edge correctly accepted — the same
// outage one hop later and much harder to see (ADR-019 §1).
//
// ---------------------------------------------------------------------------
// purchase_order CARRIES NO RECEIPT STATE.
// ---------------------------------------------------------------------------
//
// Receipt progress is DERIVED at query time from grn_line rows
// (Service.PurchaseOrderReceiptProgress) and never written back. THE CLOUD'S
// FIGURE AND THE EDGE'S LEGITIMATELY DIFFER: the cloud sums every outlet's
// receipts, the edge only its own, so a shared PO reads "40 of 100" at one till
// and "90 of 100" here at the same moment and both are right. Show both, label
// which is which, never reconcile them — reconciling needs one authority, and
// choosing one puts a second writer back on a cloud-owned aggregate (§50.1).
package procurement

import (
	contracts "github.com/holler/contracts"
)

// Wire/domain shapes are the contract types, aliased rather than duplicated
// (CLAUDE.md: import contract types, never hand-roll mirrors).
type (
	Dimension            = contracts.Dimension
	Supplier             = contracts.Supplier
	SupplierItem         = contracts.SupplierItem
	PurchaseOrder        = contracts.PurchaseOrder
	PurchaseOrderLine    = contracts.PurchaseOrderLine
	PurchaseOrderStatus  = contracts.PurchaseOrderStatus
	GoodsReceiptNote     = contracts.GoodsReceiptNote
	GrnLine              = contracts.GrnLine
	GrnGap               = contracts.GrnGap
	GrnGapReason         = contracts.GrnGapReason
	PurchaseReturn       = contracts.PurchaseReturn
	PurchaseReturnLine   = contracts.PurchaseReturnLine
	PurchaseReturnReason = contracts.PurchaseReturnReason
	StockTransferOut     = contracts.StockTransferOut
	StockTransferLine    = contracts.StockTransferLine
	SupplierInvoice      = contracts.SupplierInvoice
	SupplierCredit       = contracts.SupplierCredit
)

const (
	DimensionMass   = contracts.DimensionMass
	DimensionVolume = contracts.DimensionVolume
	DimensionCount  = contracts.DimensionCount

	PurchaseOrderStatusDraft           = contracts.PurchaseOrderStatusDraft
	PurchaseOrderStatusPendingApproval = contracts.PurchaseOrderStatusPendingApproval
	PurchaseOrderStatusApproved        = contracts.PurchaseOrderStatusApproved
	PurchaseOrderStatusSent            = contracts.PurchaseOrderStatusSent
	PurchaseOrderStatusCancelled       = contracts.PurchaseOrderStatusCancelled
	PurchaseOrderStatusClosed          = contracts.PurchaseOrderStatusClosed

	// PermissionManage gates every supplier and purchase-order config write.
	// PermissionApprove is only the FIRST of the two approval gates; the
	// second is role.po_approval_limit_paise, and neither substitutes for the
	// other (ADR-019 §5).
	PermissionManage  = contracts.PermissionProcurementManage
	PermissionApprove = contracts.PermissionProcurementApprove
)

// SupplierInvoiceStatusReceived is the ONLY status any M5 code path writes.
// The settlement states exist in the contract so the column does not change
// shape when M7 lands; nothing here transitions past it (ADR-019 §8).
const SupplierInvoiceStatusReceived = contracts.SupplierInvoiceStatusReceived

// NewSupplierInput is what a caller supplies to POST /procurement/suppliers:
// the supplier plus its whole supplier_item price list in one bundle, per the
// OpenAPI requestBody shape ({supplier, items}). supplier_item is a CHILD ROW
// with no route and no sync direction of its own (the menu_item_variant
// precedent).
type NewSupplierInput struct {
	Supplier Supplier
	Items    []SupplierItem
}

// NewPurchaseOrderInput is what a caller supplies to
// POST /procurement/purchase-orders: the order with its lines inside the
// payload. Create OR amend — the same idempotent-replay shape every other
// config write route in this codebase uses.
//
// APPROVAL FIELDS ARE NOT WRITABLE HERE. ApprovedByUserID/ApprovedAt are set
// only by ApprovePurchaseOrder, together, after both gates pass; a create that
// could set them would be an approval with no ceiling check behind it.
type NewPurchaseOrderInput struct {
	PurchaseOrder PurchaseOrder
}

// ReceiptProgress is the DERIVED, CLOUD-WIDE receipt position of one purchase
// order: ordered against received, per line, summed over EVERY outlet's
// replayed grn_line rows.
//
// Scope is a field on this struct, not a comment, because the edge computes
// the same shape over its own rows only and the two answers legitimately
// differ (ADR-019 §4). A consumer that renders this without saying which one
// it is showing is the defect the field exists to prevent.
type ReceiptProgress struct {
	PurchaseOrderID string `json:"purchase_order_id"`
	// Scope is always ScopeCloudWide from this package. It is stated on the
	// value so a UI cannot show a cloud figure labelled as an outlet one.
	Scope string                `json:"scope"`
	Lines []ReceiptProgressLine `json:"lines"`
}

// ScopeCloudWide labels a figure summed over every outlet's receipts.
const ScopeCloudWide = "CLOUD_WIDE_ALL_OUTLETS"

type ReceiptProgressLine struct {
	PurchaseOrderLineID  string `json:"purchase_order_line_id"`
	InventoryItemID      string `json:"inventory_item_id"`
	OrderedQuantityMicro int64  `json:"ordered_quantity_micro"`
	// ReceivedBaseQuantityMicro sums grn_line.base_quantity_micro — the
	// converted side, because ordered quantities are in the purchase unit the
	// buyer chose and received quantities are reconciled in base units. The
	// edge did the conversion once; nothing here recomputes it.
	ReceivedBaseQuantityMicro int64 `json:"received_base_quantity_micro"`
}

// ConfigBundle is the procurement context's contribution to GET /sync/config:
// suppliers, their price lists, purchase orders and their lines newer than the
// caller's since_version. The edge holds ALL of it read-only.
type ConfigBundle struct {
	Suppliers          []Supplier
	SupplierItems      []SupplierItem
	PurchaseOrders     []PurchaseOrder
	PurchaseOrderLines []PurchaseOrderLine
}

// GrnReplayResult is what the two-aggregate goods-receipt route returns: the
// accepted envelope's aggregate type, so the HTTP layer knows which shape it
// is echoing, alongside the stored row.
type GrnReplayResult struct {
	AggregateType contracts.AggregateType
	Receipt       *GoodsReceiptNote
	Gap           *GrnGap
}
