package tables

import (
	"context"
	"errors"
	"testing"
	"time"

	contracts "github.com/holler/contracts"
	"github.com/jackc/pgx/v5"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
)

// fakeRepository is an in-memory Repository used to unit test Service
// without a database.
type fakeRepository struct {
	outletVersions map[string]int
	tables         []RestaurantTable
	sessions       []TableSession
	bumpCalls      int
}

func newFakeRepository(outletIDs ...string) *fakeRepository {
	f := &fakeRepository{outletVersions: map[string]int{}}
	for _, o := range outletIDs {
		f.outletVersions[o] = 0
	}
	return f
}

func (f *fakeRepository) WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error {
	return fn(nil)
}

func (f *fakeRepository) BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error) {
	f.bumpCalls++
	v, ok := f.outletVersions[outletID]
	if !ok {
		return 0, httpx.ErrNotFound
	}
	v++
	f.outletVersions[outletID] = v
	return v, nil
}

func (f *fakeRepository) ListTables(ctx context.Context, outletID string) ([]RestaurantTable, error) {
	var out []RestaurantTable
	for _, t := range f.tables {
		if t.OutletID == outletID {
			out = append(out, t)
		}
	}
	return out, nil
}

func (f *fakeRepository) InsertTable(ctx context.Context, tx pgx.Tx, t RestaurantTable) error {
	f.tables = append(f.tables, t)
	return nil
}

func (f *fakeRepository) TableExists(ctx context.Context, outletID, tableID string) (bool, error) {
	for _, t := range f.tables {
		if t.ID == tableID && t.OutletID == outletID {
			return true, nil
		}
	}
	return false, nil
}

func (f *fakeRepository) TableLabelTaken(ctx context.Context, outletID, label string) (bool, error) {
	for _, t := range f.tables {
		if t.OutletID == outletID && t.Label == label {
			return true, nil
		}
	}
	return false, nil
}

func (f *fakeRepository) InsertSession(ctx context.Context, tx pgx.Tx, s TableSession) error {
	for _, existing := range f.sessions {
		if existing.TableID == s.TableID && existing.ClosedAt == nil {
			return httpx.ErrConflict
		}
	}
	f.sessions = append(f.sessions, s)
	return nil
}

func (f *fakeRepository) UpdateSession(ctx context.Context, tx pgx.Tx, s TableSession) error {
	for i, existing := range f.sessions {
		if existing.ID == s.ID {
			f.sessions[i] = s
			return nil
		}
	}
	return httpx.ErrNotFound
}

func (f *fakeRepository) GetOpenSessionByTable(ctx context.Context, tableID string) (TableSession, bool, error) {
	for _, s := range f.sessions {
		if s.TableID == tableID && s.ClosedAt == nil {
			return s, true, nil
		}
	}
	return TableSession{}, false, nil
}

func (f *fakeRepository) GetSession(ctx context.Context, outletID, sessionID string) (TableSession, error) {
	for _, s := range f.sessions {
		if s.ID == sessionID && s.OutletID == outletID {
			return s, nil
		}
	}
	return TableSession{}, httpx.ErrNotFound
}

func (f *fakeRepository) ListOpenSessions(ctx context.Context, outletID string) ([]TableSession, error) {
	var out []TableSession
	for _, s := range f.sessions {
		if s.OutletID == outletID && s.ClosedAt == nil {
			out = append(out, s)
		}
	}
	return out, nil
}

// --- principal fixtures ----------------------------------------------------

type fakePrincipal struct {
	permissions map[string]bool
}

func (p fakePrincipal) HasPermission(permission string) bool { return p.permissions[permission] }

func authorizedContext() context.Context {
	return WithPrincipal(context.Background(), fakePrincipal{permissions: map[string]bool{permTableManage: true}})
}

func unauthorizedContext() context.Context {
	return WithPrincipal(context.Background(), fakePrincipal{permissions: map[string]bool{}})
}

// --- RestaurantTable: config discipline ------------------------------------

func TestCreateTable_BumpsConfigVersionOnly(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	svc := NewService(repo)

	table, err := svc.CreateTable(authorizedContext(), NewTableInput{
		OutletID: outletID, Section: "GROUND", Label: "T1", SeatCount: 4,
	})
	if err != nil {
		t.Fatalf("CreateTable: %v", err)
	}
	if table.ConfigVersion != 1 {
		t.Fatalf("expected config_version 1, got %d", table.ConfigVersion)
	}
	if repo.bumpCalls != 1 {
		t.Fatalf("expected exactly one config_version bump, got %d", repo.bumpCalls)
	}

	second, err := svc.CreateTable(authorizedContext(), NewTableInput{
		OutletID: outletID, Section: "GROUND", Label: "T2", SeatCount: 2,
	})
	if err != nil {
		t.Fatalf("CreateTable (second): %v", err)
	}
	if second.ConfigVersion != 2 {
		t.Fatalf("expected config_version 2 on second table, got %d", second.ConfigVersion)
	}
}

func TestCreateTable_RequiresPermission(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	svc := NewService(repo)

	_, err := svc.CreateTable(unauthorizedContext(), NewTableInput{
		OutletID: outletID, Section: "GROUND", Label: "T1", SeatCount: 4,
	})
	if !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("expected ErrForbidden, got %v", err)
	}
}

func TestCreateTable_DuplicateLabelConflicts(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	svc := NewService(repo)

	if _, err := svc.CreateTable(authorizedContext(), NewTableInput{OutletID: outletID, Section: "GROUND", Label: "T1", SeatCount: 4}); err != nil {
		t.Fatalf("CreateTable: %v", err)
	}
	_, err := svc.CreateTable(authorizedContext(), NewTableInput{OutletID: outletID, Section: "GROUND", Label: "T1", SeatCount: 2})
	if !errors.Is(err, httpx.ErrConflict) {
		t.Fatalf("expected ErrConflict for duplicate label, got %v", err)
	}
}

func TestCreateTable_ScopedToOutlet(t *testing.T) {
	outletA, outletB := id.New(), id.New()
	repo := newFakeRepository(outletA, outletB)
	svc := NewService(repo)

	if _, err := svc.CreateTable(authorizedContext(), NewTableInput{OutletID: outletA, Section: "GROUND", Label: "T1", SeatCount: 4}); err != nil {
		t.Fatalf("CreateTable outlet A: %v", err)
	}
	if _, err := svc.CreateTable(authorizedContext(), NewTableInput{OutletID: outletB, Section: "GROUND", Label: "T1", SeatCount: 4}); err != nil {
		t.Fatalf("CreateTable outlet B (same label, different outlet): %v", err)
	}

	tablesA, err := svc.ListTables(context.Background(), outletA)
	if err != nil {
		t.Fatalf("ListTables A: %v", err)
	}
	if len(tablesA) != 1 {
		t.Fatalf("expected outlet A to see only its own table, got %d", len(tablesA))
	}

	// outlet B's write must not have touched outlet A's config_version.
	if repo.outletVersions[outletA] != 1 {
		t.Fatalf("expected outlet A config_version to stay at 1, got %d", repo.outletVersions[outletA])
	}
}

// --- TableSession: operational aggregate, edge-authoritative --------------

func TestOpenSession_RejectsSecondOpenSessionForSameTable(t *testing.T) {
	outletID, tableID := id.New(), id.New()
	repo := newFakeRepository(outletID)
	repo.tables = append(repo.tables, RestaurantTable{ID: tableID, OutletID: outletID, Label: "T1"})
	svc := NewService(repo)

	if _, err := svc.OpenSession(context.Background(), OpenSessionInput{OutletID: outletID, TableID: tableID, GuestCount: 2}); err != nil {
		t.Fatalf("OpenSession (first): %v", err)
	}

	_, err := svc.OpenSession(context.Background(), OpenSessionInput{OutletID: outletID, TableID: tableID, GuestCount: 3})
	if !errors.Is(err, httpx.ErrConflict) {
		t.Fatalf("expected ErrConflict for second open session on same table, got %v", err)
	}
}

func TestOpenSession_NeverBumpsConfigVersion(t *testing.T) {
	outletID, tableID := id.New(), id.New()
	repo := newFakeRepository(outletID)
	repo.tables = append(repo.tables, RestaurantTable{ID: tableID, OutletID: outletID, Label: "T1"})
	svc := NewService(repo)

	if _, err := svc.OpenSession(context.Background(), OpenSessionInput{OutletID: outletID, TableID: tableID, GuestCount: 2}); err != nil {
		t.Fatalf("OpenSession: %v", err)
	}
	if repo.bumpCalls != 0 {
		t.Fatalf("expected session write never to bump outlet config_version, got %d bumps", repo.bumpCalls)
	}
	if repo.outletVersions[outletID] != 0 {
		t.Fatalf("expected outlet config_version unchanged by session write, got %d", repo.outletVersions[outletID])
	}
}

func TestOpenSession_UnknownTableRejected(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	svc := NewService(repo)

	_, err := svc.OpenSession(context.Background(), OpenSessionInput{OutletID: outletID, TableID: id.New(), GuestCount: 2})
	if !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput for a table outside the outlet, got %v", err)
	}
}

func TestTransitionSession_LegalPath(t *testing.T) {
	outletID, tableID := id.New(), id.New()
	repo := newFakeRepository(outletID)
	repo.tables = append(repo.tables, RestaurantTable{ID: tableID, OutletID: outletID, Label: "T1"})
	svc := NewService(repo)

	sess, err := svc.OpenSession(context.Background(), OpenSessionInput{OutletID: outletID, TableID: tableID, GuestCount: 2})
	if err != nil {
		t.Fatalf("OpenSession: %v", err)
	}

	path := []TableSessionState{
		contracts.TableSessionStateOrdered,
		contracts.TableSessionStateKotSent,
		contracts.TableSessionStateFoodReady,
		contracts.TableSessionStateBillRequested,
		contracts.TableSessionStatePaymentPending,
		contracts.TableSessionStatePaid,
		contracts.TableSessionStateDirty,
		contracts.TableSessionStateClosed,
	}
	for _, next := range path {
		sess, err = svc.TransitionSession(context.Background(), outletID, sess.ID, next, nil)
		if err != nil {
			t.Fatalf("transition to %s: %v", next, err)
		}
	}
	if sess.State != contracts.TableSessionStateClosed {
		t.Fatalf("expected final state CLOSED, got %s", sess.State)
	}
	if sess.ClosedAt == nil {
		t.Fatal("expected closed_at to be set once CLOSED")
	}
}

func TestTransitionSession_RejectsIllegalSkip(t *testing.T) {
	outletID, tableID := id.New(), id.New()
	repo := newFakeRepository(outletID)
	repo.tables = append(repo.tables, RestaurantTable{ID: tableID, OutletID: outletID, Label: "T1"})
	svc := NewService(repo)

	sess, err := svc.OpenSession(context.Background(), OpenSessionInput{OutletID: outletID, TableID: tableID, GuestCount: 2})
	if err != nil {
		t.Fatalf("OpenSession: %v", err)
	}

	// OCCUPIED -> PAID is not a legal edge.
	_, err = svc.TransitionSession(context.Background(), outletID, sess.ID, contracts.TableSessionStatePaid, nil)
	if !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput for illegal transition, got %v", err)
	}
}

func TestTransitionSession_RejectsTransitionAfterClose(t *testing.T) {
	outletID, tableID := id.New(), id.New()
	repo := newFakeRepository(outletID)
	repo.tables = append(repo.tables, RestaurantTable{ID: tableID, OutletID: outletID, Label: "T1"})
	svc := NewService(repo)

	sess, err := svc.OpenSession(context.Background(), OpenSessionInput{OutletID: outletID, TableID: tableID, GuestCount: 2})
	if err != nil {
		t.Fatalf("OpenSession: %v", err)
	}
	if _, err := svc.CloseSession(context.Background(), outletID, sess.ID); err != nil {
		t.Fatalf("CloseSession: %v", err)
	}
	_, err = svc.TransitionSession(context.Background(), outletID, sess.ID, contracts.TableSessionStateOrdered, nil)
	if !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput transitioning a closed session, got %v", err)
	}
}

func TestTransitionSession_ReopeningTableAfterCloseSucceeds(t *testing.T) {
	outletID, tableID := id.New(), id.New()
	repo := newFakeRepository(outletID)
	repo.tables = append(repo.tables, RestaurantTable{ID: tableID, OutletID: outletID, Label: "T1"})
	svc := NewService(repo)

	first, err := svc.OpenSession(context.Background(), OpenSessionInput{OutletID: outletID, TableID: tableID, GuestCount: 2})
	if err != nil {
		t.Fatalf("OpenSession: %v", err)
	}
	if _, err := svc.CloseSession(context.Background(), outletID, first.ID); err != nil {
		t.Fatalf("CloseSession: %v", err)
	}

	// A closed session frees the table for a new one.
	if _, err := svc.OpenSession(context.Background(), OpenSessionInput{OutletID: outletID, TableID: tableID, GuestCount: 4}); err != nil {
		t.Fatalf("OpenSession (re-seat): %v", err)
	}
}

// --- Derived display state --------------------------------------------------

func TestDeriveDisplayState_AvailableWhenNoOpenSession(t *testing.T) {
	if got := DeriveDisplayState(nil); got != string(contracts.TableDisplayStateAvailable) {
		t.Fatalf("expected AVAILABLE for no open session, got %s", got)
	}
}

func TestDeriveDisplayState_ReflectsOpenSessionState(t *testing.T) {
	sess := &TableSession{State: contracts.TableSessionStateOrdered}
	if got := DeriveDisplayState(sess); got != string(contracts.TableSessionStateOrdered) {
		t.Fatalf("expected ORDERED, got %s", got)
	}
}

func TestGetOpenSession_NilWhenAvailable(t *testing.T) {
	outletID, tableID := id.New(), id.New()
	repo := newFakeRepository(outletID)
	repo.tables = append(repo.tables, RestaurantTable{ID: tableID, OutletID: outletID, Label: "T1"})
	svc := NewService(repo)

	sess, err := svc.GetOpenSession(context.Background(), tableID)
	if err != nil {
		t.Fatalf("GetOpenSession: %v", err)
	}
	if sess != nil {
		t.Fatalf("expected nil (available) for a table with no session, got %+v", sess)
	}
}

func TestOpenSession_DefaultsOpenedAtWhenZero(t *testing.T) {
	outletID, tableID := id.New(), id.New()
	repo := newFakeRepository(outletID)
	repo.tables = append(repo.tables, RestaurantTable{ID: tableID, OutletID: outletID, Label: "T1"})
	svc := NewService(repo)

	sess, err := svc.OpenSession(context.Background(), OpenSessionInput{OutletID: outletID, TableID: tableID, GuestCount: 2})
	if err != nil {
		t.Fatalf("OpenSession: %v", err)
	}
	if sess.OpenedAt.IsZero() {
		t.Fatal("expected opened_at to be set")
	}
	if sess.OpenedAt.Location() != time.UTC {
		t.Fatalf("expected opened_at stored in UTC, got %v", sess.OpenedAt.Location())
	}
}
