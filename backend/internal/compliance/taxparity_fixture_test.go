package compliance

import (
	"flag"
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

// writeFixtureFlag is this test's on-demand regeneration switch:
//
//	go test ./internal/compliance/ -run TestGenerateTaxParityFixture \
//	  -write-fixture=<path>
//
// writes the freshly generated JSON to <path>. It is deliberately never
// pointed at edge/database itself from inside this package — a human (or
// the track that owns edge/database) copies the output over
// edge/database/tests/fixtures/tax_parity.json after reviewing it.
var writeFixtureFlag = flag.String("write-fixture", "", "if set, write the regenerated tax_parity fixture JSON to this path")

// committedTaxParityFixturePath resolves the path to
// edge/database/tests/fixtures/tax_parity.json relative to THIS source
// file, not the process's working directory, so the test behaves the same
// whether invoked as `go test ./...` from backend/ or targeted directly.
func committedTaxParityFixturePath(t *testing.T) string {
	t.Helper()
	_, thisFile, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("could not resolve this test file's own path via runtime.Caller")
	}
	// this file:      backend/internal/compliance/taxparity_fixture_test.go
	// target fixture: edge/database/tests/fixtures/tax_parity.json
	repoRoot := filepath.Join(filepath.Dir(thisFile), "..", "..", "..")
	return filepath.Join(repoRoot, "edge", "database", "tests", "fixtures", "tax_parity.json")
}

// TestGenerateTaxParityFixture_MatchesCommittedFixture is T14's regeneration
// path and its own proof of durability in one test:
//
//  1. It runs this package's live ComputeInvoice over the 10 cases
//     edge/database/tests/tax_parity.rs requires, via generateTaxParityFixture.
//  2. It writes that output to a fresh temp file (never into edge/database —
//     this package only reads that tree, never writes it) and, separately,
//     to -write-fixture's path if the flag is set, so a human has a runnable
//     command to regenerate the committed fixture on purpose.
//  3. It reads the COMMITTED edge/database/tests/fixtures/tax_parity.json
//     (read-only) and diffs it byte-for-byte against the fresh output.
//
// A mismatch here means exactly one of two things, and the failure message
// says which to check first: either this generator mis-transcribed a case's
// input lines (a generator bug — compare against the case in
// taxparity_fixture.go), or backend/internal/compliance's arithmetic itself
// has changed since the fixture was committed (a real cross-engine
// divergence — edge/database/tests/tax_parity.rs would then also start
// failing once the fixture is regenerated for real, which is exactly the
// signal T14 exists to make impossible to miss).
//
// Any change to this package's tax arithmetic (a rate table, a rounding
// rule, an allocation order — anything ComputeInvoice/computeLineBase/
// finishInclusiveLine/rounding.go touches) requires regenerating
// edge/database/tests/fixtures/tax_parity.json via the -write-fixture
// command above and re-running edge/database's
// `cargo test -p holler_edge_database --test tax_parity`.
func TestGenerateTaxParityFixture_MatchesCommittedFixture(t *testing.T) {
	fixture, err := generateTaxParityFixture()
	if err != nil {
		t.Fatalf("generateTaxParityFixture: %v", err)
	}
	if len(fixture.ComputeCases) != 10 {
		t.Fatalf("expected all 10 documented parity cases, got %d", len(fixture.ComputeCases))
	}

	generated, err := marshalTaxParityFixture(fixture)
	if err != nil {
		t.Fatalf("marshalling generated fixture: %v", err)
	}

	tempPath := filepath.Join(t.TempDir(), "tax_parity.json")
	if err := os.WriteFile(tempPath, generated, 0o644); err != nil {
		t.Fatalf("writing generated fixture to temp path: %v", err)
	}

	if *writeFixtureFlag != "" {
		if err := os.WriteFile(*writeFixtureFlag, generated, 0o644); err != nil {
			t.Fatalf("writing generated fixture to -write-fixture path %q: %v", *writeFixtureFlag, err)
		}
		t.Logf("wrote regenerated fixture to %s", *writeFixtureFlag)
	}

	committedPath := committedTaxParityFixturePath(t)
	committed, err := os.ReadFile(committedPath)
	if err != nil {
		t.Fatalf("reading committed fixture at %s (read-only — this test never writes here): %v", committedPath, err)
	}

	if string(committed) != string(generated) {
		t.Fatalf(
			"generated tax_parity fixture (from temp file %s) differs from the committed "+
				"%s.\n\nThis is EITHER a generator bug in taxparity_fixture.go (a case's "+
				"transcribed input lines don't match the committed fixture's \"lines\") OR a "+
				"real divergence: backend/internal/compliance's arithmetic has changed since "+
				"the fixture was committed, and edge/database's Rust suite is now testing "+
				"against a stale reference. Regenerate with:\n\n"+
				"  go test ./internal/compliance/ -run TestGenerateTaxParityFixture_MatchesCommittedFixture -write-fixture=<path>\n\n"+
				"and diff <path> against %s by hand to see which case moved.",
			tempPath, committedPath, committedPath,
		)
	}
}
