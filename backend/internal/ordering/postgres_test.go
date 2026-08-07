package ordering_test

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/holler/backend/internal/menu"
	"github.com/holler/backend/internal/ordering"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/tenant"
	contracts "github.com/holler/contracts"
)

func setupPool(t *testing.T) postgres.Pool {
	t.Helper()

	dbURL := os.Getenv("HOLLER_TEST_DATABASE_URL")
	if dbURL == "" {
		t.Skip("HOLLER_TEST_DATABASE_URL not set; skipping ordering Postgres integration test")
	}

	ctx := context.Background()
	pool, err := postgres.Open(ctx, dbURL)
	if err != nil {
		t.Fatalf("postgres.Open: %v", err)
	}
	t.Cleanup(pool.Close)

	contractsDir, err := filepath.Abs(filepath.Join("..", "..", "..", "packages", "contracts", "postgres"))
	if err != nil {
		t.Fatalf("resolving contracts dir: %v", err)
	}
	if err := postgres.Migrate(ctx, pool, contractsDir); err != nil {
		t.Fatalf("postgres.Migrate: %v", err)
	}
	return pool
}

// fixture creates a tenant/brand/outlet/category/menu_item chain so an
// order_item's menu_item_id foreign key resolves, and returns the pieces
// tests need.
type fixture struct {
	tenantID   string
	outletID   string
	menuItemID string
}

func newFixture(t *testing.T, pool postgres.Pool) fixture {
	t.Helper()
	ctx := context.Background()

	tenantSvc := tenant.NewService(tenant.NewPostgresRepository(pool))
	outletSvc := outlet.NewService(outlet.NewPostgresRepository(pool))
	menuSvc := menu.NewService(menu.NewRepository(pool))

	org, err := tenantSvc.CreateOrganisation(ctx, "Ordering Integration Org")
	if err != nil {
		t.Fatalf("CreateOrganisation: %v", err)
	}
	brand, err := tenantSvc.CreateBrand(ctx, org.ID, "Ordering Integration Brand")
	if err != nil {
		t.Fatalf("CreateBrand: %v", err)
	}
	out, err := outletSvc.CreateOutlet(ctx, outlet.Principal{TenantID: org.ID}, brand.ID, "Ordering Integration Outlet", "")
	if err != nil {
		t.Fatalf("CreateOutlet: %v", err)
	}
	category, err := menuSvc.CreateCategory(ctx, menu.NewCategoryInput{OutletID: out.ID, Name: "Mains", SortOrder: 1})
	if err != nil {
		t.Fatalf("CreateCategory: %v", err)
	}
	item, _, _, err := menuSvc.CreateItem(ctx, menu.NewItemInput{
		OutletID:       out.ID,
		CategoryID:     category.ID,
		Name:           "Butter Chicken",
		BasePricePaise: 32000,
	})
	if err != nil {
		t.Fatalf("CreateItem: %v", err)
	}

	return fixture{tenantID: org.ID, outletID: out.ID, menuItemID: item.ID}
}

func envelopeFor(recordID, tenantID, outletID string, version int) contracts.SyncEnvelope {
	now := time.Now().UTC()
	return contracts.SyncEnvelope{
		RecordID:      recordID,
		TenantID:      tenantID,
		OutletID:      outletID,
		DeviceID:      "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa",
		AggregateType: contracts.AggregateTypeOrder,
		Direction:     contracts.SyncDirectionEdgeToCloud,
		CreatedAt:     now,
		UpdatedAt:     now,
		Version:       version,
		SyncStatus:    contracts.SyncStatusPending,
	}
}

func orderFor(orderID, outletID string) contracts.CanonicalOrder {
	now := time.Now().UTC()
	return contracts.CanonicalOrder{
		HollerOrderID: orderID,
		Source:        contracts.OrderSourcePOS,
		OutletID:      outletID,
		OrderType:     contracts.OrderTypeDineIn,
		Status:        contracts.OrderStatusDraft,
		Items:         []contracts.OrderItem{},
		SubtotalPaise: 0,
		TotalPaise:    0,
		PaymentStatus: contracts.PaymentStatusUnpaid,
		Timestamps:    contracts.OrderTimestamps{CreatedAt: now, UpdatedAt: now},
		SchemaVersion: 1,
	}
}

// TestPostgresRepository_DuplicateOrderEnvelopeIsIdempotent replays the
// identical order-creation envelope twice against a real Postgres and
// asserts exactly one row lands in "order" — the mandatory idempotency
// proof against the real schema, not just the in-memory fake.
func TestPostgresRepository_DuplicateOrderEnvelopeIsIdempotent(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)
	svc := ordering.NewService(ordering.NewPostgresRepository(pool))

	orderID := "bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb"
	env := envelopeFor(orderID, fx.tenantID, fx.outletID, 1)
	order := orderFor(orderID, fx.outletID)

	if _, err := svc.IngestOrder(context.Background(), fx.tenantID, env, order); err != nil {
		t.Fatalf("first IngestOrder: %v", err)
	}
	if _, err := svc.IngestOrder(context.Background(), fx.tenantID, env, order); err != nil {
		t.Fatalf("duplicate IngestOrder: %v", err)
	}

	var count int
	if err := pool.QueryRow(context.Background(), `SELECT count(*) FROM "order" WHERE id = $1`, orderID).Scan(&count); err != nil {
		t.Fatalf("counting order rows: %v", err)
	}
	if count != 1 {
		t.Fatalf("expected exactly 1 order row, got %d", count)
	}
}

// TestPostgresRepository_DuplicateItemAppendIsIdempotent proves the
// order_item append path is idempotent and append-only against the real
// schema: replaying the same item envelope twice must not duplicate the
// line item, and money assertions stay in integer paise throughout.
func TestPostgresRepository_DuplicateItemAppendIsIdempotent(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)
	svc := ordering.NewService(ordering.NewPostgresRepository(pool))

	orderID := "cccccccc-cccc-7ccc-8ccc-cccccccccccc"
	if _, err := svc.IngestOrder(context.Background(), fx.tenantID, envelopeFor(orderID, fx.tenantID, fx.outletID, 1), orderFor(orderID, fx.outletID)); err != nil {
		t.Fatalf("IngestOrder: %v", err)
	}

	itemID := "dddddddd-dddd-7ddd-8ddd-dddddddddddd"
	item := contracts.OrderItem{
		ID:             itemID,
		MenuItemID:     fx.menuItemID,
		Quantity:       3,
		UnitPricePaise: 32000,
		LineTotalPaise: 96000,
	}
	itemEnv := envelopeFor(orderID, fx.tenantID, fx.outletID, 1)

	if _, err := svc.AppendItem(context.Background(), fx.tenantID, itemEnv, orderID, item); err != nil {
		t.Fatalf("first AppendItem: %v", err)
	}
	stored, err := svc.AppendItem(context.Background(), fx.tenantID, itemEnv, orderID, item)
	if err != nil {
		t.Fatalf("duplicate AppendItem: %v", err)
	}

	if len(stored.Items) != 1 {
		t.Fatalf("expected exactly 1 line item after duplicate replay, got %d", len(stored.Items))
	}
	if stored.Items[0].LineTotalPaise != 96000 {
		t.Fatalf("expected line total 96000 paise, got %d", stored.Items[0].LineTotalPaise)
	}

	var count int
	if err := pool.QueryRow(context.Background(), `SELECT count(*) FROM order_item WHERE id = $1`, itemID).Scan(&count); err != nil {
		t.Fatalf("counting order_item rows: %v", err)
	}
	if count != 1 {
		t.Fatalf("expected exactly 1 order_item row, got %d", count)
	}
}

// TestPostgresRepository_CrossTenantOrderLookupIsNotFound mirrors
// internal/outlet's dedicated tenant-isolation test: tenant A's principal
// with tenant B's order id must 404.
func TestPostgresRepository_CrossTenantOrderLookupIsNotFound(t *testing.T) {
	pool := setupPool(t)
	fxA := newFixture(t, pool)
	fxB := newFixture(t, pool)
	svc := ordering.NewService(ordering.NewPostgresRepository(pool))

	orderID := "eeeeeeee-eeee-7eee-8eee-eeeeeeeeeeee"
	if _, err := svc.IngestOrder(context.Background(), fxB.tenantID, envelopeFor(orderID, fxB.tenantID, fxB.outletID, 1), orderFor(orderID, fxB.outletID)); err != nil {
		t.Fatalf("IngestOrder for tenant B: %v", err)
	}

	if _, err := svc.GetOrder(context.Background(), fxA.tenantID, orderID); err == nil {
		t.Fatal("expected tenant A's lookup of tenant B's order to fail")
	}
}
