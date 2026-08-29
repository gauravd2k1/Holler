package procurement

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/outlet"
	contracts "github.com/holler/contracts"
)

// newTestRouter mounts the config routes behind a human principal carrying
// exactly perms, and the ingest routes behind a device principal — mirroring
// how backend/cmd/api/main.go splits auth.Authenticate from
// outlet.DeviceAuthenticate (ADR-017 0.4.3 amendment).
func newTestRouter(t *testing.T, perms ...contracts.Permission) (*chi.Mux, *fakeRepository) {
	t.Helper()
	svc, repo := newTestService()
	h := NewHandler(svc)

	principal := auth.AuthenticatedPrincipal{
		UserID: testUserID, TenantID: testTenantID, OutletID: testOutletID, Permissions: perms,
	}
	devicePrincipal := outlet.DevicePrincipal{
		DeviceID: testDeviceID, TenantID: testTenantID, OutletID: testOutletID,
	}

	r := chi.NewRouter()
	r.Use(func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
			ctx := auth.WithPrincipal(req.Context(), principal)
			ctx = outlet.WithDevicePrincipal(ctx, devicePrincipal)
			next.ServeHTTP(w, req.WithContext(ctx))
		})
	})
	h.Mount(r)
	h.MountIngest(r)
	return r, repo
}

func doPost(t *testing.T, r http.Handler, path string, body any) *httptest.ResponseRecorder {
	t.Helper()
	raw, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("marshalling request body: %v", err)
	}
	req := httptest.NewRequest(http.MethodPost, path, bytes.NewReader(raw))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	r.ServeHTTP(rec, req)
	return rec
}

func supplierBody() map[string]any {
	return map[string]any{
		"supplier": map[string]any{
			"id": testSupplierID, "outlet_id": testOutletID, "code": "ACME", "name": "Acme",
			"payment_terms_days": 0, "is_active": true,
		},
		"items": []any{},
	}
}

// TestRoutes_RefuseACallerWithoutProcurementManage is the permission half of
// "a permission with no check is a permission that does not exist". Every
// route procurement.manage names is exercised, with a caller holding a
// DIFFERENT real permission — not an empty principal, which would also fail
// for want of authentication and prove nothing about the gate.
func TestRoutes_RefuseACallerWithoutProcurementManage(t *testing.T) {
	r, repo := newTestRouter(t, auth.PermissionOutletManage)

	for _, tc := range []struct {
		name, path string
		body       any
	}{
		{"suppliers", "/procurement/suppliers", supplierBody()},
		{"purchase-orders", "/procurement/purchase-orders", map[string]any{"purchase_order": map[string]any{}}},
		{"supplier-invoices", "/procurement/supplier-invoices", map[string]any{}},
		{"supplier-credits", "/procurement/supplier-credits", map[string]any{}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			rec := doPost(t, r, tc.path, tc.body)
			if rec.Code != http.StatusForbidden {
				t.Fatalf("want 403 without procurement.manage, got %d: %s", rec.Code, rec.Body.String())
			}
		})
	}

	if len(repo.suppliers) != 0 || len(repo.purchaseOrders) != 0 {
		t.Fatalf("a refused request must write nothing: %+v %+v", repo.suppliers, repo.purchaseOrders)
	}
}

// TestApproveRoute_RefusesACallerWithoutProcurementApprove proves the approve
// route's gate is procurement.approve and NOT procurement.manage: whoever may
// raise an order must not thereby be able to approve it.
//
// THE ORDER IS SEEDED AND THE APPROVAL LIMIT IS GENEROUS ON PURPOSE. An
// unseeded order would 404 and a missing limit would 403 for the wrong reason
// — either way the test would go red if the gate were widened, but it would
// not be red BECAUSE of the gate, and a guard that fails for the wrong reason
// is not a guard. Here the only thing standing between this caller and a
// successful approval is the middleware, so the asserted code distinguishes
// the middleware refusal ("forbidden") from the service's own §64 refusal
// (po_approval_permission_missing).
func TestApproveRoute_RefusesACallerWithoutProcurementApprove(t *testing.T) {
	r, repo := newTestRouter(t, PermissionManage)

	if rec := doPost(t, r, "/procurement/suppliers", supplierBody()); rec.Code != http.StatusOK {
		t.Fatalf("seeding supplier: %d %s", rec.Code, rec.Body.String())
	}
	poBody := map[string]any{"purchase_order": map[string]any{
		"id": testPurchaseOrder, "outlet_id": testOutletID, "supplier_id": testSupplierID,
		"po_number": "PO-1", "status": "PENDING_APPROVAL", "total_paise": 100, "lines": []any{},
	}}
	if rec := doPost(t, r, "/procurement/purchase-orders", poBody); rec.Code != http.StatusOK {
		t.Fatalf("seeding purchase order: %d %s", rec.Code, rec.Body.String())
	}
	if _, ok := repo.purchaseOrders[testPurchaseOrder]; !ok {
		t.Fatal("purchase order fixture did not insert")
	}
	repo.approvalLimits[testUserID] = ptrInt64(1_000_000_00)

	rec := doPost(t, r, "/procurement/purchase-orders/"+testPurchaseOrder+"/approve", nil)
	if rec.Code != http.StatusForbidden {
		t.Fatalf("procurement.manage must not approve: got %d: %s", rec.Code, rec.Body.String())
	}
	var body errorEnvelopeBody
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decoding refusal body: %v", err)
	}
	if body.Code != "forbidden" {
		t.Fatalf("the ROUTE MIDDLEWARE must be what refuses this caller (code %q), got %q — "+
			"a different code means the request reached the handler and the gate is wrong",
			"forbidden", body.Code)
	}
	if repo.purchaseOrders[testPurchaseOrder].ApprovedByUserID != nil {
		t.Error("a refused approval must leave the row untouched")
	}
}

// TestApproveRoute_OverLimitRefusalCarriesTheNumbers is acceptance criterion
// 5's wire shape. The 403 body must carry the total, the ceiling and the next
// action — the admin UI renders these, and a bare "Forbidden" leaves a buyer
// with a delivery due and nothing to act on (§64).
func TestApproveRoute_OverLimitRefusalCarriesTheNumbers(t *testing.T) {
	r, repo := newTestRouter(t, PermissionManage, PermissionApprove)

	if rec := doPost(t, r, "/procurement/suppliers", supplierBody()); rec.Code != http.StatusOK {
		t.Fatalf("seeding supplier: %d %s", rec.Code, rec.Body.String())
	}
	const total = 250_000_00
	poBody := map[string]any{"purchase_order": map[string]any{
		"id": testPurchaseOrder, "outlet_id": testOutletID, "supplier_id": testSupplierID,
		"po_number": "PO-1", "status": "PENDING_APPROVAL", "total_paise": total, "lines": []any{},
	}}
	if rec := doPost(t, r, "/procurement/purchase-orders", poBody); rec.Code != http.StatusOK {
		t.Fatalf("seeding purchase order: %d %s", rec.Code, rec.Body.String())
	}
	// Fixtures actually inserted before anything is asserted about them.
	if _, ok := repo.purchaseOrders[testPurchaseOrder]; !ok {
		t.Fatal("purchase order fixture did not insert")
	}
	repo.approvalLimits[testUserID] = ptrInt64(50_000_00)
	repo.rolesAbleApproveN[total] = []string{"Finance Director"}

	rec := doPost(t, r, "/procurement/purchase-orders/"+testPurchaseOrder+"/approve", nil)
	if rec.Code != http.StatusForbidden {
		t.Fatalf("want 403, got %d: %s", rec.Code, rec.Body.String())
	}
	var body approvalRefusalBody
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decoding refusal body: %v (%s)", err, rec.Body.String())
	}
	if body.Code != approvalRefusalCodeOverLimit {
		t.Errorf("want code %q, got %q", approvalRefusalCodeOverLimit, body.Code)
	}
	if body.TotalPaise != total {
		t.Errorf("want total_paise %d, got %d", total, body.TotalPaise)
	}
	if body.LimitPaise == nil || *body.LimitPaise != 50_000_00 {
		t.Errorf("want the caller's ceiling on the wire, got %v", body.LimitPaise)
	}
	if len(body.Alternatives) != 1 || body.Alternatives[0] != "Finance Director" {
		t.Errorf("§64: the body must name who can approve instead, got %v", body.Alternatives)
	}
	if body.Message == "" {
		t.Error("the message must not be empty — it is what the admin UI shows")
	}
}

// TestIngestGoodsReceipts_RejectsAnAggregateOutsideTheRoutesSet pins the
// two-type set with a 422. Anything outside {goods_receipt_note, grn_gap} is a
// route mismatch, never coerced onto one of the two shapes.
func TestIngestGoodsReceipts_RejectsAnAggregateOutsideTheRoutesSet(t *testing.T) {
	r, _ := newTestRouter(t)
	env := map[string]any{
		"record_id": "x-1", "tenant_id": testTenantID, "outlet_id": testOutletID,
		"device_id": testDeviceID, "aggregate_type": string(contracts.AggregateTypeInvoice),
		"direction": string(contracts.SyncDirectionEdgeToCloud), "created_at": "2026-08-29T10:00:00Z",
		"updated_at": "2026-08-29T10:00:00Z", "version": 1, "sync_status": "PENDING",
		"payload": map[string]any{"id": "x-1"},
	}
	rec := doPost(t, r, "/procurement/goods-receipts", env)
	if rec.Code != http.StatusUnprocessableEntity {
		t.Fatalf("want 422 for an aggregate outside the route's set, got %d: %s", rec.Code, rec.Body.String())
	}
	var body errorEnvelopeBody
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decoding 422 body: %v", err)
	}
	if body.Code != "envelope_route_mismatch" {
		t.Errorf("want envelope_route_mismatch, got %q", body.Code)
	}
}

// TestIngestGoodsReceipts_AcceptsBothDeclaredAggregateTypes is the positive
// half: BOTH members of the set land on this one route, because a gap belongs
// beside the receipt it explains.
func TestIngestGoodsReceipts_AcceptsBothDeclaredAggregateTypes(t *testing.T) {
	r, repo := newTestRouter(t)

	grn := grnFixture()
	receiptEnv := map[string]any{
		"record_id": grn.ID, "tenant_id": testTenantID, "outlet_id": testOutletID,
		"device_id": testDeviceID, "aggregate_type": string(contracts.AggregateTypeGoodsReceiptNote),
		"direction": string(contracts.SyncDirectionEdgeToCloud), "created_at": "2026-08-29T10:00:00Z",
		"updated_at": "2026-08-29T10:00:00Z", "version": 1, "sync_status": "PENDING", "payload": grn,
	}
	if rec := doPost(t, r, "/procurement/goods-receipts", receiptEnv); rec.Code != http.StatusCreated {
		t.Fatalf("want 201 for a goods_receipt_note, got %d: %s", rec.Code, rec.Body.String())
	}

	detail := "no purchase order was quoted on the delivery note"
	gap := GrnGap{
		ID: "gap-1", OutletID: testOutletID, GrnID: grn.ID,
		Reason: contracts.GrnGapReasonNoPurchaseOrder, Detail: &detail,
		OccurredAt: "2026-08-29T10:00:00Z", BusinessDate: "2026-08-29",
	}
	gapEnv := map[string]any{
		"record_id": gap.ID, "tenant_id": testTenantID, "outlet_id": testOutletID,
		"device_id": testDeviceID, "aggregate_type": string(contracts.AggregateTypeGrnGap),
		"direction": string(contracts.SyncDirectionEdgeToCloud), "created_at": "2026-08-29T10:00:00Z",
		"updated_at": "2026-08-29T10:00:00Z", "version": 1, "sync_status": "PENDING", "payload": gap,
	}
	if rec := doPost(t, r, "/procurement/goods-receipts", gapEnv); rec.Code != http.StatusCreated {
		t.Fatalf("want 201 for a grn_gap on the same route, got %d: %s", rec.Code, rec.Body.String())
	}

	if len(repo.receipts) != 1 || len(repo.gaps) != 1 {
		t.Fatalf("both rows must have inserted: receipts=%d gaps=%d", len(repo.receipts), len(repo.gaps))
	}
}

// TestCreateSupplier_DimensionMismatchIs422 pins the status the OpenAPI route
// summary declares: a mismatch is REJECTED (422), distinct from the plain 400
// an ordinary invalid input gets, and never converted.
func TestCreateSupplier_DimensionMismatchIs422(t *testing.T) {
	r, _ := newTestRouter(t, PermissionManage)
	body := supplierBody()
	body["items"] = []any{map[string]any{
		"id": "item-1", "supplier_id": testSupplierID, "inventory_item_id": countItemID,
		"purchase_unit": "sack", "pack_size_micro": 50000000,
		"quantity_dimension": string(DimensionMass), "is_preferred": false,
	}}
	rec := doPost(t, r, "/procurement/suppliers", body)
	if rec.Code != http.StatusUnprocessableEntity {
		t.Fatalf("want 422 for a dimension mismatch, got %d: %s", rec.Code, rec.Body.String())
	}
	var errBody errorEnvelopeBody
	if err := json.Unmarshal(rec.Body.Bytes(), &errBody); err != nil {
		t.Fatalf("decoding 422 body: %v", err)
	}
	if errBody.Code != "dimension_mismatch" {
		t.Errorf("want dimension_mismatch, got %q", errBody.Code)
	}
}

// --- the list / update / amend routes ---------------------------------------

func doGet(t *testing.T, r http.Handler, path string) *httptest.ResponseRecorder {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, path, nil)
	rec := httptest.NewRecorder()
	r.ServeHTTP(rec, req)
	return rec
}

func doPatch(t *testing.T, r http.Handler, path string, body any) *httptest.ResponseRecorder {
	t.Helper()
	raw, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("marshalling request body: %v", err)
	}
	req := httptest.NewRequest(http.MethodPatch, path, bytes.NewReader(raw))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	r.ServeHTTP(rec, req)
	return rec
}

// TestListAndAmendRoutes_RefuseACallerWithoutProcurementManage extends "a
// permission with no check is a permission that does not exist" to the four
// routes this track adds. The caller holds a DIFFERENT real permission, not an
// empty principal — an empty principal would fail for want of authentication
// and prove nothing about the gate.
//
// The read routes are gated too, deliberately: a supplier price list is
// commercially sensitive and a purchase order list is the outlet's spend.
func TestListAndAmendRoutes_RefuseACallerWithoutProcurementManage(t *testing.T) {
	r, repo := newTestRouter(t, auth.PermissionOutletManage)

	gets := []string{"/procurement/suppliers", "/procurement/purchase-orders"}
	for _, path := range gets {
		t.Run("GET "+path, func(t *testing.T) {
			if rec := doGet(t, r, path); rec.Code != http.StatusForbidden {
				t.Fatalf("want 403 without procurement.manage, got %d: %s", rec.Code, rec.Body.String())
			}
		})
	}

	patches := []struct{ name, path string }{
		{"supplier", "/procurement/suppliers/" + testSupplierID},
		{"purchase-order", "/procurement/purchase-orders/" + testPurchaseOrder},
	}
	for _, tc := range patches {
		t.Run("PATCH "+tc.name, func(t *testing.T) {
			rec := doPatch(t, r, tc.path, map[string]any{})
			if rec.Code != http.StatusForbidden {
				t.Fatalf("want 403 without procurement.manage, got %d: %s", rec.Code, rec.Body.String())
			}
		})
	}

	if len(repo.suppliers) != 0 || len(repo.purchaseOrders) != 0 {
		t.Fatalf("a refused request must write nothing: %+v %+v", repo.suppliers, repo.purchaseOrders)
	}
}

// TestListSuppliersRoute_ReturnsThePriceListForPrefilling is the wire shape a
// caller needs to raise a purchase order without a second round trip:
// suppliers with their items, each item carrying last_price_paise.
func TestListSuppliersRoute_ReturnsThePriceListForPrefilling(t *testing.T) {
	r, repo := newTestRouter(t, PermissionManage)

	price := int64(120000)
	body := supplierBody()
	body["items"] = []any{map[string]any{
		"id": "bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb", "supplier_id": testSupplierID,
		"inventory_item_id": massItemID, "purchase_unit": "50kg sack",
		"pack_size_micro": 50_000_000, "quantity_dimension": string(DimensionMass),
		"last_price_paise": price, "is_preferred": true,
	}}
	if rec := doPost(t, r, "/procurement/suppliers", body); rec.Code != http.StatusOK {
		t.Fatalf("seeding supplier: %d %s", rec.Code, rec.Body.String())
	}
	if _, ok := repo.suppliers[testSupplierID]; !ok {
		t.Fatal("supplier fixture did not insert; every assertion below would pass on absent data")
	}
	if len(repo.supplierItems[testSupplierID]) != 1 {
		t.Fatalf("supplier_item fixture did not insert: %+v", repo.supplierItems[testSupplierID])
	}

	rec := doGet(t, r, "/procurement/suppliers")
	if rec.Code != http.StatusOK {
		t.Fatalf("GET /procurement/suppliers: %d %s", rec.Code, rec.Body.String())
	}
	var got listSuppliersResponse
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil {
		t.Fatalf("decoding: %v", err)
	}
	if len(got.Suppliers) != 1 || got.Suppliers[0].ID != testSupplierID {
		t.Fatalf("want the seeded supplier, got %+v", got.Suppliers)
	}
	if len(got.Suppliers[0].Items) != 1 {
		t.Fatalf("the price list must travel with the supplier: %+v", got.Suppliers[0])
	}
	if lp := got.Suppliers[0].Items[0].LastPricePaise; lp == nil || *lp != price {
		t.Errorf("last_price_paise is what prefills a PO line, got %v", lp)
	}
}

// TestAmendRoute_RevokesTheApprovalOnTheWire is the response-shape half of the
// amend decision. An admin screen reads approved_by_user_id off this body to
// decide whether to show "needs approval"; if it came back still populated the
// screen would say the order was authorised at a figure nobody authorised.
func TestAmendRoute_RevokesTheApprovalOnTheWire(t *testing.T) {
	r, repo := newTestRouter(t, PermissionManage, PermissionApprove)

	if rec := doPost(t, r, "/procurement/suppliers", supplierBody()); rec.Code != http.StatusOK {
		t.Fatalf("seeding supplier: %d %s", rec.Code, rec.Body.String())
	}
	const small = 5_000_00
	poBody := map[string]any{"purchase_order": map[string]any{
		"id": testPurchaseOrder, "outlet_id": testOutletID, "supplier_id": testSupplierID,
		"po_number": "PO-1", "status": "PENDING_APPROVAL", "total_paise": small, "lines": []any{},
	}}
	if rec := doPost(t, r, "/procurement/purchase-orders", poBody); rec.Code != http.StatusOK {
		t.Fatalf("seeding purchase order: %d %s", rec.Code, rec.Body.String())
	}
	repo.approvalLimits[testUserID] = ptrInt64(10_000_00)
	if rec := doPost(t, r, "/procurement/purchase-orders/"+testPurchaseOrder+"/approve", nil); rec.Code != http.StatusOK {
		t.Fatalf("approving: %d %s", rec.Code, rec.Body.String())
	}
	if repo.purchaseOrders[testPurchaseOrder].ApprovedByUserID == nil {
		t.Fatal("the approval fixture did not take; the amend assertion below would prove nothing")
	}

	rec := doPatch(t, r, "/procurement/purchase-orders/"+testPurchaseOrder, map[string]any{
		"purchase_order": map[string]any{"total_paise": 50_000_00, "lines": []any{}},
	})
	if rec.Code != http.StatusOK {
		t.Fatalf("PATCH purchase order: %d %s", rec.Code, rec.Body.String())
	}
	var amended PurchaseOrder
	if err := json.Unmarshal(rec.Body.Bytes(), &amended); err != nil {
		t.Fatalf("decoding: %v", err)
	}
	if amended.ApprovedByUserID != nil || amended.ApprovedAt != nil {
		t.Errorf("the amend response must show the approval revoked: %+v", amended)
	}
	if amended.Status != PurchaseOrderStatusPendingApproval {
		t.Errorf("want PENDING_APPROVAL after an amend, got %s", amended.Status)
	}
	if stored := repo.purchaseOrders[testPurchaseOrder]; stored.ApprovedByUserID != nil || stored.TotalPaise != 50_000_00 {
		t.Errorf("stored row after the amend: %+v", stored)
	}
}

// TestListPurchaseOrdersRoute_RejectsAnUnknownStatusRatherThanReturningNothing
// pins the filter's failure mode. A typo that returned an empty list would be
// read as "there are no such orders" and acted on.
func TestListPurchaseOrdersRoute_RejectsAnUnknownStatusRatherThanReturningNothing(t *testing.T) {
	r, _ := newTestRouter(t, PermissionManage)

	rec := doGet(t, r, "/procurement/purchase-orders?status=PENDNIG")
	if rec.Code != http.StatusBadRequest && rec.Code != http.StatusUnprocessableEntity {
		t.Fatalf("an unknown status must be an error, got %d: %s", rec.Code, rec.Body.String())
	}

	if rec := doGet(t, r, "/procurement/purchase-orders?limit=0"); rec.Code == http.StatusOK {
		t.Errorf("limit=0 must be rejected, not silently defaulted: %s", rec.Body.String())
	}
}
