package ordering_test

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/menu"
	"github.com/holler/backend/internal/ordering"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/id"
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
	// menu.Service.CreateCategory/CreateItem require a menu.Principal with
	// menu.manage in context (its own context key, distinct from
	// auth.WithPrincipal — see backend/cmd/api/principal.go's
	// bridgeDownstreamPrincipals, which does this same wrap in production).
	// The bare context.Background() used above for tenant/outlet setup has
	// none.
	menuCtx := menu.WithPrincipal(ctx, auth.NewPrincipal(auth.AuthenticatedPrincipal{
		UserID:      "principal-user",
		TenantID:    org.ID,
		OutletID:    out.ID,
		Permissions: []auth.Permission{auth.PermissionMenuManage},
	}))
	category, err := menuSvc.CreateCategory(menuCtx, menu.NewCategoryInput{OutletID: out.ID, Name: "Mains", SortOrder: 1})
	if err != nil {
		t.Fatalf("CreateCategory: %v", err)
	}
	item, _, _, err := menuSvc.CreateItem(menuCtx, menu.NewItemInput{
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

	// id.New() rather than a fixed literal: this suite's rows are never
	// cleaned up and the same live Postgres is shared with backend/internal/
	// kitchen's postgres_test.go (which seeds its own "order" row), so a
	// fixed literal risks colliding across packages and across repeated runs
	// of this suite against the same database.
	orderID := id.New()
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

	orderID := id.New()
	if _, err := svc.IngestOrder(context.Background(), fx.tenantID, envelopeFor(orderID, fx.tenantID, fx.outletID, 1), orderFor(orderID, fx.outletID)); err != nil {
		t.Fatalf("IngestOrder: %v", err)
	}

	itemID := id.New()
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

// TestPostgresRepository_OrderRoundTripPersistsContractsV024Fields proves the
// 0.2.4 columns (source, external_order_id, payment_status, payment_source,
// confirmed_at, schema_version, taxes_paise) survive an ingest -> read
// round trip through real Postgres, and pins the still-deferred wire fields
// (packaging_paise, delivery_charge_paise, aggregator_discount_paise,
// merchant_discount_paise, customer, delivery_address, rider,
// preparation_time_minutes) to their exact synthesized values per the
// ADR-011 0.2.4 addendum's deferred-columns table. When a later milestone
// starts persisting one of the deferred fields, this test must fail so the
// ADR table gets updated rather than drifting quietly.
func TestPostgresRepository_OrderRoundTripPersistsContractsV024Fields(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)
	svc := ordering.NewService(ordering.NewPostgresRepository(pool))

	orderID := id.New()
	externalOrderID := "zomato-order-42"
	paymentSource := "cash"
	env := envelopeFor(orderID, fx.tenantID, fx.outletID, 1)
	order := orderFor(orderID, fx.outletID)
	order.ExternalOrderID = &externalOrderID
	order.PaymentSource = &paymentSource
	order.TaxesPaise = 1234
	confirmedAt := time.Now().UTC().Truncate(time.Microsecond)
	order.Status = contracts.OrderStatusConfirmed
	order.Timestamps.ConfirmedAt = &confirmedAt

	stored, err := svc.IngestOrder(context.Background(), fx.tenantID, env, order)
	if err != nil {
		t.Fatalf("IngestOrder: %v", err)
	}

	// Persisted 0.2.4 fields: what was replayed must be what comes back.
	if stored.Source != contracts.OrderSourcePOS {
		t.Fatalf("expected source POS, got %q", stored.Source)
	}
	if stored.ExternalOrderID == nil || *stored.ExternalOrderID != externalOrderID {
		t.Fatalf("expected external_order_id %q, got %v", externalOrderID, stored.ExternalOrderID)
	}
	if stored.PaymentStatus != contracts.PaymentStatusUnpaid {
		t.Fatalf("expected payment_status UNPAID, got %q", stored.PaymentStatus)
	}
	if stored.PaymentSource == nil || *stored.PaymentSource != paymentSource {
		t.Fatalf("expected payment_source %q, got %v", paymentSource, stored.PaymentSource)
	}
	if stored.TaxesPaise != 1234 {
		t.Fatalf("expected taxes_paise 1234, got %d", stored.TaxesPaise)
	}
	if stored.SchemaVersion != 1 {
		t.Fatalf("expected schema_version 1, got %d", stored.SchemaVersion)
	}
	if stored.Timestamps.ConfirmedAt == nil || !stored.Timestamps.ConfirmedAt.Equal(confirmedAt) {
		t.Fatalf("expected confirmed_at %v, got %v", confirmedAt, stored.Timestamps.ConfirmedAt)
	}

	// Re-fetch to prove this isn't just the insert echo — the same values
	// must be readable back from storage independently.
	reread, err := svc.GetOrder(context.Background(), fx.tenantID, orderID)
	if err != nil {
		t.Fatalf("GetOrder: %v", err)
	}
	if reread.TaxesPaise != 1234 {
		t.Fatalf("re-read: expected taxes_paise 1234, got %d", reread.TaxesPaise)
	}
	if reread.Timestamps.ConfirmedAt == nil || !reread.Timestamps.ConfirmedAt.Equal(confirmedAt) {
		t.Fatalf("re-read: expected confirmed_at %v, got %v", confirmedAt, reread.Timestamps.ConfirmedAt)
	}

	// Deferred fields — pinned by exact value, not by absence, per the
	// ADR-011 0.2.4 addendum's deferred-columns table.
	if reread.PackagingPaise != 0 {
		t.Fatalf("packaging_paise: expected synthesized 0, got %d", reread.PackagingPaise)
	}
	if reread.DeliveryChargePaise != 0 {
		t.Fatalf("delivery_charge_paise: expected synthesized 0, got %d", reread.DeliveryChargePaise)
	}
	if reread.AggregatorDiscountPaise != 0 {
		t.Fatalf("aggregator_discount_paise: expected synthesized 0, got %d", reread.AggregatorDiscountPaise)
	}
	if reread.MerchantDiscountPaise != 0 {
		t.Fatalf("merchant_discount_paise: expected synthesized 0, got %d", reread.MerchantDiscountPaise)
	}
	if reread.Customer != nil {
		t.Fatalf("customer: expected synthesized nil, got %+v", reread.Customer)
	}
	if reread.DeliveryAddress != nil {
		t.Fatalf("delivery_address: expected synthesized nil, got %v", *reread.DeliveryAddress)
	}
	if reread.Rider != nil {
		t.Fatalf("rider: expected synthesized nil, got %+v", reread.Rider)
	}
	if reread.PreparationTimeMinutes != nil {
		t.Fatalf("preparation_time_minutes: expected synthesized nil, got %d", *reread.PreparationTimeMinutes)
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

	orderID := id.New()
	if _, err := svc.IngestOrder(context.Background(), fxB.tenantID, envelopeFor(orderID, fxB.tenantID, fxB.outletID, 1), orderFor(orderID, fxB.outletID)); err != nil {
		t.Fatalf("IngestOrder for tenant B: %v", err)
	}

	if _, err := svc.GetOrder(context.Background(), fxA.tenantID, orderID); err == nil {
		t.Fatal("expected tenant A's lookup of tenant B's order to fail")
	}
}
