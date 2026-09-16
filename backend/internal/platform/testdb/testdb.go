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
		requireScratchDatabase(t, dbURL)
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

// ScratchDatabasePrefix is the only name a database may have if this suite is
// allowed to touch it.
//
// WHY A SUITE REFUSES A DATABASE RATHER THAN TRUSTING THE CALLER. Every
// Postgres-backed test here migrates the schema and seeds fixtures. Run against
// the shared dev database - which is what docs/RESUME.md used to tell you to do
// - it overwrote owner@holler.test and cashier@holler.test with fixture
// hashes, so the till and the admin console then refused a CORRECT password.
// The 401 was misread first as a credential fault and then as the rate
// limiter, and the only way back was the operator's reset. Nothing in the
// suite was broken and nothing said anything; a working dev stack was simply
// gone.
//
// This is the same rule scripts/agent-guard.ps1 enforces for demo-reset.ps1,
// and it is spelled there too because that script has no Go anywhere near it.
// scripts/check-scratch-db-guard.mjs fails the build if the two spellings
// disagree.
//
// It applies to every shell, not only an agent's. The operator has no more
// reason to migrate fixtures over their own dev database than an agent does,
// and the incident that produced this rule was not an agent's.
const ScratchDatabasePrefix = "holler_scratch_"

// databaseNameFromURL returns the database a postgres URL names: the last path
// segment, query string removed. An empty return means "no name found", and
// the caller treats that as a refusal rather than as permission - a URL this
// cannot parse is not a URL it can clear.
func databaseNameFromURL(dbURL string) string {
	withoutQuery, _, _ := strings.Cut(dbURL, "?")
	withoutQuery = strings.TrimRight(withoutQuery, "/")
	idx := strings.LastIndex(withoutQuery, "/")
	if idx < 0 || idx == len(withoutQuery)-1 {
		return ""
	}
	return withoutQuery[idx+1:]
}

// requireScratchDatabase fails the test - loudly, before a single migration
// runs - when HOLLER_TEST_DATABASE_URL names anything but a scratch database.
func requireScratchDatabase(t *testing.T, dbURL string) {
	t.Helper()

	name := databaseNameFromURL(dbURL)
	if strings.HasPrefix(strings.ToLower(name), ScratchDatabasePrefix) {
		return
	}
	shown := name
	if shown == "" {
		shown = "<no database name in the URL>"
	}
	t.Fatalf("HOLLER_TEST_DATABASE_URL names database %q, which is not a scratch "+
		"database. This suite MIGRATES AND SEEDS whatever it is pointed at: run "+
		"against a working database it overwrites owner@holler.test and "+
		"cashier@holler.test with fixture hashes, and the till then refuses a "+
		"correct password with a 401 that reads exactly like a wrong one. Use a "+
		"database named %q..., which you create and drop yourself, e.g. "+
		"postgres://holler:holler_dev@localhost:5432/%sci?sslmode=disable",
		shown, ScratchDatabasePrefix, ScratchDatabasePrefix)
}

func truthy(v string) bool {
	switch strings.ToLower(strings.TrimSpace(v)) {
	case "1", "true", "yes", "on":
		return true
	default:
		return false
	}
}
