package payments_test

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/compliance"
	"github.com/holler/backend/internal/menu"
	"github.com/holler/backend/internal/ordering"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/payments"
	"github.com/holler/backend/internal/platform/id"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/tenant"
	contracts "github.com/holler/contracts"
)

// setupPool mirrors backend/internal/ordering/postgres_test.go's setupPool
// exactly: same env var gate, same migration path.
func setupPool(t *testing.T) postgres.Pool {
	t.Helper()

	dbURL := os.Getenv("HOLLER_TEST_DATABASE_URL")
	if dbURL == "" {
		t.Skip("HOLLER_TEST_DATABASE_URL not set; skipping payments Postgres integration test")
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

// fixture builds a tenant/brand/outlet/order/order_item chain (so
// invoice_line.order_item_id and invoice/payment.order_id resolve), an
// app_user (for created_by_user_id), and — through
// backend/internal/compliance.Service, the same write path a real outlet
// configures itself through, not raw SQL (T13) — the CLOUD_TO_EDGE config
// rows an invoice references: compliance_version, tax_profile, invoice_series.
type fixture struct {
	tenantID          string
	outletID          string
	orderID           string
	orderItemID       string
	userID            string
	seriesID          string
	complianceVersion string
	taxProfileID      string
}

func newFixture(t *testing.T, pool postgres.Pool) fixture {
	t.Helper()
	ctx := context.Background()

	tenantSvc := tenant.NewService(tenant.NewPostgresRepository(pool))
	outletSvc := outlet.NewService(outlet.NewPostgresRepository(pool))
	menuSvc := menu.NewService(menu.NewRepository(pool))
	orderingSvc := ordering.NewService(ordering.NewPostgresRepository(pool))

	org, err := tenantSvc.CreateOrganisation(ctx, "Payments Integration Org "+id.New())
	if err != nil {
		t.Fatalf("CreateOrganisation: %v", err)
	}
	brand, err := tenantSvc.CreateBrand(ctx, org.ID, "Payments Integration Brand")
	if err != nil {
		t.Fatalf("CreateBrand: %v", err)
	}
	out, err := outletSvc.CreateOutlet(ctx, outlet.Principal{TenantID: org.ID}, brand.ID, "Payments Integration Outlet", "")
	if err != nil {
		t.Fatalf("CreateOutlet: %v", err)
	}

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
		OutletID: out.ID, CategoryID: category.ID, Name: "Butter Chicken", BasePricePaise: 32000,
	})
	if err != nil {
		t.Fatalf("CreateItem: %v", err)
	}

	orderID := id.New()
	now := time.Now().UTC()
	order := contracts.CanonicalOrder{
		HollerOrderID: orderID,
		Source:        contracts.OrderSourcePOS,
		OutletID:      out.ID,
		OrderType:     contracts.OrderTypeDineIn,
		Status:        contracts.OrderStatusDraft,
		Items:         []contracts.OrderItem{},
		SubtotalPaise: 32000,
		TotalPaise:    32000,
		PaymentStatus: contracts.PaymentStatusUnpaid,
		Timestamps:    contracts.OrderTimestamps{CreatedAt: now, UpdatedAt: now},
		SchemaVersion: 1,
	}
	orderEnv := contracts.SyncEnvelope{
		RecordID: orderID, TenantID: org.ID, OutletID: out.ID, DeviceID: id.New(),
		AggregateType: contracts.AggregateTypeOrder, Direction: contracts.SyncDirectionEdgeToCloud,
		CreatedAt: now, UpdatedAt: now, Version: 1, SyncStatus: contracts.SyncStatusPending,
	}
	if _, err := orderingSvc.IngestOrder(ctx, org.ID, orderEnv, order); err != nil {
		t.Fatalf("IngestOrder: %v", err)
	}
	orderItemID := id.New()
	if _, err := orderingSvc.AppendItem(ctx, org.ID, orderEnv, orderID, contracts.OrderItem{
		ID: orderItemID, MenuItemID: item.ID, Quantity: 1, UnitPricePaise: 32000, LineTotalPaise: 32000,
	}); err != nil {
		t.Fatalf("AppendItem: %v", err)
	}

	userID := id.New()
	authRepo := auth.NewRepository(pool)
	if err := authRepo.CreateUser(ctx, userID, org.ID, "payments-fixture-"+userID+"@holler.test", "Fixture Cashier", "unused-hash", now); err != nil {
		t.Fatalf("CreateUser: %v", err)
	}

	complianceSvc := compliance.NewService(compliance.NewRepository(pool))
	configCtx := auth.WithPrincipal(ctx, auth.AuthenticatedPrincipal{
		UserID:      "principal-user",
		TenantID:    org.ID,
		OutletID:    out.ID,
		Permissions: []auth.Permission{auth.PermissionOutletManage},
	})

	cv, err := complianceSvc.CreateComplianceVersion(configCtx, org.ID, compliance.NewComplianceVersionInput{
		OutletID: out.ID, Label: "v1-" + id.New(), EffectiveFrom: now,
	})
	if err != nil {
		t.Fatalf("CreateComplianceVersion: %v", err)
	}

	taxProfile, _, err := complianceSvc.CreateTaxProfile(configCtx, org.ID, compliance.NewTaxProfileInput{
		OutletID: out.ID, Code: "GST5-" + id.New()[:8], Name: "GST 5%",
		PricingMode: contracts.PricingModeExclusive, IsDefault: true,
	})
	if err != nil {
		t.Fatalf("CreateTaxProfile: %v", err)
	}

	series, err := complianceSvc.CreateInvoiceSeries(configCtx, org.ID, compliance.NewInvoiceSeriesInput{
		OutletID: out.ID, Code: "MAIN-" + id.New()[:8], PrefixTemplate: "FY{FY}/{OUTLET}/",
		ResetPolicy: contracts.SequenceResetFY, PaddingWidth: 6,
	})
	if err != nil {
		t.Fatalf("CreateInvoiceSeries: %v", err)
	}

	return fixture{
		tenantID: org.ID, outletID: out.ID, orderID: orderID, orderItemID: orderItemID,
		userID: userID, seriesID: series.ID, complianceVersion: cv.ID, taxProfileID: taxProfile.ID,
	}
}

func envelope(aggregate contracts.AggregateType, recordID, tenantID, outletID string, version int) contracts.SyncEnvelope {
	now := time.Now().UTC()
	return contracts.SyncEnvelope{
		RecordID: recordID, TenantID: tenantID, OutletID: outletID, DeviceID: id.New(),
		AggregateType: aggregate, Direction: contracts.SyncDirectionEdgeToCloud,
		CreatedAt: now, UpdatedAt: now, Version: version, SyncStatus: contracts.SyncStatusPending,
	}
}

func invoiceFor(fx fixture, invoiceID, number string) payments.Invoice {
	now := time.Now().UTC()
	return payments.Invoice{
		ID:                     invoiceID,
		OutletID:               fx.outletID,
		OrderID:                fx.orderID,
		SplitIndex:             1,
		SplitCount:             1,
		SeriesID:               fx.seriesID,
		InvoiceNumber:          number,
		InvoiceDate:            now,
		BusinessDate:           "2026-08-14",
		Status:                 contracts.InvoiceStatusIssued,
		PlaceOfSupplyStateCode: "27",
		Lines: []contracts.InvoiceLine{{
			ID: id.New(), InvoiceID: invoiceID, OrderItemID: fx.orderItemID, LineNo: 1,
			Description: "Butter Chicken", Quantity: 1, UnitPricePaise: 32000, GrossPaise: 32000,
			TaxableValuePaise: 32000, TaxProfileID: fx.taxProfileID,
			CGSTRateBps: 250, CGSTPaise: 800, SGSTRateBps: 250, SGSTPaise: 800, TotalPaise: 33600,
			SchemaVersion: 1,
		}},
		SubtotalPaise:       32000,
		TaxableValuePaise:   32000,
		CGSTPaise:           800,
		SGSTPaise:           800,
		RoundOffPaise:       0,
		GrandTotalPaise:     33600,
		ComplianceVersionID: fx.complianceVersion,
		TaxSnapshot:         map[string]interface{}{"rate": "5%"},
		FiscalProfile:       map[string]interface{}{"gstin": "27ABCDE1234F1Z5"},
		Channel:             "DIRECT",
		TaxLiabilityParty:   contracts.TaxLiabilityRestaurant,
		CreatedByUserID:     fx.userID,
		CreatedAt:           now,
		UpdatedAt:           now,
		Version:             1,
		SchemaVersion:       1,
	}
}

// TestIngestInvoice_DuplicateEnvelopeIsIdempotent replays the identical
// invoice envelope twice against real Postgres and asserts exactly one row
// lands, mirroring backend/internal/ordering's own Postgres idempotency
// proof (§25).
func TestIngestInvoice_DuplicateEnvelopeIsIdempotent(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)
	svc := payments.NewService(payments.NewPostgresRepository(pool))

	invoiceID := id.New()
	inv := invoiceFor(fx, invoiceID, "FY26/PNQ/"+invoiceID[:6])
	env := envelope(contracts.AggregateTypeInvoice, invoiceID, fx.tenantID, fx.outletID, 1)

	if _, err := svc.IngestInvoice(context.Background(), fx.tenantID, fx.outletID, env, inv); err != nil {
		t.Fatalf("first IngestInvoice: %v", err)
	}
	if _, err := svc.IngestInvoice(context.Background(), fx.tenantID, fx.outletID, env, inv); err != nil {
		t.Fatalf("duplicate IngestInvoice: %v", err)
	}

	var count int
	if err := pool.QueryRow(context.Background(), `SELECT count(*) FROM invoice WHERE id = $1`, invoiceID).Scan(&count); err != nil {
		t.Fatalf("counting invoice rows: %v", err)
	}
	if count != 1 {
		t.Fatalf("expected exactly 1 invoice row after replay, got %d", count)
	}
}

// TestIngestInvoice_DuplicateNumberDifferentIDIs409 proves the real unique
// index (outlet_id, series_id, invoice_number) rejects a genuinely different
// invoice id reusing an issued number, mapped to httpx.ErrConflict rather
// than a raw driver error reaching the caller.
func TestIngestInvoice_DuplicateNumberDifferentIDIs409(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)
	svc := payments.NewService(payments.NewPostgresRepository(pool))

	number := "FY26/PNQ/" + id.New()[:6]
	first := invoiceFor(fx, id.New(), number)
	firstEnv := envelope(contracts.AggregateTypeInvoice, first.ID, fx.tenantID, fx.outletID, 1)
	if _, err := svc.IngestInvoice(context.Background(), fx.tenantID, fx.outletID, firstEnv, first); err != nil {
		t.Fatalf("first IngestInvoice: %v", err)
	}

	second := invoiceFor(fx, id.New(), number) // same series+number, different id
	secondEnv := envelope(contracts.AggregateTypeInvoice, second.ID, fx.tenantID, fx.outletID, 1)
	_, err := svc.IngestInvoice(context.Background(), fx.tenantID, fx.outletID, secondEnv, second)
	if err == nil {
		t.Fatal("expected an error for a duplicate invoice_number with a different id")
	}
}

// TestIngestPayment_AppendOnlyAgainstRealSchema proves the reversal pattern
// (§53) lands as two distinct rows in the real payment table.
func TestIngestPayment_AppendOnlyAgainstRealSchema(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)
	svc := payments.NewService(payments.NewPostgresRepository(pool))
	now := time.Now().UTC()

	originalID := id.New()
	original := payments.Payment{
		ID: originalID, OutletID: fx.outletID, OrderID: fx.orderID,
		Method: contracts.PaymentMethodCash, Status: contracts.PaymentCaptureStatusCaptured,
		AmountPaise: 33600, CreatedByUserID: fx.userID, CreatedAt: now, UpdatedAt: now, Version: 1, SchemaVersion: 1,
	}
	env := envelope(contracts.AggregateTypePayment, originalID, fx.tenantID, fx.outletID, 1)
	if _, err := svc.IngestPayment(context.Background(), fx.tenantID, fx.outletID, env, original); err != nil {
		t.Fatalf("ingesting original payment: %v", err)
	}

	reversalID := id.New()
	reversal := payments.Payment{
		ID: reversalID, OutletID: fx.outletID, OrderID: fx.orderID,
		Method: contracts.PaymentMethodCash, Status: contracts.PaymentCaptureStatusRefunded,
		AmountPaise: -33600, ReversesPaymentID: &originalID,
		CreatedByUserID: fx.userID, CreatedAt: now, UpdatedAt: now, Version: 1, SchemaVersion: 1,
	}
	reversalEnv := envelope(contracts.AggregateTypePayment, reversalID, fx.tenantID, fx.outletID, 1)
	if _, err := svc.IngestPayment(context.Background(), fx.tenantID, fx.outletID, reversalEnv, reversal); err != nil {
		t.Fatalf("ingesting reversal payment: %v", err)
	}

	var count int
	if err := pool.QueryRow(context.Background(), `SELECT count(*) FROM payment WHERE order_id = $1`, fx.orderID).Scan(&count); err != nil {
		t.Fatalf("counting payment rows: %v", err)
	}
	if count != 2 {
		t.Fatalf("expected exactly 2 payment rows (original + reversal), got %d", count)
	}
}

// TestCashShift_OpenThenClose exercises both ingest routes' service methods
// against real Postgres: open, then close with a full count, asserting the
// stored row reflects CLOSED with the expected/actual/variance recorded.
func TestCashShift_OpenThenClose(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)
	svc := payments.NewService(payments.NewPostgresRepository(pool))
	now := time.Now().UTC()

	shiftID := id.New()
	deviceID := id.New()
	open := payments.CashShift{
		ID: shiftID, OutletID: fx.outletID, DeviceID: deviceID, CashierUserID: fx.userID,
		Status: contracts.CashShiftStatusOpen, OpenedAt: now, OpeningCashPaise: 500000,
		BusinessDate: "2026-08-14", CreatedAt: now, UpdatedAt: now, Version: 1, SchemaVersion: 1,
	}
	openEnv := envelope(contracts.AggregateTypeCashShift, shiftID, fx.tenantID, fx.outletID, 1)
	if _, err := svc.IngestCashShift(context.Background(), fx.tenantID, fx.outletID, openEnv, open); err != nil {
		t.Fatalf("IngestCashShift: %v", err)
	}

	expected, actual, variance := 500000, 500000, 0
	closed := open
	closed.Status = contracts.CashShiftStatusClosed
	closed.Version = 2
	closed.ClosedAt = &now
	closed.ExpectedCashPaise = &expected
	closed.ActualCashPaise = &actual
	closed.VariancePaise = &variance
	closeEnv := envelope(contracts.AggregateTypeCashShift, shiftID, fx.tenantID, fx.outletID, 2)

	stored, err := svc.CloseCashShift(context.Background(), fx.tenantID, fx.outletID, closeEnv, shiftID, closed)
	if err != nil {
		t.Fatalf("CloseCashShift: %v", err)
	}
	if stored.Status != contracts.CashShiftStatusClosed {
		t.Fatalf("expected CLOSED, got %s", stored.Status)
	}
	if stored.ActualCashPaise == nil || *stored.ActualCashPaise != actual {
		t.Fatalf("expected actual_cash_paise %d persisted, got %+v", actual, stored.ActualCashPaise)
	}
}
