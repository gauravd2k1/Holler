// Package testdb centralizes how backend integration tests decide whether a
// live Postgres is available. Every Postgres-backed test in this module
// must call RequireDatabaseURL rather than reading
// HOLLER_TEST_DATABASE_URL directly, so the fail/skip decision is made in
// exactly one place instead of once per package.
//
// The default, when HOLLER_TEST_DATABASE_URL is unset, is a loud failure —
// not a silent skip. A green `go test ./...` run must not be achievable by
// forgetting to export the variable: that shape has already produced two
// separate M2 acceptance failures (see docs/RESUME.md). A developer who
// deliberately has no local Postgres and wants these tests out of the way
// must say so explicitly by setting HOLLER_SKIP_PG_TESTS=1, which still
// skips (not fails) — but that is an opt-in, not a default.
package testdb

import (
	"os"
	"strings"
	"testing"
)

// RequireDatabaseURL returns the value of HOLLER_TEST_DATABASE_URL for use
// by a Postgres-backed integration test.
//
//   - If it is set, its value is returned and the test proceeds.
//   - If it is unset and HOLLER_SKIP_PG_TESTS is set to a truthy value, the
//     test is skipped (t.Skip) — an explicit, deliberate opt-out.
//   - If it is unset and HOLLER_SKIP_PG_TESTS is not set, the test FAILS
//     (t.Fatal), not skips. An unset database URL must never be
//     indistinguishable from a passing suite.
func RequireDatabaseURL(t *testing.T) string {
	t.Helper()

	dbURL := os.Getenv("HOLLER_TEST_DATABASE_URL")
	if dbURL != "" {
		return dbURL
	}

	if truthy(os.Getenv("HOLLER_SKIP_PG_TESTS")) {
		t.Skip("HOLLER_TEST_DATABASE_URL not set and HOLLER_SKIP_PG_TESTS " +
			"opts out explicitly; skipping Postgres integration test")
	}

	t.Fatal("HOLLER_TEST_DATABASE_URL is not set. This test requires a " +
		"live Postgres. Either export HOLLER_TEST_DATABASE_URL (see " +
		"docs/RESUME.md for the docker-compose connection string), or, if " +
		"you deliberately want Postgres-backed tests skipped in this " +
		"environment, export HOLLER_SKIP_PG_TESTS=1. An unset variable no " +
		"longer skips silently: see backend/internal/platform/testdb.")
	return ""
}

func truthy(v string) bool {
	switch strings.ToLower(strings.TrimSpace(v)) {
	case "1", "true", "yes", "on":
		return true
	default:
		return false
	}
}
