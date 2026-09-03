// Package storage owns the ONE translation from a PostgreSQL SQLSTATE to a
// domain error the HTTP layer already knows how to report.
//
// WHY THIS PACKAGE EXISTS. Before it, SQLSTATE knowledge lived in seven
// copy-pasted `isUniqueViolation` helpers (compliance, inventory, kitchen,
// outlet, payments, procurement, tables) and `23503` — foreign_key_violation
// — was handled in NONE of them. A replayed order item referencing a
// menu_item the cloud has never held therefore fell through httpx.Error's
// switch and came back 500 "internal_error": a permanent client-data fault
// reported as a transient server fault, which the edge then retried forever
// while every outbox row behind it waited (120 observed pending, 2026-09-02).
//
// Two rules this package exists to keep:
//
//   - The driver stays out of `httpx`. HTTP plumbing must not import pgconn,
//     so the mapping lives here and yields httpx sentinels, which the HTTP
//     layer already maps to status codes.
//   - There is ONE table of SQLSTATEs. Seven copies drift, and the drift is
//     invisible until the day one of them is missing the code that matters —
//     which is precisely what happened with 23503.
package storage

import (
	"errors"
	"fmt"
	"strings"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/jackc/pgx/v5/pgconn"
)

// PostgreSQL SQLSTATE codes, class 23 — integrity constraint violation.
const (
	// SQLStateForeignKeyViolation: the row references something that does
	// not exist. PERMANENT for a replayed edge row: retrying it unchanged
	// will fail identically forever.
	SQLStateForeignKeyViolation = "23503"
	// SQLStateUniqueViolation: the row is already there. For idempotent
	// replay this is usually caught by ON CONFLICT before it reaches here.
	SQLStateUniqueViolation = "23505"
	// SQLStateCheckViolation: the row violates a CHECK constraint — a value
	// outside what the schema permits.
	SQLStateCheckViolation = "23514"
	// SQLStateNotNullViolation: a required column arrived empty.
	SQLStateNotNullViolation = "23502"
)

// Classify maps a PostgreSQL integrity error onto the httpx sentinel that
// describes it, preserving the original error in the chain so a caller that
// wants the driver detail can still reach it and so logs keep the SQLSTATE.
//
// An error this package has no opinion about is returned UNCHANGED, so
// adopting Classify can never turn a currently-mapped failure into a
// differently-mapped one.
func Classify(err error) error {
	if err == nil {
		return nil
	}

	var pgErr *pgconn.PgError
	if !errors.As(err, &pgErr) {
		return err
	}

	switch pgErr.Code {
	case SQLStateForeignKeyViolation:
		// The message names the FIELD, never the constraint, the SQL or the
		// value: a constraint name ends in "_fkey" and reads as internal
		// detail, and Detail carries the offending value, which can be
		// business data. The field is what an operator and the edge both
		// need to act on.
		return fmt.Errorf("%w: %s does not exist", httpx.ErrMissingReference, referencedField(pgErr))
	case SQLStateUniqueViolation:
		return fmt.Errorf("%w: that record already exists", httpx.ErrConflict)
	case SQLStateCheckViolation, SQLStateNotNullViolation:
		return fmt.Errorf("%w: a required value is missing or out of range", httpx.ErrInvalidInput)
	default:
		return err
	}
}

// Wrap is what a repository's write path calls instead of fmt.Errorf.
//
// An integrity violation is returned as the classified domain error ALONE,
// deliberately unwrapped by the caller's own prose: httpx.Error reports
// err.Error() to the client for 4xx codes, and "ordering: appending item:
// missing reference: menu_item_id does not exist" tells a cashier nothing
// the shorter half does not. Anything else keeps the caller's context, since
// that error is only ever logged.
func Wrap(context string, err error) error {
	if err == nil {
		return nil
	}
	if classified := Classify(err); classified != err {
		return classified
	}
	return fmt.Errorf("%s: %w", context, err)
}

// IsUniqueViolation reports whether err is a unique_violation, replacing the
// seven local copies of this predicate. Kept as a predicate (rather than
// folded entirely into Classify) because several repositories branch on it
// to implement idempotent replay rather than to produce an HTTP status.
func IsUniqueViolation(err error) bool {
	var pgErr *pgconn.PgError
	return errors.As(err, &pgErr) && pgErr.Code == SQLStateUniqueViolation
}

// IsForeignKeyViolation reports whether err is a foreign_key_violation.
func IsForeignKeyViolation(err error) bool {
	var pgErr *pgconn.PgError
	return errors.As(err, &pgErr) && pgErr.Code == SQLStateForeignKeyViolation
}

// PgErrorOf returns the underlying *pgconn.PgError, if there is one, for a
// caller that needs the constraint name (e.g. to distinguish two unique
// indexes on the same table).
func PgErrorOf(err error) (*pgconn.PgError, bool) {
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) {
		return pgErr, true
	}
	return nil, false
}

// referencedField derives the offending column from the constraint name,
// which PostgreSQL builds as "<table>_<column>_fkey" by default:
// "order_item_menu_item_id_fkey" on table "order_item" yields
// "menu_item_id". A non-default constraint name that does not fit the
// pattern falls back to the constraint name with its "_fkey" suffix
// removed, and an empty constraint name to a generic phrase — never an
// empty message.
func referencedField(pgErr *pgconn.PgError) string {
	name := strings.TrimSuffix(pgErr.ConstraintName, "_fkey")
	if name == "" {
		return "a referenced record"
	}
	if pgErr.TableName != "" {
		name = strings.TrimPrefix(name, pgErr.TableName+"_")
	}
	// "grn_line_fkey" on table "grn_line" trims to the table's own name,
	// which names no field at all — "grn_line does not exist" would be
	// actively misleading, since the table plainly does.
	if name == "" || name == pgErr.TableName {
		return "a referenced record"
	}
	return name
}
