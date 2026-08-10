package kitchen

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

// fakeRepository is an in-memory Repository used to unit test Service
// without a database, mirroring backend/internal/menu and
// backend/internal/tables's fakeRepository pattern.
type fakeRepository struct {
	outletVersions  map[string]int    // outletID -> config_version
	outletTenant    map[string]string // outletID -> tenantID
	stations        map[string]Station
	stationCodes    map[string]string // outletID|code -> stationID
	printers        map[string]Printer
	printerNames    map[string]string // outletID|name -> printerID
	itemOutlet      map[string]string // menuItemID -> outletID
	itemStations    map[string][]string
	stationPrinters map[string][]string
	orderOutlet     map[string]string // orderID -> outletID
	kots            map[string]Kot
	bumpCalls       int
}

func newFakeRepository() *fakeRepository {
	return &fakeRepository{
		outletVersions:  map[string]int{},
		outletTenant:    map[string]string{},
		stations:        map[string]Station{},
		stationCodes:    map[string]string{},
		printers:        map[string]Printer{},
		printerNames:    map[string]string{},
		itemOutlet:      map[string]string{},
		itemStations:    map[string][]string{},
		stationPrinters: map[string][]string{},
		orderOutlet:     map[string]string{},
		kots:            map[string]Kot{},
	}
}

func (f *fakeRepository) WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error {
	return fn(nil)
}

func (f *fakeRepository) BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error) {
	f.bumpCalls++
	if _, ok := f.outletTenant[outletID]; !ok {
		return 0, httpx.ErrNotFound
	}
	f.outletVersions[outletID]++
	return f.outletVersions[outletID], nil
}

func (f *fakeRepository) OutletBelongsToTenant(ctx context.Context, tenantID, outletID string) (bool, error) {
	return f.outletTenant[outletID] == tenantID, nil
}

func (f *fakeRepository) ListStations(ctx context.Context, outletID string) ([]Station, error) {
	var out []Station
	for _, s := range f.stations {
		if s.OutletID == outletID {
			out = append(out, s)
		}
	}
	return out, nil
}

func (f *fakeRepository) InsertStation(ctx context.Context, tx pgx.Tx, s Station) error {
	key := s.OutletID + "|" + s.Code
	if _, exists := f.stationCodes[key]; exists {
		return httpx.ErrConflict
	}
	f.stationCodes[key] = s.ID
	f.stations[s.ID] = s
	return nil
}

func (f *fakeRepository) GetStation(ctx context.Context, stationID string) (Station, error) {
	s, ok := f.stations[stationID]
	if !ok {
		return Station{}, httpx.ErrNotFound
	}
	return s, nil
}

func (f *fakeRepository) StationsBelongToOutlet(ctx context.Context, outletID string, stationIDs []string) (bool, error) {
	for _, id := range stationIDs {
		s, ok := f.stations[id]
		if !ok || s.OutletID != outletID {
			return false, nil
		}
	}
	return true, nil
}

func (f *fakeRepository) ListPrinters(ctx context.Context, outletID string) ([]Printer, error) {
	var out []Printer
	for _, p := range f.printers {
		if p.OutletID == outletID {
			out = append(out, p)
		}
	}
	return out, nil
}

func (f *fakeRepository) InsertPrinter(ctx context.Context, tx pgx.Tx, p Printer) error {
	key := p.OutletID + "|" + p.Name
	if _, exists := f.printerNames[key]; exists {
		return httpx.ErrConflict
	}
	f.printerNames[key] = p.ID
	f.printers[p.ID] = p
	return nil
}

func (f *fakeRepository) GetPrinter(ctx context.Context, printerID string) (Printer, error) {
	p, ok := f.printers[printerID]
	if !ok {
		return Printer{}, httpx.ErrNotFound
	}
	return p, nil
}

func (f *fakeRepository) PrintersBelongToOutlet(ctx context.Context, outletID string, printerIDs []string) (bool, error) {
	for _, id := range printerIDs {
		p, ok := f.printers[id]
		if !ok || p.OutletID != outletID {
			return false, nil
		}
	}
	return true, nil
}

func (f *fakeRepository) MenuItemOutlet(ctx context.Context, itemID string) (string, error) {
	outletID, ok := f.itemOutlet[itemID]
	if !ok {
		return "", httpx.ErrNotFound
	}
	return outletID, nil
}

func (f *fakeRepository) ReplaceItemStations(ctx context.Context, tx pgx.Tx, itemID string, stationIDs []string, configVersion int) ([]MenuItemStation, error) {
	f.itemStations[itemID] = append([]string{}, stationIDs...)
	out := make([]MenuItemStation, 0, len(stationIDs))
	for _, sid := range stationIDs {
		out = append(out, MenuItemStation{MenuItemID: itemID, StationID: sid, ConfigVersion: configVersion, SchemaVersion: 1})
	}
	return out, nil
}

func (f *fakeRepository) ReplaceStationPrinters(ctx context.Context, tx pgx.Tx, stationID string, printerIDs []string, configVersion int) ([]StationPrinter, error) {
	f.stationPrinters[stationID] = append([]string{}, printerIDs...)
	out := make([]StationPrinter, 0, len(printerIDs))
	for _, pid := range printerIDs {
		out = append(out, StationPrinter{StationID: stationID, PrinterID: pid, ConfigVersion: configVersion, SchemaVersion: 1})
	}
	return out, nil
}

func (f *fakeRepository) OrderOutlet(ctx context.Context, orderID string) (string, error) {
	outletID, ok := f.orderOutlet[orderID]
	if !ok {
		return "", httpx.ErrNotFound
	}
	return outletID, nil
}

func (f *fakeRepository) InsertKot(ctx context.Context, tx pgx.Tx, deviceID string, k Kot) (Kot, bool, error) {
	if existing, ok := f.kots[k.ID]; ok {
		return existing, false, nil
	}
	if k.Items == nil {
		k.Items = []KotTicketItem{}
	}
	f.kots[k.ID] = k
	return k, true, nil
}

func (f *fakeRepository) GetKot(ctx context.Context, kotID string) (Kot, error) {
	k, ok := f.kots[kotID]
	if !ok {
		return Kot{}, httpx.ErrNotFound
	}
	return k, nil
}

func (f *fakeRepository) UpdateKotStatus(ctx context.Context, tx pgx.Tx, kotID string, status KotStatus, changedAt time.Time) (Kot, error) {
	k, ok := f.kots[kotID]
	if !ok {
		return Kot{}, httpx.ErrNotFound
	}
	k.Status = status
	k.UpdatedAt = changedAt
	f.kots[kotID] = k
	return k, nil
}

func (f *fakeRepository) StationsSince(ctx context.Context, outletID string, sinceVersion int) ([]Station, error) {
	var out []Station
	for _, s := range f.stations {
		if s.OutletID == outletID && s.ConfigVersion > sinceVersion {
			out = append(out, s)
		}
	}
	return out, nil
}

func (f *fakeRepository) ItemStationsSince(ctx context.Context, outletID string, sinceVersion int) ([]MenuItemStation, error) {
	var out []MenuItemStation
	for itemID, stationIDs := range f.itemStations {
		for _, sid := range stationIDs {
			s, ok := f.stations[sid]
			if ok && s.OutletID == outletID {
				out = append(out, MenuItemStation{MenuItemID: itemID, StationID: sid, ConfigVersion: s.ConfigVersion, SchemaVersion: 1})
			}
		}
	}
	return out, nil
}

func (f *fakeRepository) PrintersSince(ctx context.Context, outletID string, sinceVersion int) ([]Printer, error) {
	var out []Printer
	for _, p := range f.printers {
		if p.OutletID == outletID && p.ConfigVersion > sinceVersion {
			out = append(out, p)
		}
	}
	return out, nil
}

func (f *fakeRepository) StationPrintersSince(ctx context.Context, outletID string, sinceVersion int) ([]StationPrinter, error) {
	var out []StationPrinter
	for stationID, printerIDs := range f.stationPrinters {
		s, ok := f.stations[stationID]
		if !ok || s.OutletID != outletID {
			continue
		}
		for _, pid := range printerIDs {
			out = append(out, StationPrinter{StationID: stationID, PrinterID: pid, ConfigVersion: s.ConfigVersion, SchemaVersion: 1})
		}
	}
	return out, nil
}

// --- fixtures --------------------------------------------------------------

const (
	testTenantID  = "11111111-1111-7111-8111-111111111111"
	testOutletID  = "22222222-2222-7222-8222-222222222222"
	testDeviceID  = "33333333-3333-7333-8333-333333333333"
	testOrderID   = "44444444-4444-7444-8444-444444444444"
	testStationID = "55555555-5555-7555-8555-555555555555"
	testPrinterID = "66666666-6666-7666-8666-666666666666"
	testItemID    = "77777777-7777-7777-8777-777777777777"
	testKotID     = "88888888-8888-7888-8888-888888888888"
)

func newTestService() (*Service, *fakeRepository) {
	repo := newFakeRepository()
	repo.outletTenant[testOutletID] = testTenantID
	svc := NewService(repo, nil)
	return svc, repo
}

func ctxWithPermissions(perms ...auth.Permission) context.Context {
	return auth.WithPrincipal(context.Background(), auth.AuthenticatedPrincipal{
		UserID:      "principal-user",
		TenantID:    testTenantID,
		OutletID:    testOutletID,
		Permissions: perms,
	})
}

func baseKotEnvelope(recordID string, version int) contracts.SyncEnvelope {
	return contracts.SyncEnvelope{
		RecordID:      recordID,
		TenantID:      testTenantID,
		OutletID:      testOutletID,
		DeviceID:      testDeviceID,
		AggregateType: contracts.AggregateTypeKot,
		Direction:     contracts.SyncDirectionEdgeToCloud,
		Version:       version,
		SyncStatus:    contracts.SyncStatusPending,
	}
}

func baseKot() Kot {
	return Kot{
		ID:       testKotID,
		OrderID:  testOrderID,
		Station:  "MAIN_KITCHEN",
		Sequence: 1,
		Status:   KotStatusNew,
		Items: []KotTicketItem{
			{OrderItemID: "99999999-9999-7999-8999-999999999999", Name: "Butter Chicken", Quantity: 1, Modifiers: []string{}},
		},
		CreatedByDeviceID: testDeviceID,
		CreatedAt:         time.Date(2026, 8, 10, 12, 0, 0, 0, time.UTC),
		UpdatedAt:         time.Date(2026, 8, 10, 12, 0, 0, 0, time.UTC),
		SchemaVersion:     1,
	}
}

// --- Station -----------------------------------------------------------

func TestCreateStation_HappyPath(t *testing.T) {
	svc, _ := newTestService()
	ctx := ctxWithPermissions(auth.PermissionMenuManage)

	st, err := svc.CreateStation(ctx, testTenantID, NewStationInput{
		ID: testStationID, OutletID: testOutletID, Code: "MAIN_KITCHEN", Name: "Main Kitchen", SortOrder: 1, IsActive: true,
	})
	if err != nil {
		t.Fatalf("CreateStation: %v", err)
	}
	if st.ConfigVersion != 1 {
		t.Fatalf("expected config_version 1, got %d", st.ConfigVersion)
	}
}

func TestCreateStation_MissingPermissionIsForbidden(t *testing.T) {
	svc, _ := newTestService()
	ctx := ctxWithPermissions()

	_, err := svc.CreateStation(ctx, testTenantID, NewStationInput{
		ID: testStationID, OutletID: testOutletID, Code: "MAIN_KITCHEN", Name: "Main Kitchen",
	})
	if !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("expected ErrForbidden, got %v", err)
	}
}

func TestCreateStation_DuplicateCodeIsConflict(t *testing.T) {
	svc, _ := newTestService()
	ctx := ctxWithPermissions(auth.PermissionMenuManage)

	in := NewStationInput{ID: testStationID, OutletID: testOutletID, Code: "TANDOOR", Name: "Tandoor"}
	if _, err := svc.CreateStation(ctx, testTenantID, in); err != nil {
		t.Fatalf("setup: %v", err)
	}
	in2 := in
	in2.ID = "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa"
	if _, err := svc.CreateStation(ctx, testTenantID, in2); !errors.Is(err, httpx.ErrConflict) {
		t.Fatalf("expected ErrConflict for duplicate code, got %v", err)
	}
}

func TestCreateStation_WrongTenantOutletIsForbidden(t *testing.T) {
	svc, repo := newTestService()
	repo.outletTenant[testOutletID] = "other-tenant"
	ctx := ctxWithPermissions(auth.PermissionMenuManage)

	_, err := svc.CreateStation(ctx, testTenantID, NewStationInput{
		ID: testStationID, OutletID: testOutletID, Code: "BAR", Name: "Bar",
	})
	if !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("expected ErrForbidden, got %v", err)
	}
}

// --- Routing -------------------------------------------------------------

func TestReplaceItemStations_HappyPath(t *testing.T) {
	svc, repo := newTestService()
	repo.itemOutlet[testItemID] = testOutletID
	ctx := ctxWithPermissions(auth.PermissionMenuManage)

	st, err := svc.CreateStation(ctx, testTenantID, NewStationInput{ID: testStationID, OutletID: testOutletID, Code: "TANDOOR", Name: "Tandoor"})
	if err != nil {
		t.Fatalf("setup CreateStation: %v", err)
	}

	out, err := svc.ReplaceItemStations(ctx, testTenantID, testItemID, []string{st.ID})
	if err != nil {
		t.Fatalf("ReplaceItemStations: %v", err)
	}
	if len(out) != 1 || out[0].StationID != st.ID {
		t.Fatalf("expected routing to station %s, got %+v", st.ID, out)
	}
}

func TestReplaceItemStations_UnknownStationIsInvalid(t *testing.T) {
	svc, repo := newTestService()
	repo.itemOutlet[testItemID] = testOutletID
	ctx := ctxWithPermissions(auth.PermissionMenuManage)

	_, err := svc.ReplaceItemStations(ctx, testTenantID, testItemID, []string{"nonexistent-station"})
	if !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput, got %v", err)
	}
}

// --- KOT ingest: EDGE_TO_CLOUD, replay-only -------------------------------

func TestIngestKot_HappyPath(t *testing.T) {
	svc, repo := newTestService()
	repo.orderOutlet[testOrderID] = testOutletID

	stored, err := svc.IngestKot(context.Background(), testTenantID, baseKotEnvelope(testKotID, 1), baseKot())
	if err != nil {
		t.Fatalf("IngestKot: %v", err)
	}
	if stored.Status != KotStatusNew {
		t.Fatalf("expected status NEW, got %s", stored.Status)
	}
}

func TestIngestKot_DuplicateEnvelopeIsIdempotent(t *testing.T) {
	svc, repo := newTestService()
	repo.orderOutlet[testOrderID] = testOutletID

	env := baseKotEnvelope(testKotID, 1)
	if _, err := svc.IngestKot(context.Background(), testTenantID, env, baseKot()); err != nil {
		t.Fatalf("first ingest: %v", err)
	}
	if _, err := svc.IngestKot(context.Background(), testTenantID, env, baseKot()); err != nil {
		t.Fatalf("duplicate ingest: %v", err)
	}
	if len(repo.kots) != 1 {
		t.Fatalf("expected exactly one kot, got %d", len(repo.kots))
	}
}

func TestIngestKot_WrongAggregateTypeIsAuthorityViolation(t *testing.T) {
	svc, repo := newTestService()
	repo.orderOutlet[testOrderID] = testOutletID

	env := baseKotEnvelope(testKotID, 1)
	env.AggregateType = contracts.AggregateTypeOrder
	if _, err := svc.IngestKot(context.Background(), testTenantID, env, baseKot()); !errors.Is(err, ErrAuthorityViolation) {
		t.Fatalf("expected ErrAuthorityViolation, got %v", err)
	}
}

func TestIngestKot_CloudToEdgeDirectionIsAuthorityViolation(t *testing.T) {
	svc, repo := newTestService()
	repo.orderOutlet[testOrderID] = testOutletID

	env := baseKotEnvelope(testKotID, 1)
	env.Direction = contracts.SyncDirectionCloudToEdge
	if _, err := svc.IngestKot(context.Background(), testTenantID, env, baseKot()); !errors.Is(err, ErrAuthorityViolation) {
		t.Fatalf("expected ErrAuthorityViolation, got %v", err)
	}
}

func TestIngestKot_WrongTenantIsForbidden(t *testing.T) {
	svc, repo := newTestService()
	repo.orderOutlet[testOrderID] = testOutletID

	env := baseKotEnvelope(testKotID, 1)
	if _, err := svc.IngestKot(context.Background(), "some-other-tenant", env, baseKot()); !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("expected ErrForbidden, got %v", err)
	}
}

// --- KOT status ingest: the ONLY writer of kot.status ---------------------

func TestIngestKotStatus_HappyPath(t *testing.T) {
	svc, repo := newTestService()
	repo.orderOutlet[testOrderID] = testOutletID
	if _, err := svc.IngestKot(context.Background(), testTenantID, baseKotEnvelope(testKotID, 1), baseKot()); err != nil {
		t.Fatalf("setup IngestKot: %v", err)
	}

	changedAt := time.Date(2026, 8, 10, 12, 5, 0, 0, time.UTC)
	env := baseKotEnvelope(testKotID, 2)
	stored, err := svc.IngestKotStatus(context.Background(), testTenantID, env, testKotID, KotStatusTransition{
		Status: KotStatusAcknowledged, ChangedAt: changedAt, ChangedByDeviceID: testDeviceID,
	})
	if err != nil {
		t.Fatalf("IngestKotStatus: %v", err)
	}
	if stored.Status != KotStatusAcknowledged {
		t.Fatalf("expected ACKNOWLEDGED, got %s", stored.Status)
	}
	if !stored.UpdatedAt.Equal(changedAt) {
		t.Fatalf("expected updated_at %v (edge-recorded), got %v", changedAt, stored.UpdatedAt)
	}
}

func TestIngestKotStatus_IllegalTransitionIsRejected(t *testing.T) {
	svc, repo := newTestService()
	repo.orderOutlet[testOrderID] = testOutletID
	if _, err := svc.IngestKot(context.Background(), testTenantID, baseKotEnvelope(testKotID, 1), baseKot()); err != nil {
		t.Fatalf("setup IngestKot: %v", err)
	}

	env := baseKotEnvelope(testKotID, 2)
	_, err := svc.IngestKotStatus(context.Background(), testTenantID, env, testKotID, KotStatusTransition{
		Status: KotStatusServed, ChangedAt: time.Now().UTC(), ChangedByDeviceID: testDeviceID,
	})
	if !errors.Is(err, ErrIllegalTransition) {
		t.Fatalf("expected ErrIllegalTransition (NEW->SERVED is not legal), got %v", err)
	}
}

func TestIngestKotStatus_DuplicateReplayIsIdempotent(t *testing.T) {
	svc, repo := newTestService()
	repo.orderOutlet[testOrderID] = testOutletID
	if _, err := svc.IngestKot(context.Background(), testTenantID, baseKotEnvelope(testKotID, 1), baseKot()); err != nil {
		t.Fatalf("setup IngestKot: %v", err)
	}

	transition := KotStatusTransition{Status: KotStatusAcknowledged, ChangedAt: time.Now().UTC(), ChangedByDeviceID: testDeviceID}
	env := baseKotEnvelope(testKotID, 2)
	first, err := svc.IngestKotStatus(context.Background(), testTenantID, env, testKotID, transition)
	if err != nil {
		t.Fatalf("first transition: %v", err)
	}
	second, err := svc.IngestKotStatus(context.Background(), testTenantID, env, testKotID, transition)
	if err != nil {
		t.Fatalf("duplicate replay: %v", err)
	}
	if second.Status != first.Status {
		t.Fatalf("expected idempotent replay to leave status %s, got %s", first.Status, second.Status)
	}
}

func TestIngestKotStatus_WrongAggregateTypeIsAuthorityViolation(t *testing.T) {
	svc, repo := newTestService()
	repo.orderOutlet[testOrderID] = testOutletID
	if _, err := svc.IngestKot(context.Background(), testTenantID, baseKotEnvelope(testKotID, 1), baseKot()); err != nil {
		t.Fatalf("setup IngestKot: %v", err)
	}

	env := baseKotEnvelope(testKotID, 2)
	env.AggregateType = contracts.AggregateTypeOrder
	_, err := svc.IngestKotStatus(context.Background(), testTenantID, env, testKotID, KotStatusTransition{
		Status: KotStatusAcknowledged, ChangedAt: time.Now().UTC(), ChangedByDeviceID: testDeviceID,
	})
	if !errors.Is(err, ErrAuthorityViolation) {
		t.Fatalf("expected ErrAuthorityViolation, got %v", err)
	}
}
