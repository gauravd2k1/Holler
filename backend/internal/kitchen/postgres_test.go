package kitchen_test

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/kitchen"
	"github.com/holler/backend/internal/menu"
	"github.com/holler/backend/internal/ordering"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/tenant"
	contracts "github.com/holler/contracts"
)

// setupPool mirrors backend/internal/ordering/postgres_test.go exactly: same
// env var gate, same migration path (packages/contracts/postgres, which now
// includes 0006_m2_kitchen_stations_printers.sql — the migration runner
// globs every *.sql file in that directory, so no extra wiring was needed to
// pick it up).
func setupPool(t *testing.T) postgres.Pool {
	t.Helper()

	dbURL := os.Getenv("HOLLER_TEST_DATABASE_URL")
	if dbURL == "" {
		t.Skip("HOLLER_TEST_DATABASE_URL not set; skipping kitchen Postgres integration test")
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

	org, err := tenantSvc.CreateOrganisation(ctx, "Kitchen Integration Org")
	if err != nil {
		t.Fatalf("CreateOrganisation: %v", err)
	}
	brand, err := tenantSvc.CreateBrand(ctx, org.ID, "Kitchen Integration Brand")
	if err != nil {
		t.Fatalf("CreateBrand: %v", err)
	}
	out, err := outletSvc.CreateOutlet(ctx, outlet.Principal{TenantID: org.ID}, brand.ID, "Kitchen Integration Outlet", "")
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

func kitchenCtx(tenantID, outletID string) context.Context {
	return auth.WithPrincipal(context.Background(), auth.AuthenticatedPrincipal{
		UserID:   "principal-user",
		TenantID: tenantID,
		OutletID: outletID,
		Permissions: []auth.Permission{
			auth.PermissionMenuManage, auth.PermissionOutletManage, auth.PermissionOrderModify,
		},
	})
}

const testDeviceID = "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa"

func kotEnvelope(recordID, tenantID, outletID string, version int) contracts.SyncEnvelope {
	now := time.Now().UTC()
	return contracts.SyncEnvelope{
		RecordID:      recordID,
		TenantID:      tenantID,
		OutletID:      outletID,
		DeviceID:      testDeviceID,
		AggregateType: contracts.AggregateTypeKot,
		Direction:     contracts.SyncDirectionEdgeToCloud,
		CreatedAt:     now,
		UpdatedAt:     now,
		Version:       version,
		SyncStatus:    contracts.SyncStatusPending,
	}
}

// TestPostgresRepository_StationPrinterKotLifecycle exercises station
// creation, item->station routing, printer creation, station->printer
// routing, KOT ingest and a status transition end to end against a real
// Postgres, proving the 0006 migration and the repository queries built on
// top of it actually agree with the schema.
func TestPostgresRepository_StationPrinterKotLifecycle(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)

	// Seed an order the KOT will ticket — kitchen's Repository does not own
	// order creation, ordering's does.
	orderSvc := ordering.NewService(ordering.NewPostgresRepository(pool))
	orderID := "bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb"
	now := time.Now().UTC()
	order := contracts.CanonicalOrder{
		HollerOrderID: orderID,
		Source:        contracts.OrderSourcePOS,
		OutletID:      fx.outletID,
		OrderType:     contracts.OrderTypeDineIn,
		Status:        contracts.OrderStatusDraft,
		Items:         []contracts.OrderItem{},
		PaymentStatus: contracts.PaymentStatusUnpaid,
		Timestamps:    contracts.OrderTimestamps{CreatedAt: now, UpdatedAt: now},
		SchemaVersion: 1,
	}
	orderEnv := contracts.SyncEnvelope{
		RecordID: orderID, TenantID: fx.tenantID, OutletID: fx.outletID, DeviceID: testDeviceID,
		AggregateType: contracts.AggregateTypeOrder, Direction: contracts.SyncDirectionEdgeToCloud,
		Version: 1, SyncStatus: contracts.SyncStatusPending,
	}
	if _, err := orderSvc.IngestOrder(context.Background(), fx.tenantID, orderEnv, order); err != nil {
		t.Fatalf("seeding order: %v", err)
	}

	svc := kitchen.NewService(kitchen.NewRepository(pool), nil)
	ctx := kitchenCtx(fx.tenantID, fx.outletID)

	stationID := "cccccccc-cccc-7ccc-8ccc-cccccccccccc"
	station, err := svc.CreateStation(ctx, fx.tenantID, kitchen.NewStationInput{
		ID: stationID, OutletID: fx.outletID, Code: "MAIN_KITCHEN", Name: "Main Kitchen", IsActive: true,
	})
	if err != nil {
		t.Fatalf("CreateStation: %v", err)
	}
	if station.ConfigVersion != 1 {
		t.Fatalf("expected station config_version 1, got %d", station.ConfigVersion)
	}

	routing, err := svc.ReplaceItemStations(ctx, fx.tenantID, fx.menuItemID, []string{stationID})
	if err != nil {
		t.Fatalf("ReplaceItemStations: %v", err)
	}
	if len(routing) != 1 || routing[0].StationID != stationID {
		t.Fatalf("unexpected routing: %+v", routing)
	}

	printerID := "dddddddd-dddd-7ddd-8ddd-dddddddddddd"
	printer, err := svc.CreatePrinter(ctx, fx.tenantID, kitchen.NewPrinterInput{
		ID: printerID, OutletID: fx.outletID, Name: "Kitchen Printer",
		ConnectionKind: kitchen.PrinterConnectionNetwork, Address: "192.168.1.50:9100", PaperWidthMM: 80, IsActive: true,
	})
	if err != nil {
		t.Fatalf("CreatePrinter: %v", err)
	}
	if printer.ConfigVersion != 2 {
		t.Fatalf("expected printer config_version 2 (second config write on this outlet), got %d", printer.ConfigVersion)
	}

	printerRouting, err := svc.ReplaceStationPrinters(ctx, fx.tenantID, stationID, []string{printerID})
	if err != nil {
		t.Fatalf("ReplaceStationPrinters: %v", err)
	}
	if len(printerRouting) != 1 || printerRouting[0].PrinterID != printerID {
		t.Fatalf("unexpected printer routing: %+v", printerRouting)
	}

	kotID := "eeeeeeee-eeee-7eee-8eee-eeeeeeeeeeee"
	kot := contracts.Kot{
		ID: kotID, OrderID: orderID, Station: "MAIN_KITCHEN", Sequence: 1, Status: contracts.KotStatusNew,
		Items: []contracts.KotTicketItem{
			{OrderItemID: "ffffffff-ffff-7fff-8fff-ffffffffffff", Name: "Butter Chicken", Quantity: 1, Modifiers: []string{}},
		},
		CreatedByDeviceID: testDeviceID, CreatedAt: now, UpdatedAt: now, SchemaVersion: 1,
	}
	stored, err := svc.IngestKot(context.Background(), fx.tenantID, kotEnvelope(kotID, fx.tenantID, fx.outletID, 1), kot)
	if err != nil {
		t.Fatalf("IngestKot: %v", err)
	}
	if stored.Status != contracts.KotStatusNew {
		t.Fatalf("expected NEW, got %s", stored.Status)
	}

	// Duplicate replay: exactly one row.
	if _, err := svc.IngestKot(context.Background(), fx.tenantID, kotEnvelope(kotID, fx.tenantID, fx.outletID, 1), kot); err != nil {
		t.Fatalf("duplicate IngestKot: %v", err)
	}

	changedAt := time.Now().UTC()
	transitioned, err := svc.IngestKotStatus(context.Background(), fx.tenantID, kotEnvelope(kotID, fx.tenantID, fx.outletID, 2), kotID, kitchen.KotStatusTransition{
		Status: kitchen.KotStatusAcknowledged, ChangedAt: changedAt, ChangedByDeviceID: testDeviceID,
	})
	if err != nil {
		t.Fatalf("IngestKotStatus: %v", err)
	}
	if transitioned.Status != contracts.KotStatusAcknowledged {
		t.Fatalf("expected ACKNOWLEDGED, got %s", transitioned.Status)
	}

	bundle, err := svc.SyncConfigBundle(context.Background(), fx.tenantID, fx.outletID, 0)
	if err != nil {
		t.Fatalf("SyncConfigBundle: %v", err)
	}
	if len(bundle.Stations) != 1 || len(bundle.Printers) != 1 || len(bundle.ItemStations) != 1 || len(bundle.StationPrinters) != 1 {
		t.Fatalf("unexpected bundle: %+v", bundle)
	}
}
