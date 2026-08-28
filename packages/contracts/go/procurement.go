// Procurement contracts — added at 0.6.0 (ADR-019, Milestone 5).
// Mirrors src/types/procurement.ts.
//
// AUTHORITY (§50.1):
//
//	Supplier, PurchaseOrder                     CLOUD_TO_EDGE aggregates
//	GoodsReceiptNote, GrnGap, PurchaseReturn,
//	StockTransferOut                            EDGE_TO_CLOUD aggregates
//	SupplierItem, PurchaseOrderLine, GrnLine,
//	PurchaseReturnLine, StockTransferLine       child rows, no direction
//	SupplierInvoice, SupplierCredit             CLOUD-ONLY, not AggregateTypes
//
// There is deliberately NO GrnSequence struct. It is edge-local (SQLite only,
// no PostgreSQL mirror, no AggregateType) and never crosses a boundary, so a
// Go struct would imply a transport it must never have — the same treatment
// InvoiceSequence and StockBalanceSnapshot get: named in a comment, typed
// nowhere.
//
// ---------------------------------------------------------------------------
// A GRN NEVER BLOCKS ON A PURCHASE ORDER.
// ---------------------------------------------------------------------------
//
// PurchaseOrderID, SupplierID and PurchaseOrderLineID are all *string for the
// same reason they are NULLABLE in both stores: goods arrive against a PO that
// never synced, against a PO amended after dispatch, and with no PO at all.
// Each case records a GrnGap and ACCEPTS THE RECEIPT.
//
// A cloud-side NOT NULL here would refuse the replay of a receipt the edge
// correctly accepted — the same outage arriving one hop later and much harder
// to see. Do not tighten these.
//
// Field names match sqlite/0027 and postgres/0028 exactly.
package contracts

// QuantityDimension mirrors Dimension in inventory.go. Declared separately so
// this file's constraints and its schema cannot drift apart silently — the
// same reason RecipeIngredient carries its own QuantityDimension (0.5.2).
type QuantityDimension = Dimension

// ---------------------------------------------------------------------------
// Supplier — AGGREGATE, cloud->edge
// ---------------------------------------------------------------------------

type Supplier struct {
	ID       string `json:"id"`
	OutletID string `json:"outlet_id"`
	Code     string `json:"code"`
	Name     string `json:"name"`
	// Nullable because unregistered suppliers are ordinary in this market. No
	// fallback and no synthesised placeholder, for the reason
	// MenuItem.HsnSac has none.
	Gstin            *string `json:"gstin"`
	Phone            *string `json:"phone"`
	Email            *string `json:"email"`
	Address          *string `json:"address"`
	PaymentTermsDays int     `json:"payment_terms_days"`
	IsActive         bool    `json:"is_active"`
	ConfigVersion    int64   `json:"config_version"`
	CreatedAt        string  `json:"created_at"`
	UpdatedAt        string  `json:"updated_at"`
	SchemaVersion    int     `json:"schema_version"`
}

// SupplierItem is a CHILD ROW of Supplier. Not an aggregate, no direction.
type SupplierItem struct {
	ID              string `json:"id"`
	SupplierID      string `json:"supplier_id"`
	InventoryItemID string `json:"inventory_item_id"`
	// The supplier's own unit, off their delivery note. Free text on purpose:
	// it is their label, not an enum this product owns. The CONVERSION is what
	// must be exact.
	PurchaseUnit string `json:"purchase_unit"`
	// Base-dimension quantity in one PurchaseUnit. One 50 kg sack = 5e10.
	PackSizeMicro int64 `json:"pack_size_micro"`
	// THE UNIT THE AUTHOR CHOSE, NEVER DERIVED FROM THE REFERENT (0.5.2).
	//
	// THE CLOUD IS THE SIDE THAT REJECTS A MISMATCH against
	// InventoryItem.Dimension, at write time. The edge cannot: it degrades to
	// a GrnGapReasonDimensionMismatch and still accepts the receipt.
	//
	// IF A WRITE PATH OR UI AUTO-FILLS THIS FROM THE ITEM, THE COMPARISON
	// BECOMES x == x AND THE REJECTION CAN NEVER FIRE — and it will look
	// correct in review.
	QuantityDimension QuantityDimension `json:"quantity_dimension"`
	// Advisory only: prefills a PO line, never the price a GRN posts. What was
	// invoiced is a fact; what was expected is a guess.
	LastPricePaise *int64 `json:"last_price_paise"`
	IsPreferred    bool   `json:"is_preferred"`
	SchemaVersion  int    `json:"schema_version"`
}

// ---------------------------------------------------------------------------
// PurchaseOrder — AGGREGATE, cloud->edge. THE CLOUD IS THE ONLY WRITER.
// ---------------------------------------------------------------------------

type PurchaseOrderStatus string

// NO RECEIPT STATE. PartiallyReceived and closed-on-receipt are deliberately
// absent: receiving happens at the edge, this is a cloud-owned config row, and
// a receipt-driven status would make the outlet a second writer of a cloud
// aggregate (§50.1, ADR-011).
//
// Receipt progress is DERIVED on both sides, and THE TWO DERIVATIONS
// LEGITIMATELY DIFFER: the edge sees only its own GrnLines, the cloud sees
// every outlet's. A shared PO reads "40 of 100" at one till and "90 of 100" in
// the admin, simultaneously, and both are right. Show both and label them;
// never reconcile them, because reconciling reintroduces the second writer.
const (
	PurchaseOrderStatusDraft           PurchaseOrderStatus = "DRAFT"
	PurchaseOrderStatusPendingApproval PurchaseOrderStatus = "PENDING_APPROVAL"
	PurchaseOrderStatusApproved        PurchaseOrderStatus = "APPROVED"
	PurchaseOrderStatusSent            PurchaseOrderStatus = "SENT"
	PurchaseOrderStatusCancelled       PurchaseOrderStatus = "CANCELLED"
	PurchaseOrderStatusClosed          PurchaseOrderStatus = "CLOSED"
)

// PurchaseOrderLine is a CHILD ROW. Not an aggregate, no direction.
type PurchaseOrderLine struct {
	ID                   string `json:"id"`
	PurchaseOrderID      string `json:"purchase_order_id"`
	InventoryItemID      string `json:"inventory_item_id"`
	LineNumber           int    `json:"line_number"`
	PurchaseUnit         string `json:"purchase_unit"`
	OrderedQuantityMicro int64  `json:"ordered_quantity_micro"`
	// 0.5.2's rule. Never auto-filled from the referent.
	QuantityDimension QuantityDimension `json:"quantity_dimension"`
	UnitPricePaise    int64             `json:"unit_price_paise"`
	LineTotalPaise    int64             `json:"line_total_paise"`
}

type PurchaseOrder struct {
	ID           string              `json:"id"`
	OutletID     string              `json:"outlet_id"`
	SupplierID   string              `json:"supplier_id"`
	PoNumber     string              `json:"po_number"`
	Status       PurchaseOrderStatus `json:"status"`
	ExpectedDate *string             `json:"expected_date"`
	Notes        *string             `json:"notes"`
	TotalPaise   int64               `json:"total_paise"`
	// Both nil or both set — an approval is whole or it did not happen, which
	// is what makes "who authorised this spend" answerable. Enforced by CHECK
	// in both stores, not by convention here.
	ApprovedByUserID *string             `json:"approved_by_user_id"`
	ApprovedAt       *string             `json:"approved_at"`
	CreatedAt        string              `json:"created_at"`
	ConfigVersion    int64               `json:"config_version"`
	Lines            []PurchaseOrderLine `json:"lines"`
	SchemaVersion    int                 `json:"schema_version"`
}

// ---------------------------------------------------------------------------
// GoodsReceiptNote — AGGREGATE, edge->cloud. IMMUTABLE in both stores.
// ---------------------------------------------------------------------------

// GrnLine is a CHILD ROW of GoodsReceiptNote. Not an aggregate, no direction.
type GrnLine struct {
	ID              string `json:"id"`
	GrnID           string `json:"grn_id"`
	InventoryItemID string `json:"inventory_item_id"`
	LineNumber      int    `json:"line_number"`
	// NULLABLE: a line with no matching PO line is received and gapped.
	PurchaseOrderLineID *string `json:"purchase_order_line_id"`

	// BOTH SIDES OF THE CONVERSION ARE STORED, and this is not redundancy.
	// Entered* is what the human typed; BaseQuantityMicro is what the ledger
	// receives. When a receipt turns out to be 1000x wrong, "what did they
	// actually type?" must be answerable from the row, not reconstructed from
	// a pack size that may since have been edited.
	//
	// Receiving is the THIRD quantity-entry path in this product and the one
	// with the worst odds: larger quantities than a stock count, read off a
	// delivery note in the SUPPLIER's units, entered by someone reconciling
	// against a document rather than counting a shelf.
	EnteredPurchaseUnit  string            `json:"entered_purchase_unit"`
	EnteredQuantityMicro int64             `json:"entered_quantity_micro"`
	QuantityDimension    QuantityDimension `json:"quantity_dimension"`
	BaseQuantityMicro    int64             `json:"base_quantity_micro"`
	// The rate actually applied, snapshotted, so this receipt's arithmetic
	// stays reproducible after SupplierItem is edited.
	PackSizeMicroApplied int64 `json:"pack_size_micro_applied"`

	// Cost per BASE unit. The field that finally consumes
	// StockLedgerEntry.UnitCostPaise, which ADR-018 deferred to exactly here.
	UnitCostPaise  int64 `json:"unit_cost_paise"`
	LineTotalPaise int64 `json:"line_total_paise"`

	// MODELLED NOW, ALERTED IN M6. Batch identity is captured at receipt or
	// never — you cannot retrofit which crate a chicken came out of, so unlike
	// most deferred fields these cannot wait for their consumer without losing
	// the data permanently. Both are EXEMPT in
	// scripts/check-contract-field-consumers.mjs with M6 named, and BOTH
	// EXEMPTIONS COME OUT when M6's expiry alerting lands.
	BatchCode  *string `json:"batch_code"`
	ExpiryDate *string `json:"expiry_date"`
}

type GoodsReceiptNote struct {
	ID       string `json:"id"`
	OutletID string `json:"outlet_id"`
	// NULLABLE, AND THIS IS THE POINT. See the file header.
	PurchaseOrderID *string `json:"purchase_order_id"`
	SupplierID      *string `json:"supplier_id"`
	GrnNumber       string  `json:"grn_number"`
	// The supplier's own reference off the delivery note. No format assumed
	// and no uniqueness enforced — it is their number, not ours.
	DeliveryNoteRef  *string `json:"delivery_note_ref"`
	ReceivedAt       string  `json:"received_at"`
	ReceivedByUserID string  `json:"received_by_user_id"`
	// Outlet-local business day from compute_business_date(), NOT the first
	// ten characters of a UTC instant.
	BusinessDate  string    `json:"business_date"`
	Notes         *string   `json:"notes"`
	Lines         []GrnLine `json:"lines"`
	SchemaVersion int       `json:"schema_version"`
}

// ---------------------------------------------------------------------------
// GrnGap — AGGREGATE, edge->cloud. The other half of "never blocks".
// ---------------------------------------------------------------------------
//
// PLAIN ENVELOPE OUTBOX, deliberately not a ranged stream: no EntrySeq, no
// counter, no cursor, no contiguity check. A GrnGap is a discrete event a
// buyer acts on — a handful a week — not a per-sale row arriving all day like
// StockDeductionGap, which is why that one earned the 0.5.8 machinery and this
// one does not.

type GrnGapReason string

const (
	GrnGapReasonNoPurchaseOrder        GrnGapReason = "NO_PURCHASE_ORDER"
	GrnGapReasonPurchaseOrderNotFound  GrnGapReason = "PURCHASE_ORDER_NOT_FOUND"
	GrnGapReasonPoLineNotFound         GrnGapReason = "PO_LINE_NOT_FOUND"
	GrnGapReasonQuantityExceedsOrdered GrnGapReason = "QUANTITY_EXCEEDS_ORDERED"
	GrnGapReasonNoSupplierItem         GrnGapReason = "NO_SUPPLIER_ITEM"
	GrnGapReasonNoUnitConversion       GrnGapReason = "NO_UNIT_CONVERSION"
	GrnGapReasonDimensionMismatch      GrnGapReason = "DIMENSION_MISMATCH"
	GrnGapReasonSupplierNotFound       GrnGapReason = "SUPPLIER_NOT_FOUND"
)

type GrnGap struct {
	ID              string       `json:"id"`
	OutletID        string       `json:"outlet_id"`
	GrnID           string       `json:"grn_id"`
	GrnLineID       *string      `json:"grn_line_id"`
	InventoryItemID *string      `json:"inventory_item_id"`
	Reason          GrnGapReason `json:"reason"`
	// Human-readable, and it IS read by a human: M5 acceptance criterion 3
	// requires the gap visible on the POS, not merely present in a table.
	Detail        *string `json:"detail"`
	OccurredAt    string  `json:"occurred_at"`
	BusinessDate  string  `json:"business_date"`
	SchemaVersion int     `json:"schema_version"`
}

// ---------------------------------------------------------------------------
// PurchaseReturn — AGGREGATE, edge->cloud. IMMUTABLE. Posts RETURN_TO_VENDOR.
// ---------------------------------------------------------------------------

type PurchaseReturnReason string

const (
	PurchaseReturnReasonDamaged      PurchaseReturnReason = "DAMAGED"
	PurchaseReturnReasonExpired      PurchaseReturnReason = "EXPIRED"
	PurchaseReturnReasonWrongItem    PurchaseReturnReason = "WRONG_ITEM"
	PurchaseReturnReasonQuality      PurchaseReturnReason = "QUALITY"
	PurchaseReturnReasonOverDelivery PurchaseReturnReason = "OVER_DELIVERY"
	PurchaseReturnReasonOther        PurchaseReturnReason = "OTHER"
)

type PurchaseReturnLine struct {
	ID                   string            `json:"id"`
	PurchaseReturnID     string            `json:"purchase_return_id"`
	InventoryItemID      string            `json:"inventory_item_id"`
	GrnLineID            *string           `json:"grn_line_id"`
	LineNumber           int               `json:"line_number"`
	EnteredPurchaseUnit  string            `json:"entered_purchase_unit"`
	EnteredQuantityMicro int64             `json:"entered_quantity_micro"`
	QuantityDimension    QuantityDimension `json:"quantity_dimension"`
	BaseQuantityMicro    int64             `json:"base_quantity_micro"`
	UnitCostPaise        int64             `json:"unit_cost_paise"`
}

type PurchaseReturn struct {
	ID               string               `json:"id"`
	OutletID         string               `json:"outlet_id"`
	SupplierID       *string              `json:"supplier_id"`
	GrnID            *string              `json:"grn_id"`
	ReturnNumber     string               `json:"return_number"`
	Reason           PurchaseReturnReason `json:"reason"`
	ReturnedAt       string               `json:"returned_at"`
	ReturnedByUserID string               `json:"returned_by_user_id"`
	BusinessDate     string               `json:"business_date"`
	Notes            *string              `json:"notes"`
	Lines            []PurchaseReturnLine `json:"lines"`
	SchemaVersion    int                  `json:"schema_version"`
}

// ---------------------------------------------------------------------------
// StockTransferOut — AGGREGATE, edge->cloud. OUTBOUND HALF ONLY (M5).
// ---------------------------------------------------------------------------
//
// Posts TRANSFER_OUT at the SOURCE outlet. The destination receipt
// (TRANSFER_IN) and goods-in-transit reconciliation are M8, with multi-outlet:
// a transfer spans two edge databases, which is not something to half-build
// here. DestinationOutletID is recorded now so M8 has its link and no
// migration has to find it later; the cloud reads it in M5 for the transfer
// list, so it is not an unconsumed field.

type StockTransferLine struct {
	ID                 string            `json:"id"`
	StockTransferOutID string            `json:"stock_transfer_out_id"`
	InventoryItemID    string            `json:"inventory_item_id"`
	LineNumber         int               `json:"line_number"`
	BaseQuantityMicro  int64             `json:"base_quantity_micro"`
	QuantityDimension  QuantityDimension `json:"quantity_dimension"`
	UnitCostPaise      int64             `json:"unit_cost_paise"`
}

type StockTransferOut struct {
	ID                  string              `json:"id"`
	OutletID            string              `json:"outlet_id"`
	DestinationOutletID string              `json:"destination_outlet_id"`
	TransferNumber      string              `json:"transfer_number"`
	DispatchedAt        string              `json:"dispatched_at"`
	DispatchedByUserID  string              `json:"dispatched_by_user_id"`
	BusinessDate        string              `json:"business_date"`
	Notes               *string             `json:"notes"`
	Lines               []StockTransferLine `json:"lines"`
	SchemaVersion       int                 `json:"schema_version"`
}

// ---------------------------------------------------------------------------
// SupplierInvoice / SupplierCredit — CLOUD-ONLY, MODELLED NOT ACTED ON (M7)
// ---------------------------------------------------------------------------
//
// Deliberately NOT AggregateTypes (the RefreshToken / DeviceCredential
// precedent). No SQLite mirror: an outlet does not reconcile a supplier ledger
// with the uplink down, and an edge copy would be a second authority over
// money owed.

type SupplierInvoiceStatus string

// M5 writes only SupplierInvoiceStatusReceived. The settlement states exist so
// the shape does not change when M7 lands — the YieldFactorPpm precedent — and
// NOTHING in M5 transitions past Received.
const (
	SupplierInvoiceStatusReceived  SupplierInvoiceStatus = "RECEIVED"
	SupplierInvoiceStatusApproved  SupplierInvoiceStatus = "APPROVED"
	SupplierInvoiceStatusPartPaid  SupplierInvoiceStatus = "PART_PAID"
	SupplierInvoiceStatusPaid      SupplierInvoiceStatus = "PAID"
	SupplierInvoiceStatusDisputed  SupplierInvoiceStatus = "DISPUTED"
	SupplierInvoiceStatusCancelled SupplierInvoiceStatus = "CANCELLED"
)

type SupplierInvoice struct {
	ID                string                `json:"id"`
	TenantID          string                `json:"tenant_id"`
	OutletID          string                `json:"outlet_id"`
	SupplierID        string                `json:"supplier_id"`
	GrnID             *string               `json:"grn_id"`
	SupplierInvoiceNo string                `json:"supplier_invoice_no"`
	InvoiceDate       string                `json:"invoice_date"`
	DueDate           *string               `json:"due_date"`
	SubtotalPaise     int64                 `json:"subtotal_paise"`
	TaxPaise          int64                 `json:"tax_paise"`
	TotalPaise        int64                 `json:"total_paise"`
	Status            SupplierInvoiceStatus `json:"status"`
	CreatedAt         string                `json:"created_at"`
	UpdatedAt         string                `json:"updated_at"`
	SchemaVersion     int                   `json:"schema_version"`
}

type SupplierCredit struct {
	ID               string  `json:"id"`
	TenantID         string  `json:"tenant_id"`
	OutletID         string  `json:"outlet_id"`
	SupplierID       string  `json:"supplier_id"`
	PurchaseReturnID *string `json:"purchase_return_id"`
	CreditNoteNo     string  `json:"credit_note_no"`
	CreditDate       string  `json:"credit_date"`
	AmountPaise      int64   `json:"amount_paise"`
	CreatedAt        string  `json:"created_at"`
	UpdatedAt        string  `json:"updated_at"`
	SchemaVersion    int     `json:"schema_version"`
}
