package tables

import (
	"context"
	"errors"
	"testing"

	contracts "github.com/holler/contracts"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
	"github.com/holler/backend/internal/platform/testdb"
)

// TestRepository_Postgres exercises the real pgRepository against a live
// PostgreSQL instance. See internal/platform/testdb: an unset
// HOLLER_TEST_DATABASE_URL fails this test loudly by default.
func TestRepository_Postgres(t *testing.T) {
	dsn := testdb.RequireDatabaseURL(t)

	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connecting to test database: %v", err)
	}
	defer pool.Close()

	tenantID := id.New()
	brandID := id.New()
	outletID := id.New()

	if _, err := pool.Exec(ctx, `INSERT INTO tenant (id, name) VALUES ($1, 'Integration Test Tenant')`, tenantID); err != nil {
		t.Fatalf("seeding tenant: %v", err)
	}
	if _, err := pool.Exec(ctx, `INSERT INTO brand (id, tenant_id, name) VALUES ($1, $2, 'Integration Test Brand')`, brandID, tenantID); err != nil {
		t.Fatalf("seeding brand: %v", err)
	}
	if _, err := pool.Exec(ctx, `INSERT INTO outlet (id, brand_id, name, config_version) VALUES ($1, $2, 'Integration Test Outlet', 0)`, outletID, brandID); err != nil {
		t.Fatalf("seeding outlet: %v", err)
	}
	t.Cleanup(func() {
		pool.Exec(ctx, `DELETE FROM table_session WHERE outlet_id = $1`, outletID)
		pool.Exec(ctx, `DELETE FROM restaurant_table WHERE outlet_id = $1`, outletID)
		pool.Exec(ctx, `DELETE FROM outlet WHERE id = $1`, outletID)
		pool.Exec(ctx, `DELETE FROM brand WHERE id = $1`, brandID)
		pool.Exec(ctx, `DELETE FROM tenant WHERE id = $1`, tenantID)
	})

	repo := NewRepository(pool)
	svc := NewService(repo)

	table, err := svc.CreateTable(authorizedContext(), NewTableInput{
		OutletID: outletID, Section: "GROUND", Label: "T1", SeatCount: 4,
	})
	if err != nil {
		t.Fatalf("CreateTable: %v", err)
	}
	if table.ConfigVersion != 1 {
		t.Fatalf("expected first table write to bump outlet config_version to 1, got %d", table.ConfigVersion)
	}

	tables, err := svc.ListTables(context.Background(), outletID)
	if err != nil {
		t.Fatalf("ListTables: %v", err)
	}
	if len(tables) != 1 {
		t.Fatalf("expected 1 table, got %d", len(tables))
	}

	sess, err := svc.OpenSession(context.Background(), OpenSessionInput{
		OutletID: outletID, TableID: table.ID, GuestCount: 3,
	})
	if err != nil {
		t.Fatalf("OpenSession: %v", err)
	}
	if sess.State != contracts.TableSessionStateOccupied {
		t.Fatalf("expected OCCUPIED, got %s", sess.State)
	}

	// The partial unique index must reject a second concurrent open session,
	// surfaced by the repository as httpx.ErrConflict, not a raw pg error.
	_, err = svc.OpenSession(context.Background(), OpenSessionInput{
		OutletID: outletID, TableID: table.ID, GuestCount: 2,
	})
	if !errors.Is(err, httpx.ErrConflict) {
		t.Fatalf("expected ErrConflict for a second open session on the same table, got %v", err)
	}

	sess, err = svc.TransitionSession(context.Background(), outletID, sess.ID, contracts.TableSessionStateOrdered, nil)
	if err != nil {
		t.Fatalf("TransitionSession: %v", err)
	}
	if sess.Version != 2 {
		t.Fatalf("expected session version 2 after one transition, got %d", sess.Version)
	}

	// A session write must never touch the table's config_version.
	tablesAfter, err := svc.ListTables(context.Background(), outletID)
	if err != nil {
		t.Fatalf("ListTables (after session write): %v", err)
	}
	if tablesAfter[0].ConfigVersion != 1 {
		t.Fatalf("expected table config_version to stay at 1 after session writes, got %d", tablesAfter[0].ConfigVersion)
	}

	if _, err := svc.CloseSession(context.Background(), outletID, sess.ID); err != nil {
		t.Fatalf("CloseSession: %v", err)
	}

	// Closing frees the table for a new session.
	if _, err := svc.OpenSession(context.Background(), OpenSessionInput{OutletID: outletID, TableID: table.ID, GuestCount: 5}); err != nil {
		t.Fatalf("OpenSession (re-seat after close): %v", err)
	}
}
