package procurement

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

const (
	testTenantID      = "11111111-1111-7111-8111-111111111111"
	testOutletID      = "22222222-2222-7222-8222-222222222222"
	otherOutletID     = "33333333-3333-7333-8333-333333333333"
	otherTenantID     = "44444444-4444-7444-8444-444444444444"
	testDeviceID      = "55555555-5555-7555-8555-555555555555"
	testUserID        = "66666666-6666-7666-8666-666666666666"
	testSupplierID    = "77777777-7777-7777-8777-777777777777"
	massItemID        = "88888888-8888-7888-8888-888888888888"
	countItemID       = "99999999-9999-7999-8999-999999999999"
	testPurchaseOrder = "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa"
)

// fakeRepository is an in-memory Repository, mirroring
// backend/internal/compliance and backend/internal/kitchen's pattern.
type fakeRepository struct {
	outletTenant   map[string]string
	outletVersions map[string]int

	itemDimensions map[string]Dimension

	suppliers     map[string]Supplier
	supplierItems map[string][]SupplierItem

	purchaseOrders map[string]PurchaseOrder

	// approvalLimits is keyed by userID and holds what
	// PoApprovalLimitForUser returns. A MISSING KEY and a key mapped to nil
	// are the same answer — "may not approve any amount" — which is the point:
	// absence is never unlimited.
	approvalLimits    map[string]*int64
	rolesAbleApproveN map[int64][]string

	receipts     map[string]GoodsReceiptNote
	receiptLines map[string][]GrnLine
	gaps         map[string]GrnGap
	returns      map[string]PurchaseReturn
	returnLines  map[string][]PurchaseReturnLine
	transfers    map[string]StockTransferOut
	xferLines    map[string][]StockTransferLine

	receivedByLine map[string]int64

	supplierInvoices []SupplierInvoice
	supplierCredits  []SupplierCredit
}

func newFakeRepository() *fakeRepository {
	return &fakeRepository{
		outletTenant:      map[string]string{testOutletID: testTenantID, otherOutletID: testTenantID},
		outletVersions:    map[string]int{},
		itemDimensions:    map[string]Dimension{massItemID: DimensionMass, countItemID: DimensionCount},
		suppliers:         map[string]Supplier{},
		supplierItems:     map[string][]SupplierItem{},
		purchaseOrders:    map[string]PurchaseOrder{},
		approvalLimits:    map[string]*int64{},
		rolesAbleApproveN: map[int64][]string{},
		receipts:          map[string]GoodsReceiptNote{},
		receiptLines:      map[string][]GrnLine{},
		gaps:              map[string]GrnGap{},
		returns:           map[string]PurchaseReturn{},
		returnLines:       map[string][]PurchaseReturnLine{},
		transfers:         map[string]StockTransferOut{},
		xferLines:         map[string][]StockTransferLine{},
		receivedByLine:    map[string]int64{},
	}
}

func (f *fakeRepository) WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error { return fn(nil) }

func (f *fakeRepository) BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error) {
	if _, ok := f.outletTenant[outletID]; !ok {
		return 0, httpx.ErrNotFound
	}
	f.outletVersions[outletID]++
	return f.outletVersions[outletID], nil
}

func (f *fakeRepository) OutletBelongsToTenant(ctx context.Context, tenantID, outletID string) (bool, error) {
	return f.outletTenant[outletID] == tenantID, nil
}

func (f *fakeRepository) InventoryItemDimension(ctx context.Context, itemID string) (Dimension, bool, error) {
	d, ok := f.itemDimensions[itemID]
	return d, ok, nil
}

func (f *fakeRepository) UpsertSupplier(ctx context.Context, tx pgx.Tx, s Supplier, items []SupplierItem) error {
	f.suppliers[s.ID] = s
	f.supplierItems[s.ID] = items
	return nil
}

func (f *fakeRepository) SuppliersSince(ctx context.Context, outletID string, since int) ([]Supplier, error) {
	var out []Supplier
	for _, s := range f.suppliers {
		if s.OutletID == outletID && s.ConfigVersion > int64(since) {
			out = append(out, s)
		}
	}
	return out, nil
}

func (f *fakeRepository) SupplierItemsSince(ctx context.Context, outletID string, since int) ([]SupplierItem, error) {
	var out []SupplierItem
	for id, s := range f.suppliers {
		if s.OutletID == outletID && s.ConfigVersion > int64(since) {
			out = append(out, f.supplierItems[id]...)
		}
	}
	return out, nil
}

func (f *fakeRepository) SupplierOutlet(ctx context.Context, supplierID string) (string, bool, error) {
	s, ok := f.suppliers[supplierID]
	if !ok {
		return "", false, nil
	}
	return s.OutletID, true, nil
}

func (f *fakeRepository) UpsertPurchaseOrder(ctx context.Context, tx pgx.Tx, po PurchaseOrder) error {
	// Mirrors the real INSERT: approval columns are NOT written here.
	if existing, ok := f.purchaseOrders[po.ID]; ok {
		po.ApprovedByUserID = existing.ApprovedByUserID
		po.ApprovedAt = existing.ApprovedAt
	}
	f.purchaseOrders[po.ID] = po
	return nil
}

func (f *fakeRepository) GetPurchaseOrder(ctx context.Context, tenantID, id string) (PurchaseOrder, bool, error) {
	po, ok := f.purchaseOrders[id]
	if !ok || f.outletTenant[po.OutletID] != tenantID {
		return PurchaseOrder{}, false, nil
	}
	return po, true, nil
}

func (f *fakeRepository) PurchaseOrdersSince(ctx context.Context, outletID string, since int) ([]PurchaseOrder, error) {
	var out []PurchaseOrder
	for _, po := range f.purchaseOrders {
		if po.OutletID == outletID && po.ConfigVersion > int64(since) {
			out = append(out, po)
		}
	}
	return out, nil
}

func (f *fakeRepository) PurchaseOrderLinesSince(ctx context.Context, outletID string, since int) ([]PurchaseOrderLine, error) {
	out := []PurchaseOrderLine{}
	for _, po := range f.purchaseOrders {
		if po.OutletID == outletID && po.ConfigVersion > int64(since) {
			out = append(out, po.Lines...)
		}
	}
	return out, nil
}

func (f *fakeRepository) PurchaseOrderLines(ctx context.Context, id string) ([]PurchaseOrderLine, error) {
	return f.purchaseOrders[id].Lines, nil
}

func (f *fakeRepository) ApprovePurchaseOrder(ctx context.Context, tx pgx.Tx, id, approver string, at time.Time, configVersion int) error {
	po, ok := f.purchaseOrders[id]
	if !ok {
		return httpx.ErrNotFound
	}
	// Both columns, one call — the fake cannot express a half-approval either.
	a := at.UTC().Format(time.RFC3339)
	po.Status = PurchaseOrderStatusApproved
	po.ApprovedByUserID = &approver
	po.ApprovedAt = &a
	po.ConfigVersion = int64(configVersion)
	f.purchaseOrders[id] = po
	return nil
}

func (f *fakeRepository) PoApprovalLimitForUser(ctx context.Context, tenantID, outletID, userID string) (*int64, error) {
	return f.approvalLimits[userID], nil
}

func (f *fakeRepository) RolesAbleToApprove(ctx context.Context, tenantID string, totalPaise int64) ([]string, error) {
	if roles, ok := f.rolesAbleApproveN[totalPaise]; ok {
		return roles, nil
	}
	return []string{}, nil
}

func (f *fakeRepository) GetGoodsReceiptNoteByID(ctx context.Context, id string) (GoodsReceiptNote, bool, error) {
	g, ok := f.receipts[id]
	return g, ok, nil
}

func (f *fakeRepository) GrnLines(ctx context.Context, grnID string) ([]GrnLine, error) {
	return f.receiptLines[grnID], nil
}

func (f *fakeRepository) InsertGoodsReceiptNote(ctx context.Context, tx pgx.Tx, tenantID string, g GoodsReceiptNote, lines []GrnLine) error {
	f.receipts[g.ID] = g
	f.receiptLines[g.ID] = lines
	return nil
}

func (f *fakeRepository) GetGrnGapByID(ctx context.Context, id string) (GrnGap, bool, error) {
	g, ok := f.gaps[id]
	return g, ok, nil
}

func (f *fakeRepository) InsertGrnGap(ctx context.Context, tenantID string, g GrnGap) error {
	f.gaps[g.ID] = g
	return nil
}

func (f *fakeRepository) GetPurchaseReturnByID(ctx context.Context, id string) (PurchaseReturn, bool, error) {
	p, ok := f.returns[id]
	return p, ok, nil
}

func (f *fakeRepository) PurchaseReturnLines(ctx context.Context, id string) ([]PurchaseReturnLine, error) {
	return f.returnLines[id], nil
}

func (f *fakeRepository) InsertPurchaseReturn(ctx context.Context, tx pgx.Tx, tenantID string, p PurchaseReturn, lines []PurchaseReturnLine) error {
	f.returns[p.ID] = p
	f.returnLines[p.ID] = lines
	return nil
}

func (f *fakeRepository) GetStockTransferOutByID(ctx context.Context, id string) (StockTransferOut, bool, error) {
	s, ok := f.transfers[id]
	return s, ok, nil
}

func (f *fakeRepository) StockTransferLines(ctx context.Context, id string) ([]StockTransferLine, error) {
	return f.xferLines[id], nil
}

func (f *fakeRepository) InsertStockTransferOut(ctx context.Context, tx pgx.Tx, tenantID string, s StockTransferOut, lines []StockTransferLine) error {
	f.transfers[s.ID] = s
	f.xferLines[s.ID] = lines
	return nil
}

func (f *fakeRepository) ReceivedBaseQuantityByPurchaseOrderLine(ctx context.Context, id string) (map[string]int64, error) {
	return f.receivedByLine, nil
}

func (f *fakeRepository) InsertSupplierInvoice(ctx context.Context, inv SupplierInvoice) error {
	f.supplierInvoices = append(f.supplierInvoices, inv)
	return nil
}

func (f *fakeRepository) ListSupplierInvoices(ctx context.Context, tenantID, outletID string) ([]SupplierInvoice, error) {
	return f.supplierInvoices, nil
}

func (f *fakeRepository) InsertSupplierCredit(ctx context.Context, c SupplierCredit) error {
	f.supplierCredits = append(f.supplierCredits, c)
	return nil
}

func (f *fakeRepository) ListSupplierCredits(ctx context.Context, tenantID, outletID string) ([]SupplierCredit, error) {
	return f.supplierCredits, nil
}

// --- helpers ----------------------------------------------------------------

func newTestService() (*Service, *fakeRepository) {
	repo := newFakeRepository()
	svc := NewService(repo)
	svc.now = func() time.Time { return time.Date(2026, 8, 29, 12, 0, 0, 0, time.UTC) }
	return svc, repo
}

// seedSupplier makes a supplier exist WITHOUT overwriting one a test already
// created through the service — overwriting would reset its config_version to
// zero and silently drop it out of every since_version-filtered assertion.
func seedSupplier(repo *fakeRepository) {
	if _, exists := repo.suppliers[testSupplierID]; exists {
		return
	}
	repo.suppliers[testSupplierID] = Supplier{ID: testSupplierID, OutletID: testOutletID, Code: "ACME", Name: "Acme"}
}

func ptrInt64(v int64) *int64 { return &v }

func envelopeFor(aggregate contracts.AggregateType, recordID string) contracts.SyncEnvelope {
	return contracts.SyncEnvelope{
		RecordID:      recordID,
		TenantID:      testTenantID,
		OutletID:      testOutletID,
		DeviceID:      testDeviceID,
		AggregateType: aggregate,
		Direction:     contracts.AggregateAuthority[aggregate],
		Version:       1,
		SyncStatus:    contracts.SyncStatusPending,
	}
}

// --- supplier: the 0.5.2 dimension guard ------------------------------------

// TestCreateSupplier_RejectsDimensionMismatch is the guard ADR-019 §6 exists
// for. The author says MASS, the item is COUNT, and the write is refused — it
// is NOT silently corrected to the item's dimension.
func TestCreateSupplier_RejectsDimensionMismatch(t *testing.T) {
	svc, _ := newTestService()
	_, _, err := svc.CreateSupplier(context.Background(), testTenantID, NewSupplierInput{
		Supplier: Supplier{ID: testSupplierID, OutletID: testOutletID, Code: "ACME", Name: "Acme"},
		Items: []SupplierItem{{
			ID: "item-1", InventoryItemID: countItemID, PurchaseUnit: "sack",
			PackSizeMicro: 50_000_000, QuantityDimension: DimensionMass,
		}},
	})
	if !errors.Is(err, ErrDimensionMismatch) {
		t.Fatalf("want ErrDimensionMismatch, got %v", err)
	}
	if !strings.Contains(err.Error(), "COUNT") || !strings.Contains(err.Error(), "MASS") {
		t.Errorf("message must name BOTH dimensions so an author can see what disagrees: %v", err)
	}
}

// TestCreateSupplier_StoresTheAuthorsDimensionVerbatim is the OTHER half of
// the same guard, and the one that catches the failure ADR-019 §6 warns is
// invisible in review: if the write path auto-filled quantity_dimension from
// the item, the comparison above would be x == x and could never fire. This
// asserts the stored value is what the AUTHOR sent, not something derived.
func TestCreateSupplier_StoresTheAuthorsDimensionVerbatim(t *testing.T) {
	svc, repo := newTestService()
	_, items, err := svc.CreateSupplier(context.Background(), testTenantID, NewSupplierInput{
		Supplier: Supplier{ID: testSupplierID, OutletID: testOutletID, Code: "ACME", Name: "Acme"},
		Items: []SupplierItem{{
			ID: "item-1", InventoryItemID: massItemID, PurchaseUnit: "sack",
			PackSizeMicro: 50_000_000, QuantityDimension: DimensionMass,
		}},
	})
	if err != nil {
		t.Fatalf("CreateSupplier: %v", err)
	}
	if len(items) != 1 || items[0].QuantityDimension != DimensionMass {
		t.Fatalf("author's dimension not preserved: %+v", items)
	}
	// Fixtures actually inserted — asserted before anything is claimed about
	// them, so a rejected write cannot leave every later assertion trivially
	// true on zero rows.
	if len(repo.supplierItems[testSupplierID]) != 1 {
		t.Fatalf("supplier_item rows did not insert: %+v", repo.supplierItems)
	}
	if repo.suppliers[testSupplierID].ConfigVersion == 0 {
		t.Error("config_version was not bumped, so this supplier would never reach GET /sync/config")
	}
}

func TestCreateSupplier_RefusesAnotherTenantsOutlet(t *testing.T) {
	svc, _ := newTestService()
	_, _, err := svc.CreateSupplier(context.Background(), otherTenantID, NewSupplierInput{
		Supplier: Supplier{ID: testSupplierID, OutletID: testOutletID, Code: "ACME", Name: "Acme"},
	})
	if !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("want ErrForbidden for a cross-tenant outlet, got %v", err)
	}
}

// --- purchase order ---------------------------------------------------------

func seedPurchaseOrder(t *testing.T, svc *Service, repo *fakeRepository, totalPaise int64) PurchaseOrder {
	t.Helper()
	seedSupplier(repo)
	po, err := svc.CreatePurchaseOrder(context.Background(), testTenantID, NewPurchaseOrderInput{
		PurchaseOrder: PurchaseOrder{
			ID: testPurchaseOrder, OutletID: testOutletID, SupplierID: testSupplierID,
			PoNumber: "PO-1", Status: PurchaseOrderStatusPendingApproval, TotalPaise: totalPaise,
			Lines: []PurchaseOrderLine{{
				ID: "po-line-1", InventoryItemID: massItemID, LineNumber: 1, PurchaseUnit: "sack",
				OrderedQuantityMicro: 100_000_000, QuantityDimension: DimensionMass,
				UnitPricePaise: 1000, LineTotalPaise: totalPaise,
			}},
		},
	})
	if err != nil {
		t.Fatalf("CreatePurchaseOrder: %v", err)
	}
	if _, ok := repo.purchaseOrders[po.ID]; !ok {
		t.Fatalf("purchase order did not insert")
	}
	return po
}

// TestCreatePurchaseOrder_CannotSelfApprove proves the create route cannot
// grant an approval — neither by asking for an approved status nor by posting
// the approval columns directly.
func TestCreatePurchaseOrder_CannotSelfApprove(t *testing.T) {
	svc, repo := newTestService()
	seedSupplier(repo)

	_, err := svc.CreatePurchaseOrder(context.Background(), testTenantID, NewPurchaseOrderInput{
		PurchaseOrder: PurchaseOrder{
			ID: testPurchaseOrder, OutletID: testOutletID, SupplierID: testSupplierID,
			PoNumber: "PO-1", Status: PurchaseOrderStatusApproved, TotalPaise: 100,
		},
	})
	if !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("APPROVED must be unreachable from the create route, got %v", err)
	}

	smuggled := testUserID
	at := "2026-08-29T12:00:00Z"
	po, err := svc.CreatePurchaseOrder(context.Background(), testTenantID, NewPurchaseOrderInput{
		PurchaseOrder: PurchaseOrder{
			ID: testPurchaseOrder, OutletID: testOutletID, SupplierID: testSupplierID,
			PoNumber: "PO-1", Status: PurchaseOrderStatusDraft, TotalPaise: 100,
			ApprovedByUserID: &smuggled, ApprovedAt: &at,
		},
	})
	if err != nil {
		t.Fatalf("CreatePurchaseOrder: %v", err)
	}
	if po.ApprovedByUserID != nil || po.ApprovedAt != nil {
		t.Fatalf("approval columns must be ignored on the create route, got %v/%v", po.ApprovedByUserID, po.ApprovedAt)
	}
}

func TestCreatePurchaseOrder_RejectsLineDimensionMismatch(t *testing.T) {
	svc, repo := newTestService()
	seedSupplier(repo)
	_, err := svc.CreatePurchaseOrder(context.Background(), testTenantID, NewPurchaseOrderInput{
		PurchaseOrder: PurchaseOrder{
			ID: testPurchaseOrder, OutletID: testOutletID, SupplierID: testSupplierID,
			PoNumber: "PO-1", TotalPaise: 100,
			Lines: []PurchaseOrderLine{{
				ID: "po-line-1", InventoryItemID: massItemID, LineNumber: 1, PurchaseUnit: "sack",
				OrderedQuantityMicro: 1, QuantityDimension: DimensionCount,
				UnitPricePaise: 1, LineTotalPaise: 1,
			}},
		},
	})
	if !errors.Is(err, ErrDimensionMismatch) {
		t.Fatalf("want ErrDimensionMismatch on a PO line, got %v", err)
	}
}

// --- the two approval gates -------------------------------------------------

func principalWith(perms ...contracts.Permission) auth.AuthenticatedPrincipal {
	return auth.AuthenticatedPrincipal{
		UserID: testUserID, TenantID: testTenantID, OutletID: testOutletID, Permissions: perms,
	}
}

// TestApprove_RefusesCallerWithoutTheApprovePermission is GATE 1. A caller
// holding procurement.manage — enough to RAISE the order — may not approve it.
func TestApprove_RefusesCallerWithoutTheApprovePermission(t *testing.T) {
	svc, repo := newTestService()
	po := seedPurchaseOrder(t, svc, repo, 500_00)
	repo.approvalLimits[testUserID] = ptrInt64(10_000_00) // generous, and irrelevant

	_, err := svc.ApprovePurchaseOrder(context.Background(), principalWith(PermissionManage), po.ID)
	if !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("want a forbidden refusal, got %v", err)
	}
	var refusal *ApprovalRefusal
	if !errors.As(err, &refusal) {
		t.Fatalf("want *ApprovalRefusal, got %T", err)
	}
	if refusal.Code != approvalRefusalCodeNoPermission {
		t.Errorf("want code %q, got %q", approvalRefusalCodeNoPermission, refusal.Code)
	}
	if repo.purchaseOrders[po.ID].ApprovedByUserID != nil {
		t.Error("a refused approval must leave the row untouched")
	}
}

// TestApprove_NullLimitMeansMayNotApproveAnyAmount is GATE 2's central case:
// a NULL role limit is NOT unlimited. This is the printer_role rule — absence
// is never read as permission — and getting it backwards would turn every
// unconfigured role into an unbounded approver, silently.
func TestApprove_NullLimitMeansMayNotApproveAnyAmount(t *testing.T) {
	svc, repo := newTestService()
	po := seedPurchaseOrder(t, svc, repo, 1)
	// No entry at all: PoApprovalLimitForUser returns nil.
	repo.rolesAbleApproveN[1] = []string{"Finance Director"}

	_, err := svc.ApprovePurchaseOrder(context.Background(), principalWith(PermissionApprove), po.ID)
	var refusal *ApprovalRefusal
	if !errors.As(err, &refusal) {
		t.Fatalf("want *ApprovalRefusal, got %v", err)
	}
	if refusal.LimitPaise != nil {
		t.Errorf("a NULL limit must stay nil on the refusal, not become 0: %v", *refusal.LimitPaise)
	}
	if !strings.Contains(refusal.Error(), "may not approve any amount") {
		t.Errorf("message must say the caller may not approve ANY amount: %s", refusal.Error())
	}
	if !strings.Contains(refusal.Error(), "Finance Director") {
		t.Errorf("§64: the message must name who can approve it instead: %s", refusal.Error())
	}
	if repo.purchaseOrders[po.ID].ApprovedAt != nil {
		t.Error("a refused approval must leave the row untouched")
	}
}

// TestApprove_RefusesOverLimitAndSaysWhatToDoNext is acceptance criterion 5's
// backend half. §64: the message names the TOTAL, the CEILING and the next
// action. "Forbidden" alone leaves a buyer with a delivery due and nothing to
// act on.
func TestApprove_RefusesOverLimitAndSaysWhatToDoNext(t *testing.T) {
	svc, repo := newTestService()
	const total int64 = 250_000_00
	po := seedPurchaseOrder(t, svc, repo, total)
	repo.approvalLimits[testUserID] = ptrInt64(50_000_00)
	repo.rolesAbleApproveN[total] = []string{"Regional Manager"}

	_, err := svc.ApprovePurchaseOrder(context.Background(), principalWith(PermissionApprove), po.ID)
	var refusal *ApprovalRefusal
	if !errors.As(err, &refusal) {
		t.Fatalf("want *ApprovalRefusal, got %v", err)
	}
	if refusal.Code != approvalRefusalCodeOverLimit {
		t.Errorf("want code %q, got %q", approvalRefusalCodeOverLimit, refusal.Code)
	}
	if refusal.TotalPaise != total {
		t.Errorf("want total %d, got %d", total, refusal.TotalPaise)
	}
	if refusal.LimitPaise == nil || *refusal.LimitPaise != 50_000_00 {
		t.Errorf("want the caller's ceiling on the refusal, got %v", refusal.LimitPaise)
	}
	msg := refusal.Error()
	for _, want := range []string{"25000000", "5000000", "Regional Manager"} {
		if !strings.Contains(msg, want) {
			t.Errorf("§64: message must contain %q — got %s", want, msg)
		}
	}
}

// TestApprove_WritesBothApprovalColumnsTogether is the whole-or-nothing rule.
func TestApprove_WritesBothApprovalColumnsTogether(t *testing.T) {
	svc, repo := newTestService()
	po := seedPurchaseOrder(t, svc, repo, 10_000)
	repo.approvalLimits[testUserID] = ptrInt64(10_000)

	approved, err := svc.ApprovePurchaseOrder(context.Background(), principalWith(PermissionApprove), po.ID)
	if err != nil {
		t.Fatalf("an exactly-at-the-limit approval must succeed: %v", err)
	}
	if approved.Status != PurchaseOrderStatusApproved {
		t.Errorf("want APPROVED, got %s", approved.Status)
	}
	if approved.ApprovedByUserID == nil || approved.ApprovedAt == nil {
		t.Fatalf("both approval fields must be set: %v/%v", approved.ApprovedByUserID, approved.ApprovedAt)
	}
	stored := repo.purchaseOrders[po.ID]
	if stored.ApprovedByUserID == nil || stored.ApprovedAt == nil {
		t.Fatalf("stored row must carry both approval fields: %+v", stored)
	}
	if *stored.ApprovedByUserID != testUserID {
		t.Errorf("want approver %s, got %s", testUserID, *stored.ApprovedByUserID)
	}
}

func TestApprove_RefusesAnotherTenantsPurchaseOrder(t *testing.T) {
	svc, repo := newTestService()
	po := seedPurchaseOrder(t, svc, repo, 100)
	repo.approvalLimits[testUserID] = ptrInt64(1_000_000)

	p := principalWith(PermissionApprove)
	p.TenantID = otherTenantID
	_, err := svc.ApprovePurchaseOrder(context.Background(), p, po.ID)
	// NOT FOUND, not forbidden: a 403 would confirm the id exists.
	if !errors.Is(err, httpx.ErrNotFound) {
		t.Fatalf("want ErrNotFound for a cross-tenant purchase order, got %v", err)
	}
}

// --- receipt progress is derived and labelled -------------------------------

func TestReceiptProgress_IsDerivedAndLabelledCloudWide(t *testing.T) {
	svc, repo := newTestService()
	po := seedPurchaseOrder(t, svc, repo, 1000)
	repo.receivedByLine["po-line-1"] = 40_000_000

	progress, err := svc.PurchaseOrderReceiptProgress(context.Background(), testTenantID, po.ID)
	if err != nil {
		t.Fatalf("PurchaseOrderReceiptProgress: %v", err)
	}
	if progress.Scope != ScopeCloudWide {
		t.Errorf("the figure must label its own scope so an edge figure cannot be shown as this one: %q", progress.Scope)
	}
	if len(progress.Lines) != 1 || progress.Lines[0].ReceivedBaseQuantityMicro != 40_000_000 {
		t.Fatalf("progress not derived from grn_line rows: %+v", progress.Lines)
	}
	if progress.Lines[0].OrderedQuantityMicro != 100_000_000 {
		t.Errorf("ordered quantity lost: %+v", progress.Lines[0])
	}
	// And nothing was written back onto the order.
	if repo.purchaseOrders[po.ID].Status != PurchaseOrderStatusPendingApproval {
		t.Error("deriving progress must not transition the purchase order")
	}
}

// --- GRN ingest: NEVER BLOCKS ON A PO ---------------------------------------

func grnFixture() GoodsReceiptNote {
	return GoodsReceiptNote{
		ID: "grn-1", OutletID: testOutletID, GrnNumber: "GRN-1",
		ReceivedAt: "2026-08-29T10:00:00Z", ReceivedByUserID: testUserID,
		BusinessDate: "2026-08-29",
		Lines: []GrnLine{{
			ID: "grn-line-1", InventoryItemID: massItemID, LineNumber: 1,
			EnteredPurchaseUnit: "sack", EnteredQuantityMicro: 2_000_000,
			QuantityDimension: DimensionMass, BaseQuantityMicro: 100_000_000,
			PackSizeMicroApplied: 50_000_000, UnitCostPaise: 40, LineTotalPaise: 4000,
		}},
	}
}

// TestIngestGoodsReceipt_AcceptsAReceiptWithNoPurchaseOrder is ADR-019 §1 and
// M5 acceptance criterion 3's cloud half. A receipt with a NULL
// purchase_order_id, NULL supplier_id and a NULL purchase_order_line_id is
// STORED, not refused. A cloud-side rejection here would refuse the replay of
// a receipt the edge correctly accepted.
func TestIngestGoodsReceipt_AcceptsAReceiptWithNoPurchaseOrder(t *testing.T) {
	svc, repo := newTestService()
	grn := grnFixture()

	stored, err := svc.IngestGoodsReceiptNote(context.Background(), testTenantID,
		envelopeFor(contracts.AggregateTypeGoodsReceiptNote, grn.ID), grn)
	if err != nil {
		t.Fatalf("a receipt with no PO must be accepted: %v", err)
	}
	if _, ok := repo.receipts[grn.ID]; !ok {
		t.Fatal("the receipt did not insert")
	}
	if stored.PurchaseOrderID != nil || stored.SupplierID != nil {
		t.Error("nothing may invent a PO or supplier link that the edge did not send")
	}
	if len(repo.receiptLines[grn.ID]) != 1 {
		t.Fatalf("grn_line rows did not insert: %+v", repo.receiptLines)
	}
}

// TestIngestGoodsReceipt_AcceptsAPurchaseOrderIdThisCloudHasNeverSeen is the
// other half: the link is stored verbatim even when it resolves to nothing.
// Validating it into existence would be the same outage one hop later.
func TestIngestGoodsReceipt_AcceptsAPurchaseOrderIdThisCloudHasNeverSeen(t *testing.T) {
	svc, repo := newTestService()
	grn := grnFixture()
	unknownPO := "bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb"
	unknownLine := "cccccccc-cccc-7ccc-8ccc-cccccccccccc"
	grn.PurchaseOrderID = &unknownPO
	grn.Lines[0].PurchaseOrderLineID = &unknownLine

	stored, err := svc.IngestGoodsReceiptNote(context.Background(), testTenantID,
		envelopeFor(contracts.AggregateTypeGoodsReceiptNote, grn.ID), grn)
	if err != nil {
		t.Fatalf("a receipt naming an unsynced PO must be accepted: %v", err)
	}
	if stored.PurchaseOrderID == nil || *stored.PurchaseOrderID != unknownPO {
		t.Errorf("purchase_order_id must be stored verbatim, got %v", stored.PurchaseOrderID)
	}
	if got := repo.receiptLines[grn.ID][0].PurchaseOrderLineID; got == nil || *got != unknownLine {
		t.Errorf("purchase_order_line_id must be stored verbatim, got %v", got)
	}
}

// TestIngestGoodsReceipt_RecomputesNeitherSideOfTheConversion: the edge
// converted once. Even a receipt whose base quantity disagrees with
// entered × pack_size is stored as sent — recomputing would restate history
// against configuration that may since have changed.
func TestIngestGoodsReceipt_RecomputesNeitherSideOfTheConversion(t *testing.T) {
	svc, repo := newTestService()
	grn := grnFixture()
	grn.Lines[0].BaseQuantityMicro = 99_999_999 // deliberately not 2 x 50_000_000

	if _, err := svc.IngestGoodsReceiptNote(context.Background(), testTenantID,
		envelopeFor(contracts.AggregateTypeGoodsReceiptNote, grn.ID), grn); err != nil {
		t.Fatalf("IngestGoodsReceiptNote: %v", err)
	}
	line := repo.receiptLines[grn.ID][0]
	if line.BaseQuantityMicro != 99_999_999 {
		t.Errorf("base_quantity_micro was recomputed: got %d", line.BaseQuantityMicro)
	}
	if line.EnteredQuantityMicro != 2_000_000 || line.PackSizeMicroApplied != 50_000_000 {
		t.Errorf("both sides of the conversion must survive verbatim: %+v", line)
	}
}

func TestIngestGoodsReceipt_IsIdempotentOnID(t *testing.T) {
	svc, repo := newTestService()
	grn := grnFixture()
	env := envelopeFor(contracts.AggregateTypeGoodsReceiptNote, grn.ID)
	if _, err := svc.IngestGoodsReceiptNote(context.Background(), testTenantID, env, grn); err != nil {
		t.Fatalf("first replay: %v", err)
	}
	if _, err := svc.IngestGoodsReceiptNote(context.Background(), testTenantID, env, grn); err != nil {
		t.Fatalf("a repeated replay is an ordinary retry, not a fault: %v", err)
	}
	if len(repo.receipts) != 1 {
		t.Fatalf("want one stored receipt, got %d", len(repo.receipts))
	}
}

// --- envelope authority: 422, never coerced ---------------------------------

func TestIngest_RejectsWrongAggregateTypeForRoute(t *testing.T) {
	svc, _ := newTestService()
	grn := grnFixture()
	// A grn_gap envelope handed to the receipt ingest, and a receipt envelope
	// handed to the return ingest: both are route mismatches.
	env := envelopeFor(contracts.AggregateTypeGrnGap, grn.ID)
	if _, err := svc.IngestGoodsReceiptNote(context.Background(), testTenantID, env, grn); !errors.Is(err, ErrAuthorityViolation) {
		t.Fatalf("want ErrAuthorityViolation, got %v", err)
	}
	env2 := envelopeFor(contracts.AggregateTypeGoodsReceiptNote, "ret-1")
	if _, err := svc.IngestPurchaseReturn(context.Background(), testTenantID, env2, PurchaseReturn{ID: "ret-1"}); !errors.Is(err, ErrAuthorityViolation) {
		t.Fatalf("want ErrAuthorityViolation, got %v", err)
	}
}

// TestIngest_RejectsWrongDirection pins §50.1: an EDGE_TO_CLOUD aggregate
// arriving with CLOUD_TO_EDGE is a protocol violation, not something to
// silently correct.
func TestIngest_RejectsWrongDirection(t *testing.T) {
	svc, _ := newTestService()
	grn := grnFixture()
	env := envelopeFor(contracts.AggregateTypeGoodsReceiptNote, grn.ID)
	env.Direction = contracts.SyncDirectionCloudToEdge
	if _, err := svc.IngestGoodsReceiptNote(context.Background(), testTenantID, env, grn); !errors.Is(err, ErrAuthorityViolation) {
		t.Fatalf("want ErrAuthorityViolation for a flipped direction, got %v", err)
	}
}

func TestIngest_RejectsEnvelopeForAnotherTenant(t *testing.T) {
	svc, _ := newTestService()
	grn := grnFixture()
	env := envelopeFor(contracts.AggregateTypeGoodsReceiptNote, grn.ID)
	if _, err := svc.IngestGoodsReceiptNote(context.Background(), otherTenantID, env, grn); !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("want ErrForbidden, got %v", err)
	}
}

// --- grn_gap ----------------------------------------------------------------

func TestIngestGrnGap_AcceptsEveryContractReason(t *testing.T) {
	reasons := []GrnGapReason{
		contracts.GrnGapReasonNoPurchaseOrder,
		contracts.GrnGapReasonPurchaseOrderNotFound,
		contracts.GrnGapReasonPoLineNotFound,
		contracts.GrnGapReasonQuantityExceedsOrdered,
		contracts.GrnGapReasonNoSupplierItem,
		contracts.GrnGapReasonNoUnitConversion,
		contracts.GrnGapReasonDimensionMismatch,
		contracts.GrnGapReasonSupplierNotFound,
	}
	svc, repo := newTestService()
	for i, reason := range reasons {
		id := string(reason)
		detail := "the buyer reads this"
		gap := GrnGap{
			ID: id, OutletID: testOutletID, GrnID: "grn-1", Reason: reason, Detail: &detail,
			OccurredAt: "2026-08-29T10:00:00Z", BusinessDate: "2026-08-29",
		}
		if _, err := svc.IngestGrnGap(context.Background(), testTenantID,
			envelopeFor(contracts.AggregateTypeGrnGap, id), gap); err != nil {
			t.Fatalf("reason %s (%d): %v", reason, i, err)
		}
	}
	if len(repo.gaps) != len(reasons) {
		t.Fatalf("want %d gap rows, got %d", len(reasons), len(repo.gaps))
	}
}

func TestIngestGrnGap_RejectsAnUnknownReason(t *testing.T) {
	svc, _ := newTestService()
	gap := GrnGap{ID: "gap-x", OutletID: testOutletID, GrnID: "grn-1", Reason: GrnGapReason("MADE_UP")}
	if _, err := svc.IngestGrnGap(context.Background(), testTenantID,
		envelopeFor(contracts.AggregateTypeGrnGap, gap.ID), gap); !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("want ErrInvalidInput, got %v", err)
	}
}

// --- purchase_return / stock_transfer_out -----------------------------------

func TestIngestPurchaseReturn_StoresLines(t *testing.T) {
	svc, repo := newTestService()
	ret := PurchaseReturn{
		ID: "ret-1", OutletID: testOutletID, ReturnNumber: "RET-1",
		Reason: contracts.PurchaseReturnReasonDamaged, ReturnedAt: "2026-08-29T11:00:00Z",
		ReturnedByUserID: testUserID, BusinessDate: "2026-08-29",
		Lines: []PurchaseReturnLine{{
			ID: "ret-line-1", InventoryItemID: massItemID, LineNumber: 1,
			EnteredPurchaseUnit: "sack", EnteredQuantityMicro: 1_000_000,
			QuantityDimension: DimensionMass, BaseQuantityMicro: 50_000_000, UnitCostPaise: 40,
		}},
	}
	if _, err := svc.IngestPurchaseReturn(context.Background(), testTenantID,
		envelopeFor(contracts.AggregateTypePurchaseReturn, ret.ID), ret); err != nil {
		t.Fatalf("IngestPurchaseReturn: %v", err)
	}
	if len(repo.returnLines["ret-1"]) != 1 {
		t.Fatalf("purchase_return_line rows did not insert: %+v", repo.returnLines)
	}
	if repo.returnLines["ret-1"][0].PurchaseReturnID != "ret-1" {
		t.Error("child rows must be stamped with their parent id")
	}
}

func TestIngestStockTransferOut_RefusesADestinationOutsideTheTenant(t *testing.T) {
	svc, repo := newTestService()
	repo.outletTenant["dddddddd-dddd-7ddd-8ddd-dddddddddddd"] = otherTenantID
	transfer := StockTransferOut{
		ID: "xfer-1", OutletID: testOutletID,
		DestinationOutletID: "dddddddd-dddd-7ddd-8ddd-dddddddddddd",
		TransferNumber:      "TR-1", DispatchedAt: "2026-08-29T11:00:00Z",
		DispatchedByUserID: testUserID, BusinessDate: "2026-08-29",
	}
	if _, err := svc.IngestStockTransferOut(context.Background(), testTenantID,
		envelopeFor(contracts.AggregateTypeStockTransferOut, transfer.ID), transfer); !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("want ErrForbidden for a cross-tenant destination, got %v", err)
	}
}

func TestIngestStockTransferOut_RefusesATransferToItself(t *testing.T) {
	svc, _ := newTestService()
	transfer := StockTransferOut{
		ID: "xfer-2", OutletID: testOutletID, DestinationOutletID: testOutletID,
		TransferNumber: "TR-2", DispatchedAt: "2026-08-29T11:00:00Z",
		DispatchedByUserID: testUserID, BusinessDate: "2026-08-29",
	}
	if _, err := svc.IngestStockTransferOut(context.Background(), testTenantID,
		envelopeFor(contracts.AggregateTypeStockTransferOut, transfer.ID), transfer); !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("want ErrInvalidInput, got %v", err)
	}
}

// --- supplier accounts: M5 records, M7 acts ---------------------------------

func TestCreateSupplierInvoice_RefusesAnyM7SettlementState(t *testing.T) {
	svc, repo := newTestService()
	seedSupplier(repo)
	for _, status := range []contracts.SupplierInvoiceStatus{
		contracts.SupplierInvoiceStatusApproved,
		contracts.SupplierInvoiceStatusPartPaid,
		contracts.SupplierInvoiceStatusPaid,
		contracts.SupplierInvoiceStatusDisputed,
		contracts.SupplierInvoiceStatusCancelled,
	} {
		_, err := svc.CreateSupplierInvoice(context.Background(), testTenantID, SupplierInvoice{
			ID: "inv-1", OutletID: testOutletID, SupplierID: testSupplierID,
			SupplierInvoiceNo: "SI-1", InvoiceDate: "2026-08-29", TotalPaise: 100, Status: status,
		})
		if !errors.Is(err, httpx.ErrInvalidInput) {
			t.Errorf("status %s is an M7 state and must be refused in M5, got %v", status, err)
		}
	}
	if len(repo.supplierInvoices) != 0 {
		t.Fatalf("no invoice should have been stored: %+v", repo.supplierInvoices)
	}

	stored, err := svc.CreateSupplierInvoice(context.Background(), testTenantID, SupplierInvoice{
		ID: "inv-1", OutletID: testOutletID, SupplierID: testSupplierID,
		SupplierInvoiceNo: "SI-1", InvoiceDate: "2026-08-29", TotalPaise: 100,
	})
	if err != nil {
		t.Fatalf("CreateSupplierInvoice: %v", err)
	}
	if stored.Status != SupplierInvoiceStatusReceived {
		t.Errorf("M5 writes RECEIVED only, got %s", stored.Status)
	}
	if stored.TenantID != testTenantID {
		t.Errorf("tenant_id must come from the principal, not the payload: %s", stored.TenantID)
	}
}

func TestCreateSupplierCredit_RefusesASupplierAtAnotherOutlet(t *testing.T) {
	svc, repo := newTestService()
	seedSupplier(repo)
	_, err := svc.CreateSupplierCredit(context.Background(), testTenantID, SupplierCredit{
		ID: "cr-1", OutletID: otherOutletID, SupplierID: testSupplierID,
		CreditNoteNo: "CN-1", CreditDate: "2026-08-29", AmountPaise: 100,
	})
	if !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("want ErrInvalidInput, got %v", err)
	}
}

// --- sync config ------------------------------------------------------------

// TestSyncConfigBundle_FiltersBySinceVersion proves the new config tables are
// reachable by GET /sync/config's since_version filter like every other config
// table — the mechanism that makes a cloud write arrive at an outlet at all.
func TestSyncConfigBundle_FiltersBySinceVersion(t *testing.T) {
	svc, repo := newTestService()
	if _, _, err := svc.CreateSupplier(context.Background(), testTenantID, NewSupplierInput{
		Supplier: Supplier{ID: testSupplierID, OutletID: testOutletID, Code: "ACME", Name: "Acme"},
		Items: []SupplierItem{{
			ID: "item-1", InventoryItemID: massItemID, PurchaseUnit: "sack",
			PackSizeMicro: 50_000_000, QuantityDimension: DimensionMass,
		}},
	}); err != nil {
		t.Fatalf("CreateSupplier: %v", err)
	}
	supplierVersion := repo.suppliers[testSupplierID].ConfigVersion
	if supplierVersion == 0 {
		t.Fatal("the supplier did not get a config_version, so nothing below is meaningful")
	}
	seedPurchaseOrder(t, svc, repo, 1000)

	all, err := svc.SyncConfigBundle(context.Background(), testTenantID, testOutletID, 0)
	if err != nil {
		t.Fatalf("SyncConfigBundle: %v", err)
	}
	if len(all.Suppliers) != 1 || len(all.SupplierItems) != 1 {
		t.Fatalf("supplier/supplier_item missing from the bundle: %+v", all)
	}
	if len(all.PurchaseOrders) != 1 || len(all.PurchaseOrderLines) != 1 {
		t.Fatalf("purchase_order/purchase_order_line missing from the bundle: %+v", all)
	}

	// Everything at or below the caller's mark is withheld.
	latest := repo.outletVersions[testOutletID]
	none, err := svc.SyncConfigBundle(context.Background(), testTenantID, testOutletID, latest)
	if err != nil {
		t.Fatalf("SyncConfigBundle: %v", err)
	}
	if len(none.Suppliers) != 0 || len(none.PurchaseOrders) != 0 {
		t.Fatalf("since_version=%d must withhold already-delivered rows: %+v", latest, none)
	}

	// And the slices are empty, never nil — a nil marshals to `null` and an
	// edge parsing `null` as an array is a crash nobody sees in Go tests.
	if none.Suppliers == nil || none.SupplierItems == nil || none.PurchaseOrders == nil || none.PurchaseOrderLines == nil {
		t.Error("empty bundles must carry empty slices, not nil")
	}
}

func TestSyncConfigBundle_RefusesAnotherTenantsOutlet(t *testing.T) {
	svc, _ := newTestService()
	if _, err := svc.SyncConfigBundle(context.Background(), otherTenantID, testOutletID, 0); !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("want ErrForbidden, got %v", err)
	}
}
