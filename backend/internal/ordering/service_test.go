package ordering

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

// fakeRepo is an in-memory Repository used to test Service without a live
// Postgres, matching the pattern in internal/outlet's fakeRepo.
type fakeRepo struct {
	orders      map[string]StoredOrder // orderID -> row
	orderTenant map[string]string      // orderID -> tenantID (as if joined via outlet/brand)
	outletOK    map[string]bool        // outletID -> belongs to the tenant used in tests
	items       map[string][]contracts.OrderItem
	itemIDs     map[string]bool // global item id set, to mimic the item PK's idempotency
}

func newFakeRepo() *fakeRepo {
	return &fakeRepo{
		orders:      map[string]StoredOrder{},
		orderTenant: map[string]string{},
		outletOK:    map[string]bool{},
		items:       map[string][]contracts.OrderItem{},
		itemIDs:     map[string]bool{},
	}
}

func (f *fakeRepo) InsertOrder(ctx context.Context, tenantID, deviceID string, version int, order Order) (StoredOrder, bool, error) {
	if existing, ok := f.orders[order.HollerOrderID]; ok {
		return existing, false, nil
	}
	if !f.outletOK[order.OutletID] {
		return StoredOrder{}, false, httpx.ErrNotFound
	}
	order.Items = []contracts.OrderItem{}
	stored := StoredOrder{Order: order, Version: version}
	f.orders[order.HollerOrderID] = stored
	f.orderTenant[order.HollerOrderID] = tenantID
	return stored, true, nil
}

func (f *fakeRepo) GetByID(ctx context.Context, tenantID, orderID string) (StoredOrder, error) {
	stored, ok := f.orders[orderID]
	if !ok || f.orderTenant[orderID] != tenantID {
		return StoredOrder{}, httpx.ErrNotFound
	}
	stored.Items = append([]contracts.OrderItem{}, f.items[orderID]...)
	return stored, nil
}

func (f *fakeRepo) ListByOutlet(ctx context.Context, tenantID, outletID string) ([]StoredOrder, error) {
	var out []StoredOrder
	for id, stored := range f.orders {
		if f.orderTenant[id] == tenantID && stored.OutletID == outletID {
			stored.Items = append([]contracts.OrderItem{}, f.items[id]...)
			out = append(out, stored)
		}
	}
	return out, nil
}

func (f *fakeRepo) AppendItem(ctx context.Context, tenantID string, orderID string, item contracts.OrderItem) (bool, error) {
	if f.itemIDs[item.ID] {
		return false, nil
	}
	f.itemIDs[item.ID] = true
	f.items[orderID] = append(f.items[orderID], item)
	return true, nil
}

func (f *fakeRepo) ItemsForOrder(ctx context.Context, orderID string) ([]contracts.OrderItem, error) {
	return append([]contracts.OrderItem{}, f.items[orderID]...), nil
}

func (f *fakeRepo) UpdateStatus(ctx context.Context, tenantID, orderID string, expectedCurrentVersion, newVersion int, newStatus contracts.OrderStatus) (StoredOrder, bool, error) {
	stored, ok := f.orders[orderID]
	if !ok || f.orderTenant[orderID] != tenantID {
		return StoredOrder{}, false, httpx.ErrNotFound
	}
	if stored.Version != expectedCurrentVersion {
		stored.Items = append([]contracts.OrderItem{}, f.items[orderID]...)
		return stored, false, nil
	}
	stored.Status = newStatus
	stored.Version = newVersion
	f.orders[orderID] = stored
	stored.Items = append([]contracts.OrderItem{}, f.items[orderID]...)
	return stored, true, nil
}

const (
	testTenantID = "11111111-1111-7111-8111-111111111111"
	testOutletID = "22222222-2222-7222-8222-222222222222"
	testDeviceID = "33333333-3333-7333-8333-333333333333"
	testOrderID  = "44444444-4444-7444-8444-444444444444"
)

func baseEnvelope(version int) contracts.SyncEnvelope {
	now := time.Now().UTC()
	return contracts.SyncEnvelope{
		RecordID:      testOrderID,
		TenantID:      testTenantID,
		OutletID:      testOutletID,
		DeviceID:      testDeviceID,
		AggregateType: contracts.AggregateTypeOrder,
		Direction:     contracts.SyncDirectionEdgeToCloud,
		CreatedAt:     now,
		UpdatedAt:     now,
		Version:       version,
		SyncStatus:    contracts.SyncStatusPending,
	}
}

func baseOrder() contracts.CanonicalOrder {
	now := time.Now().UTC()
	return contracts.CanonicalOrder{
		HollerOrderID: testOrderID,
		Source:        contracts.OrderSourcePOS,
		OutletID:      testOutletID,
		OrderType:     contracts.OrderTypeDineIn,
		Status:        contracts.OrderStatusDraft,
		Items:         []contracts.OrderItem{},
		SubtotalPaise: 0,
		TotalPaise:    0,
		PaymentStatus: contracts.PaymentStatusUnpaid,
		Timestamps: contracts.OrderTimestamps{
			CreatedAt: now,
			UpdatedAt: now,
		},
		SchemaVersion: 1,
	}
}

func newTestService() (*Service, *fakeRepo) {
	repo := newFakeRepo()
	repo.outletOK[testOutletID] = true
	return NewService(repo), repo
}

func TestIngestOrder_CreatesOrder(t *testing.T) {
	svc, _ := newTestService()
	stored, err := svc.IngestOrder(context.Background(), testTenantID, baseEnvelope(1), baseOrder())
	if err != nil {
		t.Fatalf("IngestOrder: %v", err)
	}
	if stored.Status != contracts.OrderStatusDraft {
		t.Fatalf("expected DRAFT, got %s", stored.Status)
	}
	if stored.Version != 1 {
		t.Fatalf("expected version 1, got %d", stored.Version)
	}
}

// TestIngestOrder_DuplicateEnvelopeIsIdempotent replays the identical
// envelope twice and asserts exactly one order row results — the mandatory
// idempotency proof (docs/spec/sync.md §Idempotency).
func TestIngestOrder_DuplicateEnvelopeIsIdempotent(t *testing.T) {
	svc, repo := newTestService()
	env := baseEnvelope(1)
	order := baseOrder()

	if _, err := svc.IngestOrder(context.Background(), testTenantID, env, order); err != nil {
		t.Fatalf("first IngestOrder: %v", err)
	}
	if _, err := svc.IngestOrder(context.Background(), testTenantID, env, order); err != nil {
		t.Fatalf("second (duplicate) IngestOrder: %v", err)
	}

	if len(repo.orders) != 1 {
		t.Fatalf("expected exactly one order row after duplicate replay, got %d", len(repo.orders))
	}
}

func TestIngestOrder_RejectsCloudToEdgeDirection(t *testing.T) {
	svc, _ := newTestService()
	env := baseEnvelope(1)
	env.Direction = contracts.SyncDirectionCloudToEdge

	_, err := svc.IngestOrder(context.Background(), testTenantID, env, baseOrder())
	if !errors.Is(err, ErrAuthorityViolation) {
		t.Fatalf("expected ErrAuthorityViolation, got %v", err)
	}
}

func TestIngestOrder_RejectsIllegalCreationStatus(t *testing.T) {
	svc, _ := newTestService()
	order := baseOrder()
	order.Status = contracts.OrderStatusClosed

	_, err := svc.IngestOrder(context.Background(), testTenantID, baseEnvelope(1), order)
	if !errors.Is(err, ErrIllegalTransition) {
		t.Fatalf("expected ErrIllegalTransition, got %v", err)
	}
}

func TestIngestOrder_RejectsCrossTenantCaller(t *testing.T) {
	svc, _ := newTestService()
	otherTenant := "99999999-9999-7999-8999-999999999999"

	_, err := svc.IngestOrder(context.Background(), otherTenant, baseEnvelope(1), baseOrder())
	if !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("expected ErrForbidden for cross-tenant envelope, got %v", err)
	}
}

func TestAppendItem_DuplicateDeliveryIsIdempotentAndAppendOnly(t *testing.T) {
	svc, _ := newTestService()
	if _, err := svc.IngestOrder(context.Background(), testTenantID, baseEnvelope(1), baseOrder()); err != nil {
		t.Fatalf("IngestOrder: %v", err)
	}

	item := contracts.OrderItem{
		ID:             "55555555-5555-7555-8555-555555555555",
		MenuItemID:     "66666666-6666-7666-8666-666666666666",
		Quantity:       2,
		UnitPricePaise: 15000,
		LineTotalPaise: 30000,
	}

	itemEnv := baseEnvelope(1)
	stored, err := svc.AppendItem(context.Background(), testTenantID, itemEnv, testOrderID, item)
	if err != nil {
		t.Fatalf("AppendItem: %v", err)
	}
	if len(stored.Items) != 1 {
		t.Fatalf("expected 1 item, got %d", len(stored.Items))
	}
	if stored.Items[0].LineTotalPaise != 30000 {
		t.Fatalf("expected line total 30000 paise, got %d", stored.Items[0].LineTotalPaise)
	}

	// Replay the identical item envelope: must not duplicate the line item.
	stored, err = svc.AppendItem(context.Background(), testTenantID, itemEnv, testOrderID, item)
	if err != nil {
		t.Fatalf("AppendItem (duplicate): %v", err)
	}
	if len(stored.Items) != 1 {
		t.Fatalf("expected exactly 1 item after duplicate replay, got %d", len(stored.Items))
	}
}

func TestAppendItem_RejectedOnceOrderLeftDraft(t *testing.T) {
	svc, _ := newTestService()
	if _, err := svc.IngestOrder(context.Background(), testTenantID, baseEnvelope(1), baseOrder()); err != nil {
		t.Fatalf("IngestOrder: %v", err)
	}
	if _, err := svc.transition(context.Background(), testTenantID, baseEnvelope(2), testOrderID, contracts.OrderStatusConfirmed); err != nil {
		t.Fatalf("transition to CONFIRMED: %v", err)
	}

	item := contracts.OrderItem{
		ID:             "77777777-7777-7777-8777-777777777777",
		MenuItemID:     "88888888-8888-7888-8888-888888888888",
		Quantity:       1,
		UnitPricePaise: 10000,
		LineTotalPaise: 10000,
	}
	_, err := svc.AppendItem(context.Background(), testTenantID, baseEnvelope(3), testOrderID, item)
	if !errors.Is(err, httpx.ErrConflict) {
		t.Fatalf("expected ErrConflict appending to a non-DRAFT order, got %v", err)
	}
}

func TestSendToKitchen_LegalTransitionAndIdempotentReplay(t *testing.T) {
	svc, _ := newTestService()
	if _, err := svc.IngestOrder(context.Background(), testTenantID, baseEnvelope(1), baseOrder()); err != nil {
		t.Fatalf("IngestOrder: %v", err)
	}
	if _, err := svc.transition(context.Background(), testTenantID, baseEnvelope(2), testOrderID, contracts.OrderStatusConfirmed); err != nil {
		t.Fatalf("transition to CONFIRMED: %v", err)
	}

	env := baseEnvelope(3)
	stored, err := svc.SendToKitchen(context.Background(), testTenantID, env, testOrderID)
	if err != nil {
		t.Fatalf("SendToKitchen: %v", err)
	}
	if stored.Status != contracts.OrderStatusSentToKitchen {
		t.Fatalf("expected SENT_TO_KITCHEN, got %s", stored.Status)
	}

	// Replaying the identical envelope must not error and must not re-apply.
	stored, err = svc.SendToKitchen(context.Background(), testTenantID, env, testOrderID)
	if err != nil {
		t.Fatalf("SendToKitchen (duplicate replay): %v", err)
	}
	if stored.Status != contracts.OrderStatusSentToKitchen {
		t.Fatalf("expected order to remain SENT_TO_KITCHEN after duplicate replay, got %s", stored.Status)
	}
}

func TestSendToKitchen_RejectsIllegalTransitionFromDraft(t *testing.T) {
	svc, _ := newTestService()
	if _, err := svc.IngestOrder(context.Background(), testTenantID, baseEnvelope(1), baseOrder()); err != nil {
		t.Fatalf("IngestOrder: %v", err)
	}

	_, err := svc.SendToKitchen(context.Background(), testTenantID, baseEnvelope(2), testOrderID)
	if !errors.Is(err, ErrIllegalTransition) {
		t.Fatalf("expected ErrIllegalTransition moving DRAFT -> SENT_TO_KITCHEN directly, got %v", err)
	}
}

func TestCancel_RequiresReasonAndRejectsAfterClosed(t *testing.T) {
	svc, _ := newTestService()
	if _, err := svc.IngestOrder(context.Background(), testTenantID, baseEnvelope(1), baseOrder()); err != nil {
		t.Fatalf("IngestOrder: %v", err)
	}

	if _, err := svc.Cancel(context.Background(), testTenantID, baseEnvelope(2), testOrderID, ""); !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput for missing reason, got %v", err)
	}

	stored, err := svc.Cancel(context.Background(), testTenantID, baseEnvelope(2), testOrderID, "guest left")
	if err != nil {
		t.Fatalf("Cancel: %v", err)
	}
	if stored.Status != contracts.OrderStatusCancelled {
		t.Fatalf("expected CANCELLED, got %s", stored.Status)
	}

	if _, err := svc.Cancel(context.Background(), testTenantID, baseEnvelope(3), testOrderID, "again"); !errors.Is(err, ErrIllegalTransition) {
		t.Fatalf("expected ErrIllegalTransition cancelling an already-CANCELLED order, got %v", err)
	}
}

func TestGetOrder_TenantScoped_CrossTenantIsNotFound(t *testing.T) {
	svc, _ := newTestService()
	if _, err := svc.IngestOrder(context.Background(), testTenantID, baseEnvelope(1), baseOrder()); err != nil {
		t.Fatalf("IngestOrder: %v", err)
	}

	otherTenant := "99999999-9999-7999-8999-999999999999"
	if _, err := svc.GetOrder(context.Background(), otherTenant, testOrderID); !errors.Is(err, httpx.ErrNotFound) {
		t.Fatalf("expected ErrNotFound for cross-tenant order lookup, got %v", err)
	}

	if _, err := svc.GetOrder(context.Background(), testTenantID, testOrderID); err != nil {
		t.Fatalf("owning tenant should be able to fetch its own order: %v", err)
	}
}
