package tables

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	contracts "github.com/holler/contracts"
	"github.com/jackc/pgx/v5"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
)

// Service holds the tables context's business logic. HTTP handlers, and the
// future sync ingest worker, call this — it never touches pgx directly
// (CLAUDE.md §Coding rules).
type Service struct {
	repo Repository
	now  func() time.Time
}

func NewService(repo Repository) *Service {
	return &Service{repo: repo, now: time.Now}
}

// --- RestaurantTable: pure configuration, cloud→edge ---------------------

// NewTableInput is what a caller supplies to define a physical table.
type NewTableInput struct {
	OutletID  string
	Section   string
	Label     string
	SeatCount int
}

func (s *Service) ListTables(ctx context.Context, outletID string) ([]RestaurantTable, error) {
	if strings.TrimSpace(outletID) == "" {
		return nil, fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	return s.repo.ListTables(ctx, outletID)
}

// CreateTable defines a new physical table. This is a config write like
// menu's: it bumps the outlet's config_version exactly once and never
// touches table_session (ADR-011).
func (s *Service) CreateTable(ctx context.Context, in NewTableInput) (RestaurantTable, error) {
	if err := requirePermission(ctx, permTableManage); err != nil {
		return RestaurantTable{}, err
	}
	if err := validateNewTableInput(in); err != nil {
		return RestaurantTable{}, err
	}

	taken, err := s.repo.TableLabelTaken(ctx, in.OutletID, in.Label)
	if err != nil {
		return RestaurantTable{}, err
	}
	if taken {
		return RestaurantTable{}, fmt.Errorf("%w: table label %q already exists in this outlet", httpx.ErrConflict, in.Label)
	}

	t := RestaurantTable{
		ID:            id.New(),
		OutletID:      in.OutletID,
		Section:       in.Section,
		Label:         in.Label,
		SeatCount:     in.SeatCount,
		IsActive:      true,
		SchemaVersion: 1,
	}

	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, in.OutletID)
		if err != nil {
			return err
		}
		t.ConfigVersion = newVersion
		return s.repo.InsertTable(ctx, tx, t)
	})
	if err != nil {
		return RestaurantTable{}, err
	}
	return t, nil
}

func validateNewTableInput(in NewTableInput) error {
	if strings.TrimSpace(in.OutletID) == "" {
		return fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.Section) == "" {
		return fmt.Errorf("%w: section is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.Label) == "" {
		return fmt.Errorf("%w: label is required", httpx.ErrInvalidInput)
	}
	if in.SeatCount < 1 {
		return fmt.Errorf("%w: seat_count must be at least 1", httpx.ErrInvalidInput)
	}
	return nil
}

// --- TableSession: operational, edge→cloud append-only replay ------------
//
// The cloud never originates a session on its own initiative — it accepts
// what the edge replays. These methods are the ingest and read path a future
// sync worker calls; Milestone 1 defines no HTTP surface for them (the
// openapi.yaml table endpoints are RestaurantTable-only).

// OpenSessionInput is a replayed "table opened" event from the edge.
// SessionID and Version, when set, are the edge-assigned record_id/version
// from the sync envelope — the cloud never mints a session id (ADR-011). When
// both are empty/zero (as in direct, non-replay callers such as tests), the
// service assigns a fresh id and starts at version 1.
type OpenSessionInput struct {
	SessionID      string
	Version        int
	OutletID       string
	TableID        string
	GuestCount     int
	OpenedByUserID *string
	OpenedAt       time.Time
}

// OpenSession records a new seating. It enforces at most one open session
// per table (uq_table_session_open) by failing cleanly as httpx.ErrConflict
// rather than letting a raw constraint violation reach the caller.
//
// It is idempotent when the caller supplies SessionID: replaying the exact
// same "session opened" event twice (the edge retries on a dropped ack) must
// leave exactly one session in one state, so a second call carrying a
// session id that already exists for the same table returns the existing
// row rather than erroring.
func (s *Service) OpenSession(ctx context.Context, in OpenSessionInput) (TableSession, error) {
	if strings.TrimSpace(in.OutletID) == "" {
		return TableSession{}, fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.TableID) == "" {
		return TableSession{}, fmt.Errorf("%w: table_id is required", httpx.ErrInvalidInput)
	}
	if in.GuestCount < 1 {
		return TableSession{}, fmt.Errorf("%w: guest_count must be at least 1", httpx.ErrInvalidInput)
	}

	if strings.TrimSpace(in.SessionID) != "" {
		existing, err := s.repo.GetSession(ctx, in.OutletID, in.SessionID)
		if err == nil {
			if existing.TableID != in.TableID {
				return TableSession{}, fmt.Errorf("%w: session %s is already recorded against a different table", httpx.ErrConflict, in.SessionID)
			}
			return existing, nil
		}
		if !errors.Is(err, httpx.ErrNotFound) {
			return TableSession{}, err
		}
	}

	exists, err := s.repo.TableExists(ctx, in.OutletID, in.TableID)
	if err != nil {
		return TableSession{}, err
	}
	if !exists {
		return TableSession{}, fmt.Errorf("%w: table %s does not belong to outlet %s", httpx.ErrInvalidInput, in.TableID, in.OutletID)
	}

	if _, open, err := s.repo.GetOpenSessionByTable(ctx, in.TableID); err != nil {
		return TableSession{}, err
	} else if open {
		return TableSession{}, fmt.Errorf("%w: table %s already has an open session", httpx.ErrConflict, in.TableID)
	}

	sessionID := in.SessionID
	if strings.TrimSpace(sessionID) == "" {
		sessionID = id.New()
	}
	openedAt := in.OpenedAt
	if openedAt.IsZero() {
		openedAt = s.now()
	}
	openedAt = openedAt.UTC()

	version := in.Version
	if version < 1 {
		version = 1
	}

	sess := TableSession{
		ID:             sessionID,
		OutletID:       in.OutletID,
		TableID:        in.TableID,
		State:          contracts.TableSessionStateOccupied,
		GuestCount:     in.GuestCount,
		OpenedByUserID: in.OpenedByUserID,
		OpenedAt:       openedAt,
		Version:        version,
		CreatedAt:      openedAt,
		UpdatedAt:      openedAt,
		SchemaVersion:  1,
	}

	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		return s.repo.InsertSession(ctx, tx, sess)
	})
	if err != nil {
		if errors.Is(err, httpx.ErrConflict) {
			// Lost the idempotency pre-check race: another replay of the same
			// record_id landed first. Return that row rather than erroring.
			if existing, getErr := s.repo.GetSession(ctx, in.OutletID, sessionID); getErr == nil && existing.TableID == in.TableID {
				return existing, nil
			}
		}
		return TableSession{}, err
	}
	return sess, nil
}

// TransitionSession moves a session to newState, validating the edge is
// legal in the state machine. It never touches restaurant_table or bumps
// config_version — session writes are a separate aggregate (ADR-011).
func (s *Service) TransitionSession(ctx context.Context, outletID, sessionID string, newState TableSessionState, currentOrderID *string) (TableSession, error) {
	current, err := s.repo.GetSession(ctx, outletID, sessionID)
	if err != nil {
		return TableSession{}, err
	}
	if current.ClosedAt != nil {
		return TableSession{}, fmt.Errorf("%w: table session %s is already closed", httpx.ErrInvalidInput, sessionID)
	}
	if err := validateTransition(current.State, newState); err != nil {
		return TableSession{}, err
	}

	updated := current
	updated.State = newState
	if currentOrderID != nil {
		updated.CurrentOrderID = currentOrderID
	}
	updated.Version = current.Version + 1
	updated.UpdatedAt = s.now().UTC()
	if newState == contracts.TableSessionStateClosed {
		closedAt := updated.UpdatedAt
		updated.ClosedAt = &closedAt
	}

	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		return s.repo.UpdateSession(ctx, tx, updated)
	})
	if err != nil {
		return TableSession{}, err
	}
	return updated, nil
}

// CloseSession is a convenience wrapper over TransitionSession(..., CLOSED,
// ...) for the edge's "table closed" replay event.
func (s *Service) CloseSession(ctx context.Context, outletID, sessionID string) (TableSession, error) {
	return s.TransitionSession(ctx, outletID, sessionID, contracts.TableSessionStateClosed, nil)
}

// ReplayTransition applies an edge-assigned transition or close event
// carrying its own envelope version, and is idempotent: replaying the exact
// same event (identical target state at the version it already produced)
// must leave the session in exactly that one state rather than erroring or
// advancing further. Older/duplicate replays (envelopeVersion at or below
// the stored version) are no-ops that return the current row unchanged.
// Only a genuinely new version advances the state, and only along a legal
// edge of the state machine — an illegal transition is rejected, not
// applied.
func (s *Service) ReplayTransition(ctx context.Context, outletID, sessionID string, newState TableSessionState, currentOrderID *string, envelopeVersion int) (TableSession, error) {
	current, err := s.repo.GetSession(ctx, outletID, sessionID)
	if err != nil {
		return TableSession{}, err
	}

	if envelopeVersion <= current.Version {
		// Stale or duplicate replay of an already-applied event. Idempotent:
		// return what is already stored without touching it.
		return current, nil
	}
	if current.ClosedAt != nil {
		return TableSession{}, fmt.Errorf("%w: table session %s is already closed", httpx.ErrInvalidInput, sessionID)
	}
	if err := validateTransition(current.State, newState); err != nil {
		return TableSession{}, err
	}

	updated := current
	updated.State = newState
	if currentOrderID != nil {
		updated.CurrentOrderID = currentOrderID
	}
	updated.Version = envelopeVersion
	updated.UpdatedAt = s.now().UTC()
	if newState == contracts.TableSessionStateClosed {
		closedAt := updated.UpdatedAt
		updated.ClosedAt = &closedAt
	}

	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		return s.repo.UpdateSession(ctx, tx, updated)
	})
	if err != nil {
		return TableSession{}, err
	}
	return updated, nil
}

// GetSession returns a single session, scoped to the outlet — the read side
// of the envelope-ingest routes (GET is unwrapped, never an envelope).
func (s *Service) GetSession(ctx context.Context, outletID, sessionID string) (TableSession, error) {
	return s.repo.GetSession(ctx, outletID, sessionID)
}

// GetOpenSession returns the table's current seating, if any. nil means the
// table is available.
func (s *Service) GetOpenSession(ctx context.Context, tableID string) (*TableSession, error) {
	sess, open, err := s.repo.GetOpenSessionByTable(ctx, tableID)
	if err != nil {
		return nil, err
	}
	if !open {
		return nil, nil
	}
	return &sess, nil
}

// ListOpenSessions returns every currently-seated table for an outlet.
func (s *Service) ListOpenSessions(ctx context.Context, outletID string) ([]TableSession, error) {
	if strings.TrimSpace(outletID) == "" {
		return nil, fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	return s.repo.ListOpenSessions(ctx, outletID)
}

func requirePermission(ctx context.Context, permission string) error {
	p, ok := PrincipalFromContext(ctx)
	if !ok {
		return httpx.ErrUnauthorized
	}
	if !p.HasPermission(permission) {
		return httpx.ErrForbidden
	}
	return nil
}
