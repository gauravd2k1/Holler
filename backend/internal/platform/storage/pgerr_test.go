package storage

import (
	"errors"
	"fmt"
	"strings"
	"testing"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/jackc/pgx/v5/pgconn"
)

// The real error a replayed order item produces, reproduced field for field
// from the observed 2026-09-02 failure so this table is not a guess about
// what pgx returns.
func fkViolation() *pgconn.PgError {
	return &pgconn.PgError{
		Severity:       "ERROR",
		Code:           SQLStateForeignKeyViolation,
		Message:        `insert or update on table "order_item" violates foreign key constraint "order_item_menu_item_id_fkey"`,
		Detail:         `Key (menu_item_id)=(01a05ea9-118e-7071-8fc5-a01a5690be29) is not present in table "menu_item".`,
		TableName:      "order_item",
		ConstraintName: "order_item_menu_item_id_fkey",
	}
}

func TestClassify_ForeignKeyViolationIsMissingReference(t *testing.T) {
	got := Classify(fmt.Errorf("ordering: appending item: %w", fkViolation()))

	if !errors.Is(got, httpx.ErrMissingReference) {
		t.Fatalf("Classify(23503) = %v, want it to match httpx.ErrMissingReference", got)
	}
	if !strings.Contains(got.Error(), "menu_item_id") {
		t.Errorf("message %q does not name the field; the operator cannot act on it", got.Error())
	}
}

// The message reaches a client verbatim for 4xx codes, so it must carry the
// field and nothing else. Detail holds the offending VALUE — business data —
// and the constraint name is internal noise ending in "_fkey".
func TestClassify_MessageLeaksNoInternalDetail(t *testing.T) {
	msg := Classify(fkViolation()).Error()

	for _, forbidden := range []string{"fkey", "SQLSTATE", "23503", "insert or update", "Key (", "01a05ea9"} {
		if strings.Contains(strings.ToLower(msg), strings.ToLower(forbidden)) {
			t.Errorf("message %q leaks %q", msg, forbidden)
		}
	}
}

func TestClassify_OtherSQLStates(t *testing.T) {
	cases := []struct {
		name   string
		code   string
		target error
	}{
		{"unique violation", SQLStateUniqueViolation, httpx.ErrConflict},
		{"check violation", SQLStateCheckViolation, httpx.ErrInvalidInput},
		{"not null violation", SQLStateNotNullViolation, httpx.ErrInvalidInput},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := Classify(&pgconn.PgError{Code: tc.code})
			if !errors.Is(got, tc.target) {
				t.Errorf("Classify(%s) = %v, want %v", tc.code, got, tc.target)
			}
		})
	}
}

// An error this package has no opinion about must come back untouched, so
// adopting Classify in a repository cannot silently re-map a failure that
// something upstream already handles.
func TestClassify_UnknownErrorsPassThroughUnchanged(t *testing.T) {
	sentinel := errors.New("some other failure")
	if got := Classify(sentinel); got != sentinel {
		t.Errorf("Classify(non-pg error) = %v, want the original error", got)
	}

	unmapped := &pgconn.PgError{Code: "40001"} // serialization_failure
	if got := Classify(unmapped); !errors.Is(got, unmapped) {
		t.Errorf("Classify(40001) = %v, want the original error", got)
	}
	if Classify(nil) != nil {
		t.Errorf("Classify(nil) must be nil")
	}
}

// Wrap keeps the caller's context for anything that will only be logged, and
// drops it for a classified error, whose text reaches a client.
func TestWrap(t *testing.T) {
	classified := Wrap("ordering: appending item", fkViolation())
	if strings.Contains(classified.Error(), "ordering: appending item") {
		t.Errorf("classified error %q kept the caller's internal prose", classified.Error())
	}

	other := Wrap("ordering: appending item", errors.New("connection reset"))
	if !strings.HasPrefix(other.Error(), "ordering: appending item: ") {
		t.Errorf("unclassified error %q lost the caller's context", other.Error())
	}
	if Wrap("ctx", nil) != nil {
		t.Errorf("Wrap(nil) must be nil")
	}
}

// PostgreSQL names constraints "<table>_<column>_fkey" by default, but a
// hand-named constraint must still produce a usable message rather than an
// empty one.
func TestReferencedField_Fallbacks(t *testing.T) {
	cases := []struct {
		name string
		err  *pgconn.PgError
		want string
	}{
		{"default naming", fkViolation(), "menu_item_id"},
		{"hand-named constraint", &pgconn.PgError{ConstraintName: "grn_must_have_supplier", TableName: "grn_line"}, "grn_must_have_supplier"},
		{"no constraint name", &pgconn.PgError{TableName: "grn_line"}, "a referenced record"},
		{"constraint equals table prefix only", &pgconn.PgError{ConstraintName: "grn_line_fkey", TableName: "grn_line"}, "a referenced record"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := referencedField(tc.err); got != tc.want {
				t.Errorf("referencedField = %q, want %q", got, tc.want)
			}
		})
	}
}
