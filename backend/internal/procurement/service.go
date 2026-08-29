package procurement

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

// Service implements the procurement business commands: the cloud config
// write routes (supplier + its price list, purchase order + its lines, and
// approval), the envelope-wrapped edge->cloud replay routes (goods receipt,
// grn gap, purchase return, outbound transfer), the cloud-only supplier
// accounts shapes, and the GET /sync/config contribution.
type Service struct {
	repo Repository
	now  func() time.Time
}

func NewService(repo Repository) *Service {
	return &Service{repo: repo, now: func() time.Time { return time.Now().UTC() }}
}

func (s *Service) requireOutletInTenant(ctx context.Context, tenantID, outletID string) error {
	if strings.TrimSpace(tenantID) == "" {
		return httpx.ErrUnauthorized
	}
	outletID = strings.TrimSpace(outletID)
	if outletID == "" {
		return fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	ok, err := s.repo.OutletBelongsToTenant(ctx, tenantID, outletID)
	if err != nil {
		return err
	}
	if !ok {
		return httpx.ErrForbidden
	}
	return nil
}

func validDimension(d Dimension) bool {
	switch d {
	case DimensionMass, DimensionVolume, DimensionCount:
		return true
	default:
		return false
	}
}

// requireDimensionMatchesItem is THE 0.5.2 GUARD, on the purchase side
// (ADR-019 §6). It COMPARES the author's chosen dimension against the
// referent's own and rejects a mismatch.
//
// IT NEVER RETURNS A VALUE FOR THE CALLER TO COPY, and no caller may fill the
// field from the item: an auto-filled column makes the comparison x == x, the
// guard can never fire, and it will look correct in review. That is the entire
// failure mode this function exists against.
func (s *Service) requireDimensionMatchesItem(ctx context.Context, itemID string, chosen Dimension, what string) error {
	if !validDimension(chosen) {
		return fmt.Errorf("%w: %s quantity_dimension must be one of MASS, VOLUME, COUNT", httpx.ErrInvalidInput, what)
	}
	itemDim, found, err := s.repo.InventoryItemDimension(ctx, itemID)
	if err != nil {
		return err
	}
	if !found {
		return fmt.Errorf("%w: %s references inventory_item %s which does not exist", httpx.ErrInvalidInput, what, itemID)
	}
	if itemDim != chosen {
		return fmt.Errorf("%w: %s quantity_dimension %q but inventory_item %s is measured in %q",
			ErrDimensionMismatch, what, chosen, itemID, itemDim)
	}
	return nil
}

// --- supplier ---------------------------------------------------------------

// CreateSupplier creates or updates a supplier and REPLACES its whole
// supplier_item price list (config, cloud->edge). Requires procurement.manage,
// enforced by the route middleware in http.go.
func (s *Service) CreateSupplier(ctx context.Context, tenantID string, in NewSupplierInput) (Supplier, []SupplierItem, error) {
	sup := in.Supplier
	if err := s.requireOutletInTenant(ctx, tenantID, sup.OutletID); err != nil {
		return Supplier{}, nil, err
	}
	if err := validateSupplierFields(sup); err != nil {
		return Supplier{}, nil, err
	}
	items, err := s.normaliseSupplierItems(ctx, sup.ID, in.Items)
	if err != nil {
		return Supplier{}, nil, err
	}

	now := s.now().Format(time.RFC3339)
	if strings.TrimSpace(sup.CreatedAt) == "" {
		sup.CreatedAt = now
	}
	sup.UpdatedAt = now
	sup.SchemaVersion = 1

	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, sup.OutletID)
		if err != nil {
			return err
		}
		sup.ConfigVersion = int64(newVersion)
		return s.repo.UpsertSupplier(ctx, tx, sup, items)
	})
	if err != nil {
		return Supplier{}, nil, err
	}
	return sup, items, nil
}

// --- purchase_order ---------------------------------------------------------

// CreatePurchaseOrder raises or amends a purchase order with its lines
// (config, cloud->edge). Requires procurement.manage.
//
// THIS METHOD NEVER SETS AN APPROVAL and never sets receipt state. The
// approval columns are written only by ApprovePurchaseOrder, after both gates
// pass; receipt progress has no column to set (ADR-019 §4).
func (s *Service) CreatePurchaseOrder(ctx context.Context, tenantID string, in NewPurchaseOrderInput) (PurchaseOrder, error) {
	po := in.PurchaseOrder
	if err := s.requireOutletInTenant(ctx, tenantID, po.OutletID); err != nil {
		return PurchaseOrder{}, err
	}
	if strings.TrimSpace(po.ID) == "" {
		return PurchaseOrder{}, fmt.Errorf("%w: id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(po.SupplierID) == "" {
		return PurchaseOrder{}, fmt.Errorf("%w: supplier_id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(po.PoNumber) == "" {
		return PurchaseOrder{}, fmt.Errorf("%w: po_number is required", httpx.ErrInvalidInput)
	}
	if po.TotalPaise < 0 {
		return PurchaseOrder{}, fmt.Errorf("%w: total_paise must not be negative", httpx.ErrInvalidInput)
	}

	// The supplier must live at the same outlet: a PO is outlet-scoped config
	// and a cross-outlet supplier reference would leak a price list across an
	// isolation boundary.
	supplierOutlet, found, err := s.repo.SupplierOutlet(ctx, po.SupplierID)
	if err != nil {
		return PurchaseOrder{}, err
	}
	if !found {
		return PurchaseOrder{}, fmt.Errorf("%w: supplier %s does not exist", httpx.ErrInvalidInput, po.SupplierID)
	}
	if supplierOutlet != po.OutletID {
		return PurchaseOrder{}, fmt.Errorf("%w: supplier %s belongs to a different outlet", httpx.ErrForbidden, po.SupplierID)
	}

	// Only the statuses a HUMAN raise/amend may express. APPROVED/SENT/CLOSED
	// all require an approver on the row (the
	// purchase_order_approved_states_need_an_approver CHECK), and this route
	// cannot grant one.
	switch po.Status {
	case "":
		po.Status = PurchaseOrderStatusDraft
	case PurchaseOrderStatusDraft, PurchaseOrderStatusPendingApproval, PurchaseOrderStatusCancelled:
		// fine
	case PurchaseOrderStatusApproved, PurchaseOrderStatusSent, PurchaseOrderStatusClosed:
		return PurchaseOrder{}, fmt.Errorf("%w: status %q may only be reached through POST /procurement/purchase-orders/{id}/approve",
			httpx.ErrInvalidInput, po.Status)
	default:
		return PurchaseOrder{}, fmt.Errorf("%w: unknown purchase order status %q", httpx.ErrInvalidInput, po.Status)
	}

	lines, err := s.normalisePurchaseOrderLines(ctx, po.ID, po.Lines)
	if err != nil {
		return PurchaseOrder{}, err
	}
	po.Lines = lines

	// Approval fields are cleared off the INPUT rather than trusted: a caller
	// that posts approved_by_user_id must not thereby approve anything. The
	// repository's upsert does not write these columns either — belt and
	// braces, because this is the one field pair on this aggregate that
	// authorises money.
	po.ApprovedByUserID = nil
	po.ApprovedAt = nil

	if strings.TrimSpace(po.CreatedAt) == "" {
		po.CreatedAt = s.now().Format(time.RFC3339)
	}
	po.SchemaVersion = 1

	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, po.OutletID)
		if err != nil {
			return err
		}
		po.ConfigVersion = int64(newVersion)
		return s.repo.UpsertPurchaseOrder(ctx, tx, po)
	})
	if err != nil {
		return PurchaseOrder{}, err
	}
	return po, nil
}

// ApprovePurchaseOrder applies BOTH APPROVAL GATES and, only if both pass,
// writes approved_by_user_id and approved_at TOGETHER with the status
// transition (ADR-019 §5).
//
//	GATE 1: the caller's roles carry procurement.approve.
//	GATE 2: role.po_approval_limit_paise is NON-NULL and >= total_paise.
//
// NULL MEANS "MAY NOT APPROVE ANY AMOUNT". Absence is never read as unlimited
// — a NULL that defaulted to unlimited would turn every unconfigured role into
// an unbounded approver, silently (the printer_role rule).
//
// A refusal is an *ApprovalRefusal carrying the total, the ceiling and the
// roles that could approve instead, because §64 requires the message to say
// what to do next: a buyer with a delivery due needs an action, not the word
// "Forbidden".
func (s *Service) ApprovePurchaseOrder(ctx context.Context, principal auth.AuthenticatedPrincipal, purchaseOrderID string) (PurchaseOrder, error) {
	if strings.TrimSpace(principal.TenantID) == "" || strings.TrimSpace(principal.UserID) == "" {
		return PurchaseOrder{}, httpx.ErrUnauthorized
	}

	po, found, err := s.repo.GetPurchaseOrder(ctx, principal.TenantID, purchaseOrderID)
	if err != nil {
		return PurchaseOrder{}, err
	}
	if !found {
		// Cross-tenant reads and genuinely absent rows are indistinguishable
		// by design: a 403 here would confirm the id exists.
		return PurchaseOrder{}, fmt.Errorf("%w: purchase order %s", httpx.ErrNotFound, purchaseOrderID)
	}

	// GATE 1. Checked in the service rather than only in middleware so the
	// refusal can carry the §64 message; the route ALSO carries the
	// middleware, so the permission is enforced twice and bypassed nowhere.
	if !principalHasPermission(principal, PermissionApprove) {
		alternatives, err := s.repo.RolesAbleToApprove(ctx, principal.TenantID, po.TotalPaise)
		if err != nil {
			return PurchaseOrder{}, err
		}
		return PurchaseOrder{}, &ApprovalRefusal{
			Code:         approvalRefusalCodeNoPermission,
			TotalPaise:   po.TotalPaise,
			LimitPaise:   nil,
			Alternatives: alternatives,
			Reason:       "your role does not carry procurement.approve",
		}
	}

	// GATE 2. The limit is read only from roles that ALSO carry
	// procurement.approve, so one gate cannot be satisfied by the other's
	// configuration.
	limit, err := s.repo.PoApprovalLimitForUser(ctx, principal.TenantID, po.OutletID, principal.UserID)
	if err != nil {
		return PurchaseOrder{}, err
	}
	if limit == nil || *limit < po.TotalPaise {
		alternatives, err := s.repo.RolesAbleToApprove(ctx, principal.TenantID, po.TotalPaise)
		if err != nil {
			return PurchaseOrder{}, err
		}
		reason := "this purchase order exceeds your role's approval limit"
		if limit == nil {
			reason = "your role has no purchase-order approval limit configured"
		}
		return PurchaseOrder{}, &ApprovalRefusal{
			Code:         approvalRefusalCodeOverLimit,
			TotalPaise:   po.TotalPaise,
			LimitPaise:   limit,
			Alternatives: alternatives,
			Reason:       reason,
		}
	}

	if !canApprove(po.Status) {
		return PurchaseOrder{}, fmt.Errorf("%w: purchase order %s is %s", ErrPurchaseOrderNotApprovable, po.ID, po.Status)
	}

	approvedAt := s.now()
	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, po.OutletID)
		if err != nil {
			return err
		}
		po.ConfigVersion = int64(newVersion)
		return s.repo.ApprovePurchaseOrder(ctx, tx, po.ID, principal.UserID, approvedAt, newVersion)
	})
	if err != nil {
		return PurchaseOrder{}, err
	}

	// Written together or not at all — mirrored onto the returned value the
	// same way, so no caller ever sees half an approval.
	approver := principal.UserID
	at := approvedAt.Format(time.RFC3339)
	po.Status = PurchaseOrderStatusApproved
	po.ApprovedByUserID = &approver
	po.ApprovedAt = &at
	return po, nil
}

func principalHasPermission(p auth.AuthenticatedPrincipal, want contracts.Permission) bool {
	for _, granted := range p.Permissions {
		if granted == want {
			return true
		}
	}
	return false
}

// PurchaseOrderReceiptProgress DERIVES receipt progress from grn_line rows at
// query time. Nothing is stored and nothing is written back.
//
// THIS FIGURE IS CLOUD-WIDE and legitimately differs from the same order's
// figure at any single till: the cloud sums every outlet's receipts, an edge
// sums only its own. Both are right for the question each answers. The
// returned value LABELS its own scope so a consumer cannot render one as the
// other, and no code path anywhere may reconcile the two — reconciling needs
// one authority, and choosing one puts a second writer back on this aggregate
// (§50.1, ADR-019 §4).
func (s *Service) PurchaseOrderReceiptProgress(ctx context.Context, tenantID, purchaseOrderID string) (ReceiptProgress, error) {
	po, found, err := s.repo.GetPurchaseOrder(ctx, tenantID, purchaseOrderID)
	if err != nil {
		return ReceiptProgress{}, err
	}
	if !found {
		return ReceiptProgress{}, fmt.Errorf("%w: purchase order %s", httpx.ErrNotFound, purchaseOrderID)
	}
	received, err := s.repo.ReceivedBaseQuantityByPurchaseOrderLine(ctx, po.ID)
	if err != nil {
		return ReceiptProgress{}, err
	}
	lines := make([]ReceiptProgressLine, 0, len(po.Lines))
	for _, l := range po.Lines {
		lines = append(lines, ReceiptProgressLine{
			PurchaseOrderLineID:       l.ID,
			InventoryItemID:           l.InventoryItemID,
			OrderedQuantityMicro:      l.OrderedQuantityMicro,
			ReceivedBaseQuantityMicro: received[l.ID],
		})
	}
	return ReceiptProgress{PurchaseOrderID: po.ID, Scope: ScopeCloudWide, Lines: lines}, nil
}

// GetPurchaseOrder is the tenant-scoped read behind the admin's PO detail
// screen.
func (s *Service) GetPurchaseOrder(ctx context.Context, tenantID, purchaseOrderID string) (PurchaseOrder, error) {
	po, found, err := s.repo.GetPurchaseOrder(ctx, tenantID, purchaseOrderID)
	if err != nil {
		return PurchaseOrder{}, err
	}
	if !found {
		return PurchaseOrder{}, fmt.Errorf("%w: purchase order %s", httpx.ErrNotFound, purchaseOrderID)
	}
	return po, nil
}

// --- goods_receipt_note / grn_gap (EDGE_TO_CLOUD replay) --------------------

// IngestGoodsReceiptNote replays an edge-recorded goods receipt and its lines.
//
// IT DOES NOT VALIDATE THE PURCHASE ORDER, THE PO LINE OR THE SUPPLIER, AND
// MUST NOT. A receipt whose purchase_order_id is null, or names an order this
// cloud has never seen, is stored exactly as received: the edge already
// recorded a grn_gap for it and accepted the goods, and a cloud-side rejection
// here would refuse a receipt that was correctly taken — the same outage one
// hop later and much harder to see (ADR-019 §1).
//
// IT ALSO RECOMPUTES NOTHING. entered_*, base_quantity_micro and
// pack_size_micro_applied are stored as sent; the conversion happened once, at
// the edge, and recomputing against a since-edited supplier_item would
// silently restate history (ADR-019 §3).
func (s *Service) IngestGoodsReceiptNote(ctx context.Context, callerTenantID string, env contracts.SyncEnvelope, grn GoodsReceiptNote) (GoodsReceiptNote, error) {
	if err := requireEnvelope(env, contracts.AggregateTypeGoodsReceiptNote); err != nil {
		return GoodsReceiptNote{}, err
	}
	if err := requireTenantMatch(callerTenantID, env); err != nil {
		return GoodsReceiptNote{}, err
	}
	if strings.TrimSpace(grn.ID) == "" {
		return GoodsReceiptNote{}, fmt.Errorf("%w: id is required", httpx.ErrInvalidInput)
	}
	if grn.ID != env.RecordID {
		return GoodsReceiptNote{}, fmt.Errorf("%w: payload id must match envelope record_id", httpx.ErrInvalidInput)
	}
	if grn.OutletID != env.OutletID {
		return GoodsReceiptNote{}, fmt.Errorf("%w: payload outlet_id must match envelope outlet_id", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(grn.GrnNumber) == "" {
		return GoodsReceiptNote{}, fmt.Errorf("%w: grn_number is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(grn.ReceivedByUserID) == "" {
		return GoodsReceiptNote{}, fmt.Errorf("%w: received_by_user_id is required", httpx.ErrInvalidInput)
	}

	// Idempotent and silent: the same id arriving twice is an ordinary retry
	// (a dropped ack, a resumed batch), not a fault. The row is IMMUTABLE in
	// both stores, so this path never issues an UPDATE either way.
	if existing, found, err := s.repo.GetGoodsReceiptNoteByID(ctx, grn.ID); err != nil {
		return GoodsReceiptNote{}, err
	} else if found {
		lines, err := s.repo.GrnLines(ctx, existing.ID)
		if err != nil {
			return GoodsReceiptNote{}, err
		}
		existing.Lines = lines
		return existing, nil
	}

	lines := grn.Lines
	if lines == nil {
		lines = []GrnLine{}
	}
	for i := range lines {
		l := lines[i]
		if strings.TrimSpace(l.ID) == "" {
			return GoodsReceiptNote{}, fmt.Errorf("%w: grn_line id is required", httpx.ErrInvalidInput)
		}
		if l.EnteredQuantityMicro <= 0 || l.BaseQuantityMicro <= 0 || l.PackSizeMicroApplied <= 0 {
			return GoodsReceiptNote{}, fmt.Errorf("%w: grn_line quantities must be positive", httpx.ErrInvalidInput)
		}
		if l.UnitCostPaise < 0 || l.LineTotalPaise < 0 {
			return GoodsReceiptNote{}, fmt.Errorf("%w: grn_line costs must not be negative", httpx.ErrInvalidInput)
		}
		if !validDimension(l.QuantityDimension) {
			return GoodsReceiptNote{}, fmt.Errorf("%w: grn_line quantity_dimension must be one of MASS, VOLUME, COUNT", httpx.ErrInvalidInput)
		}
		// NOTE what is NOT checked here: purchase_order_line_id. It is a
		// nullable link to a row this cloud may not have, and the receipt is
		// stored either way.
		lines[i].GrnID = grn.ID
	}
	grn.Lines = lines
	grn.SchemaVersion = 1

	err := s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		return s.repo.InsertGoodsReceiptNote(ctx, tx, env.TenantID, grn, lines)
	})
	if err != nil {
		return GoodsReceiptNote{}, err
	}
	return grn, nil
}

// IngestGrnGap replays the record of what could not be matched about a
// receipt. It shares the receipt's route because a gap belongs beside the
// receipt it explains — a gap arriving by another path could not be joined to
// it (ADR-019 §9, the /inventory/ledger-entries precedent).
//
// PLAIN OUTBOX: no entry_seq, no cursor, no contiguity check, deliberately.
func (s *Service) IngestGrnGap(ctx context.Context, callerTenantID string, env contracts.SyncEnvelope, gap GrnGap) (GrnGap, error) {
	if err := requireEnvelope(env, contracts.AggregateTypeGrnGap); err != nil {
		return GrnGap{}, err
	}
	if err := requireTenantMatch(callerTenantID, env); err != nil {
		return GrnGap{}, err
	}
	if strings.TrimSpace(gap.ID) == "" {
		return GrnGap{}, fmt.Errorf("%w: id is required", httpx.ErrInvalidInput)
	}
	if gap.ID != env.RecordID {
		return GrnGap{}, fmt.Errorf("%w: payload id must match envelope record_id", httpx.ErrInvalidInput)
	}
	if gap.OutletID != env.OutletID {
		return GrnGap{}, fmt.Errorf("%w: payload outlet_id must match envelope outlet_id", httpx.ErrInvalidInput)
	}
	if !validGrnGapReason(gap.Reason) {
		return GrnGap{}, fmt.Errorf("%w: unknown grn_gap reason %q", httpx.ErrInvalidInput, gap.Reason)
	}

	if existing, found, err := s.repo.GetGrnGapByID(ctx, gap.ID); err != nil {
		return GrnGap{}, err
	} else if found {
		return existing, nil
	}

	gap.SchemaVersion = 1
	if err := s.repo.InsertGrnGap(ctx, env.TenantID, gap); err != nil {
		return GrnGap{}, err
	}
	return gap, nil
}

func validGrnGapReason(r GrnGapReason) bool {
	switch r {
	case contracts.GrnGapReasonNoPurchaseOrder,
		contracts.GrnGapReasonPurchaseOrderNotFound,
		contracts.GrnGapReasonPoLineNotFound,
		contracts.GrnGapReasonQuantityExceedsOrdered,
		contracts.GrnGapReasonNoSupplierItem,
		contracts.GrnGapReasonNoUnitConversion,
		contracts.GrnGapReasonDimensionMismatch,
		contracts.GrnGapReasonSupplierNotFound:
		return true
	default:
		return false
	}
}

// --- purchase_return --------------------------------------------------------

// IngestPurchaseReturn replays an edge-recorded purchase return and its lines.
// The RETURN_TO_VENDOR ledger entries were posted at the edge; the cloud
// replays and computes nothing.
func (s *Service) IngestPurchaseReturn(ctx context.Context, callerTenantID string, env contracts.SyncEnvelope, ret PurchaseReturn) (PurchaseReturn, error) {
	if err := requireEnvelope(env, contracts.AggregateTypePurchaseReturn); err != nil {
		return PurchaseReturn{}, err
	}
	if err := requireTenantMatch(callerTenantID, env); err != nil {
		return PurchaseReturn{}, err
	}
	if strings.TrimSpace(ret.ID) == "" {
		return PurchaseReturn{}, fmt.Errorf("%w: id is required", httpx.ErrInvalidInput)
	}
	if ret.ID != env.RecordID {
		return PurchaseReturn{}, fmt.Errorf("%w: payload id must match envelope record_id", httpx.ErrInvalidInput)
	}
	if ret.OutletID != env.OutletID {
		return PurchaseReturn{}, fmt.Errorf("%w: payload outlet_id must match envelope outlet_id", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(ret.ReturnNumber) == "" {
		return PurchaseReturn{}, fmt.Errorf("%w: return_number is required", httpx.ErrInvalidInput)
	}
	if !validPurchaseReturnReason(ret.Reason) {
		return PurchaseReturn{}, fmt.Errorf("%w: unknown purchase_return reason %q", httpx.ErrInvalidInput, ret.Reason)
	}

	if existing, found, err := s.repo.GetPurchaseReturnByID(ctx, ret.ID); err != nil {
		return PurchaseReturn{}, err
	} else if found {
		lines, err := s.repo.PurchaseReturnLines(ctx, existing.ID)
		if err != nil {
			return PurchaseReturn{}, err
		}
		existing.Lines = lines
		return existing, nil
	}

	lines := ret.Lines
	if lines == nil {
		lines = []PurchaseReturnLine{}
	}
	for i := range lines {
		l := lines[i]
		if strings.TrimSpace(l.ID) == "" {
			return PurchaseReturn{}, fmt.Errorf("%w: purchase_return_line id is required", httpx.ErrInvalidInput)
		}
		if l.EnteredQuantityMicro <= 0 || l.BaseQuantityMicro <= 0 {
			return PurchaseReturn{}, fmt.Errorf("%w: purchase_return_line quantities must be positive", httpx.ErrInvalidInput)
		}
		if l.UnitCostPaise < 0 {
			return PurchaseReturn{}, fmt.Errorf("%w: purchase_return_line unit_cost_paise must not be negative", httpx.ErrInvalidInput)
		}
		if !validDimension(l.QuantityDimension) {
			return PurchaseReturn{}, fmt.Errorf("%w: purchase_return_line quantity_dimension must be one of MASS, VOLUME, COUNT", httpx.ErrInvalidInput)
		}
		lines[i].PurchaseReturnID = ret.ID
	}
	ret.Lines = lines
	ret.SchemaVersion = 1

	err := s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		return s.repo.InsertPurchaseReturn(ctx, tx, env.TenantID, ret, lines)
	})
	if err != nil {
		return PurchaseReturn{}, err
	}
	return ret, nil
}

func validPurchaseReturnReason(r PurchaseReturnReason) bool {
	switch r {
	case contracts.PurchaseReturnReasonDamaged,
		contracts.PurchaseReturnReasonExpired,
		contracts.PurchaseReturnReasonWrongItem,
		contracts.PurchaseReturnReasonQuality,
		contracts.PurchaseReturnReasonOverDelivery,
		contracts.PurchaseReturnReasonOther:
		return true
	default:
		return false
	}
}

// --- stock_transfer_out -----------------------------------------------------

// IngestStockTransferOut replays an outbound inter-outlet dispatch. OUTBOUND
// HALF ONLY: TRANSFER_IN, the destination receipt and goods-in-transit are M8,
// and nothing here creates a matching inbound row.
func (s *Service) IngestStockTransferOut(ctx context.Context, callerTenantID string, env contracts.SyncEnvelope, transfer StockTransferOut) (StockTransferOut, error) {
	if err := requireEnvelope(env, contracts.AggregateTypeStockTransferOut); err != nil {
		return StockTransferOut{}, err
	}
	if err := requireTenantMatch(callerTenantID, env); err != nil {
		return StockTransferOut{}, err
	}
	if strings.TrimSpace(transfer.ID) == "" {
		return StockTransferOut{}, fmt.Errorf("%w: id is required", httpx.ErrInvalidInput)
	}
	if transfer.ID != env.RecordID {
		return StockTransferOut{}, fmt.Errorf("%w: payload id must match envelope record_id", httpx.ErrInvalidInput)
	}
	if transfer.OutletID != env.OutletID {
		return StockTransferOut{}, fmt.Errorf("%w: payload outlet_id must match envelope outlet_id", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(transfer.TransferNumber) == "" {
		return StockTransferOut{}, fmt.Errorf("%w: transfer_number is required", httpx.ErrInvalidInput)
	}
	if transfer.DestinationOutletID == transfer.OutletID {
		return StockTransferOut{}, fmt.Errorf("%w: destination_outlet_id must differ from the source outlet", httpx.ErrInvalidInput)
	}
	// The DESTINATION must be inside the caller's tenant. This is the one
	// cross-outlet reference in the package and the one place a stray id could
	// name another tenant's outlet.
	if err := s.requireOutletInTenant(ctx, callerTenantID, transfer.DestinationOutletID); err != nil {
		return StockTransferOut{}, err
	}

	if existing, found, err := s.repo.GetStockTransferOutByID(ctx, transfer.ID); err != nil {
		return StockTransferOut{}, err
	} else if found {
		lines, err := s.repo.StockTransferLines(ctx, existing.ID)
		if err != nil {
			return StockTransferOut{}, err
		}
		existing.Lines = lines
		return existing, nil
	}

	lines := transfer.Lines
	if lines == nil {
		lines = []StockTransferLine{}
	}
	for i := range lines {
		l := lines[i]
		if strings.TrimSpace(l.ID) == "" {
			return StockTransferOut{}, fmt.Errorf("%w: stock_transfer_line id is required", httpx.ErrInvalidInput)
		}
		if l.BaseQuantityMicro <= 0 {
			return StockTransferOut{}, fmt.Errorf("%w: stock_transfer_line base_quantity_micro must be positive", httpx.ErrInvalidInput)
		}
		if l.UnitCostPaise < 0 {
			return StockTransferOut{}, fmt.Errorf("%w: stock_transfer_line unit_cost_paise must not be negative", httpx.ErrInvalidInput)
		}
		if !validDimension(l.QuantityDimension) {
			return StockTransferOut{}, fmt.Errorf("%w: stock_transfer_line quantity_dimension must be one of MASS, VOLUME, COUNT", httpx.ErrInvalidInput)
		}
		lines[i].StockTransferOutID = transfer.ID
	}
	transfer.Lines = lines
	transfer.SchemaVersion = 1

	err := s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		return s.repo.InsertStockTransferOut(ctx, tx, env.TenantID, transfer, lines)
	})
	if err != nil {
		return StockTransferOut{}, err
	}
	return transfer, nil
}

// --- supplier_invoice / supplier_credit (CLOUD-ONLY) -------------------------

// CreateSupplierInvoice records a supplier's invoice against an outlet.
//
// M5 IS CREATE AND LIST ONLY. status accepts RECEIVED and nothing else; there
// is no transition method in this package, deliberately, because posting,
// credit application and settlement are M7 (ADR-019 §8). The settlement states
// exist in the contract so the column does not change shape later — they are
// not a hint that this milestone may write them.
func (s *Service) CreateSupplierInvoice(ctx context.Context, tenantID string, inv SupplierInvoice) (SupplierInvoice, error) {
	if err := s.requireOutletInTenant(ctx, tenantID, inv.OutletID); err != nil {
		return SupplierInvoice{}, err
	}
	if strings.TrimSpace(inv.ID) == "" {
		return SupplierInvoice{}, fmt.Errorf("%w: id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(inv.SupplierInvoiceNo) == "" {
		return SupplierInvoice{}, fmt.Errorf("%w: supplier_invoice_no is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(inv.InvoiceDate) == "" {
		return SupplierInvoice{}, fmt.Errorf("%w: invoice_date is required", httpx.ErrInvalidInput)
	}
	if inv.SubtotalPaise < 0 || inv.TaxPaise < 0 || inv.TotalPaise < 0 {
		return SupplierInvoice{}, fmt.Errorf("%w: supplier_invoice amounts must not be negative", httpx.ErrInvalidInput)
	}
	supplierOutlet, found, err := s.repo.SupplierOutlet(ctx, inv.SupplierID)
	if err != nil {
		return SupplierInvoice{}, err
	}
	if !found || supplierOutlet != inv.OutletID {
		return SupplierInvoice{}, fmt.Errorf("%w: supplier %s does not belong to outlet %s", httpx.ErrInvalidInput, inv.SupplierID, inv.OutletID)
	}
	if inv.Status != "" && inv.Status != SupplierInvoiceStatusReceived {
		return SupplierInvoice{}, fmt.Errorf("%w: supplier_invoice status %q is an M7 settlement state; M5 writes RECEIVED only",
			httpx.ErrInvalidInput, inv.Status)
	}
	inv.Status = SupplierInvoiceStatusReceived
	inv.TenantID = tenantID
	now := s.now().Format(time.RFC3339)
	inv.CreatedAt, inv.UpdatedAt = now, now
	inv.SchemaVersion = 1
	if err := s.repo.InsertSupplierInvoice(ctx, inv); err != nil {
		return SupplierInvoice{}, err
	}
	return inv, nil
}

func (s *Service) ListSupplierInvoices(ctx context.Context, tenantID, outletID string) ([]SupplierInvoice, error) {
	if err := s.requireOutletInTenant(ctx, tenantID, outletID); err != nil {
		return nil, err
	}
	return s.repo.ListSupplierInvoices(ctx, tenantID, outletID)
}

// CreateSupplierCredit records a credit note. Like the invoice above: recorded
// in M5, APPLIED in M7. Nothing here reduces a balance or settles anything.
func (s *Service) CreateSupplierCredit(ctx context.Context, tenantID string, c SupplierCredit) (SupplierCredit, error) {
	if err := s.requireOutletInTenant(ctx, tenantID, c.OutletID); err != nil {
		return SupplierCredit{}, err
	}
	if strings.TrimSpace(c.ID) == "" {
		return SupplierCredit{}, fmt.Errorf("%w: id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(c.CreditNoteNo) == "" {
		return SupplierCredit{}, fmt.Errorf("%w: credit_note_no is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(c.CreditDate) == "" {
		return SupplierCredit{}, fmt.Errorf("%w: credit_date is required", httpx.ErrInvalidInput)
	}
	if c.AmountPaise < 0 {
		return SupplierCredit{}, fmt.Errorf("%w: amount_paise must not be negative", httpx.ErrInvalidInput)
	}
	supplierOutlet, found, err := s.repo.SupplierOutlet(ctx, c.SupplierID)
	if err != nil {
		return SupplierCredit{}, err
	}
	if !found || supplierOutlet != c.OutletID {
		return SupplierCredit{}, fmt.Errorf("%w: supplier %s does not belong to outlet %s", httpx.ErrInvalidInput, c.SupplierID, c.OutletID)
	}
	c.TenantID = tenantID
	now := s.now().Format(time.RFC3339)
	c.CreatedAt, c.UpdatedAt = now, now
	c.SchemaVersion = 1
	if err := s.repo.InsertSupplierCredit(ctx, c); err != nil {
		return SupplierCredit{}, err
	}
	return c, nil
}

func (s *Service) ListSupplierCredits(ctx context.Context, tenantID, outletID string) ([]SupplierCredit, error) {
	if err := s.requireOutletInTenant(ctx, tenantID, outletID); err != nil {
		return nil, err
	}
	return s.repo.ListSupplierCredits(ctx, tenantID, outletID)
}

// --- Sync config bundle -----------------------------------------------------

// SyncConfigBundle returns the procurement context's contribution to
// GET /sync/config: suppliers, their price lists, purchase orders and their
// lines newer than sinceVersion, pre-scoped to the caller's tenant. Mirrors
// inventory.Service.SyncConfigBundle exactly, which is what puts the new
// config tables under the same since_version filter every other config table
// already has.
func (s *Service) SyncConfigBundle(ctx context.Context, tenantID, outletID string, sinceVersion int) (ConfigBundle, error) {
	if err := s.requireOutletInTenant(ctx, tenantID, outletID); err != nil {
		return ConfigBundle{}, err
	}
	suppliers, err := s.repo.SuppliersSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	items, err := s.repo.SupplierItemsSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	orders, err := s.repo.PurchaseOrdersSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	lines, err := s.repo.PurchaseOrderLinesSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	if suppliers == nil {
		suppliers = []Supplier{}
	}
	if items == nil {
		items = []SupplierItem{}
	}
	if orders == nil {
		orders = []PurchaseOrder{}
	}
	if lines == nil {
		lines = []PurchaseOrderLine{}
	}
	return ConfigBundle{
		Suppliers:          suppliers,
		SupplierItems:      items,
		PurchaseOrders:     orders,
		PurchaseOrderLines: lines,
	}, nil
}

// --- shared write-path validation -------------------------------------------

// validateSupplierFields is the field-level half of a supplier write, shared
// by CreateSupplier and UpdateSupplier so the two paths cannot drift. Outlet
// tenancy is NOT checked here: that is the caller's first act, before any
// field is looked at.
func validateSupplierFields(sup Supplier) error {
	if strings.TrimSpace(sup.ID) == "" {
		return fmt.Errorf("%w: id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(sup.Code) == "" {
		return fmt.Errorf("%w: code is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(sup.Name) == "" {
		return fmt.Errorf("%w: name is required", httpx.ErrInvalidInput)
	}
	if sup.PaymentTermsDays < 0 {
		return fmt.Errorf("%w: payment_terms_days must not be negative", httpx.ErrInvalidInput)
	}
	return nil
}

// normaliseSupplierItems validates a whole price list and stamps the parent
// id onto each row.
//
// IT SETS SupplierID AND SchemaVersion AND NOTHING ELSE. It emphatically does
// NOT fill QuantityDimension: that is the unit the author chose, and filling
// it from the referenced inventory_item would make
// requireDimensionMatchesItem's comparison x == x, so the guard could never
// fire and would look correct in review (ADR-019 §6, contracts 0.5.2).
func (s *Service) normaliseSupplierItems(ctx context.Context, supplierID string, in []SupplierItem) ([]SupplierItem, error) {
	items := make([]SupplierItem, 0, len(in))
	for _, it := range in {
		if strings.TrimSpace(it.ID) == "" {
			return nil, fmt.Errorf("%w: supplier_item id is required", httpx.ErrInvalidInput)
		}
		if strings.TrimSpace(it.PurchaseUnit) == "" {
			return nil, fmt.Errorf("%w: supplier_item purchase_unit is required", httpx.ErrInvalidInput)
		}
		if it.PackSizeMicro <= 0 {
			return nil, fmt.Errorf("%w: supplier_item pack_size_micro must be positive", httpx.ErrInvalidInput)
		}
		if it.LastPricePaise != nil && *it.LastPricePaise < 0 {
			return nil, fmt.Errorf("%w: supplier_item last_price_paise must not be negative", httpx.ErrInvalidInput)
		}
		if err := s.requireDimensionMatchesItem(ctx, it.InventoryItemID, it.QuantityDimension, "supplier_item"); err != nil {
			return nil, err
		}
		it.SupplierID = supplierID
		it.SchemaVersion = 1
		items = append(items, it)
	}
	return items, nil
}

// normalisePurchaseOrderLines is the line-level half of a purchase order
// write, shared by CreatePurchaseOrder and AmendPurchaseOrder.
//
// Same rule as normaliseSupplierItems: it stamps the parent id and never
// touches QuantityDimension.
func (s *Service) normalisePurchaseOrderLines(ctx context.Context, purchaseOrderID string, in []PurchaseOrderLine) ([]PurchaseOrderLine, error) {
	lines := make([]PurchaseOrderLine, 0, len(in))
	seenLineNumbers := map[int]bool{}
	for _, l := range in {
		if strings.TrimSpace(l.ID) == "" {
			return nil, fmt.Errorf("%w: purchase_order_line id is required", httpx.ErrInvalidInput)
		}
		if l.OrderedQuantityMicro <= 0 {
			return nil, fmt.Errorf("%w: purchase_order_line ordered_quantity_micro must be positive", httpx.ErrInvalidInput)
		}
		if l.UnitPricePaise < 0 || l.LineTotalPaise < 0 {
			return nil, fmt.Errorf("%w: purchase_order_line prices must not be negative", httpx.ErrInvalidInput)
		}
		if strings.TrimSpace(l.PurchaseUnit) == "" {
			return nil, fmt.Errorf("%w: purchase_order_line purchase_unit is required", httpx.ErrInvalidInput)
		}
		if seenLineNumbers[l.LineNumber] {
			return nil, fmt.Errorf("%w: purchase_order_line line_number %d is duplicated", httpx.ErrInvalidInput, l.LineNumber)
		}
		seenLineNumbers[l.LineNumber] = true
		if err := s.requireDimensionMatchesItem(ctx, l.InventoryItemID, l.QuantityDimension, "purchase_order_line"); err != nil {
			return nil, err
		}
		l.PurchaseOrderID = purchaseOrderID
		lines = append(lines, l)
	}
	return lines, nil
}

// --- supplier read + update -------------------------------------------------

// ListSuppliers returns the tenant's suppliers with their whole price lists
// attached, which is what lets a caller prefill a purchase order line from
// SupplierItem.LastPricePaise without a second round trip.
//
// Requires procurement.manage (route middleware). Tenancy is applied inside
// the query, and an explicit outlet_id is checked against the tenant BEFORE
// the list is read, so an outlet id from another tenant is a 403 rather than
// an empty list — an empty list reads as "this outlet has no suppliers" and
// hides the isolation failure.
func (s *Service) ListSuppliers(ctx context.Context, tenantID string, filter SupplierFilter) ([]SupplierWithItems, error) {
	if strings.TrimSpace(tenantID) == "" {
		return nil, httpx.ErrUnauthorized
	}
	if strings.TrimSpace(filter.OutletID) != "" {
		if err := s.requireOutletInTenant(ctx, tenantID, filter.OutletID); err != nil {
			return nil, err
		}
	}
	suppliers, err := s.repo.ListSuppliers(ctx, tenantID, filter)
	if err != nil {
		return nil, err
	}
	ids := make([]string, 0, len(suppliers))
	for _, sup := range suppliers {
		ids = append(ids, sup.ID)
	}
	itemsBySupplier, err := s.repo.SupplierItemsForSuppliers(ctx, ids)
	if err != nil {
		return nil, err
	}
	out := make([]SupplierWithItems, 0, len(suppliers))
	for _, sup := range suppliers {
		items := itemsBySupplier[sup.ID]
		if items == nil {
			items = []SupplierItem{}
		}
		out = append(out, SupplierWithItems{Supplier: sup, Items: items})
	}
	return out, nil
}

// UpdateSupplier edits an EXISTING supplier and REPLACES its whole price list.
// Requires procurement.manage. A supplier that does not exist in this tenant
// is httpx.ErrNotFound — this route never creates one, because a typo'd id
// that silently minted a second supplier is how a price list ends up split
// across two rows nobody can reconcile.
//
// A SUPPLIER MAY NOT CHANGE OUTLET. purchase_order, goods_receipt_note and
// supplier_invoice rows already point at it from the outlet it was created in;
// moving it would leave those rows referencing a supplier that is, from their
// outlet's point of view, no longer there. Retire it and create a new one.
//
// created_at is taken from the STORED row, never from the request: a caller
// re-posting a whole supplier object must not be able to rewrite when it came
// into existence.
func (s *Service) UpdateSupplier(ctx context.Context, tenantID, supplierID string, in NewSupplierInput) (Supplier, []SupplierItem, error) {
	if strings.TrimSpace(tenantID) == "" {
		return Supplier{}, nil, httpx.ErrUnauthorized
	}
	supplierID = strings.TrimSpace(supplierID)
	if supplierID == "" {
		return Supplier{}, nil, fmt.Errorf("%w: supplier id is required", httpx.ErrInvalidInput)
	}
	if id := strings.TrimSpace(in.Supplier.ID); id != "" && id != supplierID {
		return Supplier{}, nil, fmt.Errorf("%w: body id %q does not match path id %q", httpx.ErrInvalidInput, id, supplierID)
	}

	existing, found, err := s.repo.GetSupplier(ctx, tenantID, supplierID)
	if err != nil {
		return Supplier{}, nil, err
	}
	if !found {
		// Indistinguishable from another tenant's supplier, deliberately: a
		// 403 here would confirm the id exists.
		return Supplier{}, nil, fmt.Errorf("%w: supplier %s", httpx.ErrNotFound, supplierID)
	}

	sup := in.Supplier
	sup.ID = supplierID
	if strings.TrimSpace(sup.OutletID) == "" {
		sup.OutletID = existing.OutletID
	}
	if sup.OutletID != existing.OutletID {
		return Supplier{}, nil, fmt.Errorf("%w: a supplier may not move between outlets; retire supplier %s and create a new one at outlet %s",
			httpx.ErrInvalidInput, supplierID, sup.OutletID)
	}
	if err := validateSupplierFields(sup); err != nil {
		return Supplier{}, nil, err
	}
	items, err := s.normaliseSupplierItems(ctx, sup.ID, in.Items)
	if err != nil {
		return Supplier{}, nil, err
	}

	sup.CreatedAt = existing.CreatedAt
	sup.UpdatedAt = s.now().Format(time.RFC3339)
	sup.SchemaVersion = 1

	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, sup.OutletID)
		if err != nil {
			return err
		}
		sup.ConfigVersion = int64(newVersion)
		return s.repo.UpsertSupplier(ctx, tx, sup, items)
	})
	if err != nil {
		return Supplier{}, nil, err
	}
	return sup, items, nil
}

// --- purchase order read + amend --------------------------------------------

// ListPurchaseOrders is the buyer's list, filtered by outlet, supplier and
// status. Requires procurement.manage.
//
// IT RETURNS NO RECEIPT PROGRESS. See the note in domain.go: progress is
// cloud-wide, differs legitimately from any till's own figure, and is only
// ever returned LABELLED, from the detail route.
func (s *Service) ListPurchaseOrders(ctx context.Context, tenantID string, filter PurchaseOrderFilter) ([]PurchaseOrder, error) {
	if strings.TrimSpace(tenantID) == "" {
		return nil, httpx.ErrUnauthorized
	}
	if strings.TrimSpace(filter.OutletID) != "" {
		if err := s.requireOutletInTenant(ctx, tenantID, filter.OutletID); err != nil {
			return nil, err
		}
	}
	for _, st := range filter.Statuses {
		if !validPurchaseOrderStatus(st) {
			return nil, fmt.Errorf("%w: unknown purchase order status %q", httpx.ErrInvalidInput, st)
		}
	}
	orders, err := s.repo.ListPurchaseOrders(ctx, tenantID, filter)
	if err != nil {
		return nil, err
	}
	ids := make([]string, 0, len(orders))
	for _, po := range orders {
		ids = append(ids, po.ID)
	}
	linesByOrder, err := s.repo.PurchaseOrderLinesForOrders(ctx, ids)
	if err != nil {
		return nil, err
	}
	for i := range orders {
		lines := linesByOrder[orders[i].ID]
		if lines == nil {
			lines = []PurchaseOrderLine{}
		}
		orders[i].Lines = lines
	}
	return orders, nil
}

func validPurchaseOrderStatus(st PurchaseOrderStatus) bool {
	switch st {
	case PurchaseOrderStatusDraft, PurchaseOrderStatusPendingApproval, PurchaseOrderStatusApproved,
		PurchaseOrderStatusSent, PurchaseOrderStatusCancelled, PurchaseOrderStatusClosed:
		return true
	default:
		return false
	}
}

// AmendPurchaseOrder edits an EXISTING order's lines, notes, expected date and
// total. Requires procurement.manage.
//
// ---------------------------------------------------------------------------
// AMENDING A PURCHASE ORDER REVOKES ITS APPROVAL. ALWAYS. NO EXCEPTIONS.
// ---------------------------------------------------------------------------
//
// approved_by_user_id and approved_at go to NULL and the status returns to
// PENDING_APPROVAL (or stays DRAFT if it never left). The order must be
// approved again, and that second approval is checked against the NEW
// total_paise by both gates.
//
// The alternative was considered and rejected: if an approved order could be
// amended without re-approval, role.po_approval_limit_paise would be
// bypassable by anyone holding procurement.manage — raise ₹5,000, have it
// approved by someone whose ceiling is ₹10,000, then amend it to ₹5,00,000.
// The approval would still read as granted, by name and timestamp, and no gate
// would ever see the new number. An approval is an approval OF CONTENTS; change
// the contents and it no longer refers to anything.
//
// TERMINAL ORDERS ARE NOT AMENDABLE. CANCELLED and CLOSED are 409: reviving one
// by editing it would erase the fact that it ended.
func (s *Service) AmendPurchaseOrder(ctx context.Context, tenantID, purchaseOrderID string, in NewPurchaseOrderInput) (PurchaseOrder, error) {
	if strings.TrimSpace(tenantID) == "" {
		return PurchaseOrder{}, httpx.ErrUnauthorized
	}
	purchaseOrderID = strings.TrimSpace(purchaseOrderID)
	if purchaseOrderID == "" {
		return PurchaseOrder{}, fmt.Errorf("%w: purchase order id is required", httpx.ErrInvalidInput)
	}
	if id := strings.TrimSpace(in.PurchaseOrder.ID); id != "" && id != purchaseOrderID {
		return PurchaseOrder{}, fmt.Errorf("%w: body id %q does not match path id %q", httpx.ErrInvalidInput, id, purchaseOrderID)
	}

	existing, found, err := s.repo.GetPurchaseOrder(ctx, tenantID, purchaseOrderID)
	if err != nil {
		return PurchaseOrder{}, err
	}
	if !found {
		return PurchaseOrder{}, fmt.Errorf("%w: purchase order %s", httpx.ErrNotFound, purchaseOrderID)
	}
	switch existing.Status {
	case PurchaseOrderStatusCancelled, PurchaseOrderStatusClosed:
		return PurchaseOrder{}, fmt.Errorf("%w: purchase order %s is %s and cannot be amended; raise a new order instead",
			httpx.ErrConflict, existing.ID, existing.Status)
	}

	po := in.PurchaseOrder
	po.ID = purchaseOrderID
	// The outlet is NOT movable, for the reason a supplier is not: every
	// receipt and gap already recorded against this order lives at an outlet.
	po.OutletID = existing.OutletID
	if strings.TrimSpace(po.SupplierID) == "" {
		po.SupplierID = existing.SupplierID
	}
	if strings.TrimSpace(po.PoNumber) == "" {
		po.PoNumber = existing.PoNumber
	}
	if po.TotalPaise < 0 {
		return PurchaseOrder{}, fmt.Errorf("%w: total_paise must not be negative", httpx.ErrInvalidInput)
	}

	supplierOutlet, supplierFound, err := s.repo.SupplierOutlet(ctx, po.SupplierID)
	if err != nil {
		return PurchaseOrder{}, err
	}
	if !supplierFound {
		return PurchaseOrder{}, fmt.Errorf("%w: supplier %s does not exist", httpx.ErrInvalidInput, po.SupplierID)
	}
	if supplierOutlet != po.OutletID {
		return PurchaseOrder{}, fmt.Errorf("%w: supplier %s belongs to a different outlet", httpx.ErrForbidden, po.SupplierID)
	}

	// THE REVOCATION, expressed in the status as well as the two columns. A
	// caller may ask for DRAFT or CANCELLED instead; it may not ask for
	// APPROVED, SENT or CLOSED, which are reachable only through the approve
	// route and only after the ceiling has seen the new total.
	switch po.Status {
	case "":
		if existing.Status == PurchaseOrderStatusDraft {
			po.Status = PurchaseOrderStatusDraft
		} else {
			po.Status = PurchaseOrderStatusPendingApproval
		}
	case PurchaseOrderStatusDraft, PurchaseOrderStatusPendingApproval, PurchaseOrderStatusCancelled:
		// fine
	case PurchaseOrderStatusApproved, PurchaseOrderStatusSent, PurchaseOrderStatusClosed:
		return PurchaseOrder{}, fmt.Errorf("%w: status %q may only be reached through POST /procurement/purchase-orders/{id}/approve",
			httpx.ErrInvalidInput, po.Status)
	default:
		return PurchaseOrder{}, fmt.Errorf("%w: unknown purchase order status %q", httpx.ErrInvalidInput, po.Status)
	}

	lines, err := s.normalisePurchaseOrderLines(ctx, po.ID, po.Lines)
	if err != nil {
		return PurchaseOrder{}, err
	}
	po.Lines = lines

	po.ApprovedByUserID = nil
	po.ApprovedAt = nil
	po.CreatedAt = existing.CreatedAt
	po.SchemaVersion = 1

	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, po.OutletID)
		if err != nil {
			return err
		}
		po.ConfigVersion = int64(newVersion)
		return s.repo.AmendPurchaseOrder(ctx, tx, po)
	})
	if err != nil {
		return PurchaseOrder{}, err
	}
	return po, nil
}
