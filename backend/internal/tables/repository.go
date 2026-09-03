package tables

import (
	"context"
	"errors"
	"fmt"

	"github.com/jackc/pgx/v5"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/platform/storage"
)

// Repository is the persistence boundary for the tables context. The
// service depends on this interface, not on pgx directly.
type Repository interface {
	WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error

	// BumpOutletConfigVersion increments outlet.config_version by exactly
	// one, mirroring backend/internal/menu's discipline. Only
	// RestaurantTable writes call this — TableSession writes never touch
	// config_version (ADR-011).
	BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error)

	ListTables(ctx context.Context, outletID string) ([]RestaurantTable, error)
	InsertTable(ctx context.Context, tx pgx.Tx, t RestaurantTable) error
	TableExists(ctx context.Context, outletID, tableID string) (bool, error)
	TableLabelTaken(ctx context.Context, outletID, label string) (bool, error)

	InsertSession(ctx context.Context, tx pgx.Tx, s TableSession) error
	UpdateSession(ctx context.Context, tx pgx.Tx, s TableSession) error
	GetOpenSessionByTable(ctx context.Context, tableID string) (TableSession, bool, error)
	GetSession(ctx context.Context, outletID, sessionID string) (TableSession, error)
	ListOpenSessions(ctx context.Context, outletID string) ([]TableSession, error)
}

type pgRepository struct {
	pool postgres.Pool
}

// NewRepository returns a Repository backed by a live PostgreSQL pool.
func NewRepository(pool postgres.Pool) Repository {
	return &pgRepository{pool: pool}
}

func (r *pgRepository) WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("tables: begin tx: %w", err)
	}
	if err := fn(tx); err != nil {
		_ = tx.Rollback(ctx)
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("tables: commit tx: %w", err)
	}
	return nil
}

func (r *pgRepository) BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error) {
	var newVersion int
	err := tx.QueryRow(ctx,
		`UPDATE outlet SET config_version = config_version + 1, updated_at = now()
		 WHERE id = $1 RETURNING config_version`,
		outletID,
	).Scan(&newVersion)
	if errors.Is(err, pgx.ErrNoRows) {
		return 0, fmt.Errorf("%w: outlet %s", httpx.ErrNotFound, outletID)
	}
	if err != nil {
		return 0, fmt.Errorf("tables: bumping outlet config_version: %w", err)
	}
	return newVersion, nil
}

func (r *pgRepository) ListTables(ctx context.Context, outletID string) ([]RestaurantTable, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, outlet_id, section, label, seat_count, is_active, config_version
		 FROM restaurant_table WHERE outlet_id = $1 ORDER BY section, label`,
		outletID,
	)
	if err != nil {
		return nil, fmt.Errorf("tables: listing tables: %w", err)
	}
	defer rows.Close()

	var out []RestaurantTable
	for rows.Next() {
		var t RestaurantTable
		if err := rows.Scan(&t.ID, &t.OutletID, &t.Section, &t.Label, &t.SeatCount, &t.IsActive, &t.ConfigVersion); err != nil {
			return nil, fmt.Errorf("tables: scanning table: %w", err)
		}
		t.SchemaVersion = 1
		out = append(out, t)
	}
	return out, rows.Err()
}

func (r *pgRepository) InsertTable(ctx context.Context, tx pgx.Tx, t RestaurantTable) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO restaurant_table (id, outlet_id, section, label, seat_count, is_active, config_version, created_at, updated_at)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, now(), now())`,
		t.ID, t.OutletID, t.Section, t.Label, t.SeatCount, t.IsActive, t.ConfigVersion,
	)
	if err != nil {
		if storage.IsUniqueViolation(err) {
			return fmt.Errorf("%w: table label %q already exists in this outlet", httpx.ErrConflict, t.Label)
		}
		return fmt.Errorf("tables: inserting table: %w", err)
	}
	return nil
}

func (r *pgRepository) TableExists(ctx context.Context, outletID, tableID string) (bool, error) {
	var exists bool
	err := r.pool.QueryRow(ctx,
		`SELECT EXISTS(SELECT 1 FROM restaurant_table WHERE id = $1 AND outlet_id = $2)`,
		tableID, outletID,
	).Scan(&exists)
	if err != nil {
		return false, fmt.Errorf("tables: checking table existence: %w", err)
	}
	return exists, nil
}

func (r *pgRepository) TableLabelTaken(ctx context.Context, outletID, label string) (bool, error) {
	var exists bool
	err := r.pool.QueryRow(ctx,
		`SELECT EXISTS(SELECT 1 FROM restaurant_table WHERE outlet_id = $1 AND label = $2)`,
		outletID, label,
	).Scan(&exists)
	if err != nil {
		return false, fmt.Errorf("tables: checking table label: %w", err)
	}
	return exists, nil
}

func (r *pgRepository) InsertSession(ctx context.Context, tx pgx.Tx, s TableSession) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO table_session
		 (id, outlet_id, table_id, state, current_order_id, guest_count, opened_by_user_id, opened_at, closed_at, version, created_at, updated_at)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)`,
		s.ID, s.OutletID, s.TableID, s.State, s.CurrentOrderID, s.GuestCount, s.OpenedByUserID,
		s.OpenedAt, s.ClosedAt, s.Version, s.CreatedAt, s.UpdatedAt,
	)
	if err != nil {
		if storage.IsUniqueViolation(err) {
			return fmt.Errorf("%w: table %s already has an open session", httpx.ErrConflict, s.TableID)
		}
		return fmt.Errorf("tables: inserting session: %w", err)
	}
	return nil
}

func (r *pgRepository) UpdateSession(ctx context.Context, tx pgx.Tx, s TableSession) error {
	tag, err := tx.Exec(ctx,
		`UPDATE table_session
		 SET state = $1, current_order_id = $2, guest_count = $3, closed_at = $4,
		     version = $5, updated_at = $6
		 WHERE id = $7`,
		s.State, s.CurrentOrderID, s.GuestCount, s.ClosedAt, s.Version, s.UpdatedAt, s.ID,
	)
	if err != nil {
		return fmt.Errorf("tables: updating session: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("%w: table session %s", httpx.ErrNotFound, s.ID)
	}
	return nil
}

func (r *pgRepository) GetOpenSessionByTable(ctx context.Context, tableID string) (TableSession, bool, error) {
	s, err := scanSession(r.pool.QueryRow(ctx,
		`SELECT id, outlet_id, table_id, state, current_order_id, guest_count, opened_by_user_id,
		        opened_at, closed_at, version, created_at, updated_at
		 FROM table_session WHERE table_id = $1 AND closed_at IS NULL`,
		tableID,
	))
	if errors.Is(err, pgx.ErrNoRows) {
		return TableSession{}, false, nil
	}
	if err != nil {
		return TableSession{}, false, fmt.Errorf("tables: getting open session: %w", err)
	}
	return s, true, nil
}

func (r *pgRepository) GetSession(ctx context.Context, outletID, sessionID string) (TableSession, error) {
	s, err := scanSession(r.pool.QueryRow(ctx,
		`SELECT id, outlet_id, table_id, state, current_order_id, guest_count, opened_by_user_id,
		        opened_at, closed_at, version, created_at, updated_at
		 FROM table_session WHERE id = $1 AND outlet_id = $2`,
		sessionID, outletID,
	))
	if errors.Is(err, pgx.ErrNoRows) {
		return TableSession{}, fmt.Errorf("%w: table session %s", httpx.ErrNotFound, sessionID)
	}
	if err != nil {
		return TableSession{}, fmt.Errorf("tables: getting session: %w", err)
	}
	return s, nil
}

func (r *pgRepository) ListOpenSessions(ctx context.Context, outletID string) ([]TableSession, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, outlet_id, table_id, state, current_order_id, guest_count, opened_by_user_id,
		        opened_at, closed_at, version, created_at, updated_at
		 FROM table_session WHERE outlet_id = $1 AND closed_at IS NULL`,
		outletID,
	)
	if err != nil {
		return nil, fmt.Errorf("tables: listing open sessions: %w", err)
	}
	defer rows.Close()

	var out []TableSession
	for rows.Next() {
		s, err := scanSessionRow(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, s)
	}
	return out, rows.Err()
}

// rowScanner is satisfied by both pgx.Row and pgx.Rows.
type rowScanner interface {
	Scan(dest ...any) error
}

func scanSession(row rowScanner) (TableSession, error) {
	var s TableSession
	s.SchemaVersion = 1
	err := row.Scan(&s.ID, &s.OutletID, &s.TableID, &s.State, &s.CurrentOrderID, &s.GuestCount,
		&s.OpenedByUserID, &s.OpenedAt, &s.ClosedAt, &s.Version, &s.CreatedAt, &s.UpdatedAt)
	return s, err
}

func scanSessionRow(rows pgx.Rows) (TableSession, error) {
	s, err := scanSession(rows)
	if err != nil {
		return TableSession{}, fmt.Errorf("tables: scanning session: %w", err)
	}
	return s, nil
}
