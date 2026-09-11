package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// TestLoadSeedFile_DecodesFixture exercises the decode+validate path against
// a fixture shaped like seed/demo-outlet.json (seed/README.md's "File
// format"). It does not touch Postgres.
func TestLoadSeedFile_DecodesFixture(t *testing.T) {
	sf, err := loadSeedFile("testdata/demo-outlet-fixture.json")
	if err != nil {
		t.Fatalf("loadSeedFile: %v", err)
	}

	if len(sf.MenuItems) == 0 {
		t.Fatal("expected at least one menu_item in the fixture")
	}
	if sf.MenuItems[0].HsnSac == "" {
		t.Fatal("fixture menu_item has a blank hsn_sac -- fixture is broken, not the code under test")
	}
	if len(sf.RecipeIngredients) == 0 {
		t.Fatal("expected at least one recipe_ingredient in the fixture")
	}
	if sf.RecipeIngredients[0].QuantityDimension == "" {
		t.Fatal("fixture recipe_ingredient has a blank quantity_dimension -- fixture is broken, not the code under test")
	}
	if sf.GoodsReceipt == nil {
		t.Fatal("expected a goods_receipt in the fixture")
	}
	if len(sf.GoodsReceipt.Lines) == 0 {
		t.Fatal("expected at least one grn line in the fixture")
	}
}

// TestLoadSeedFile_RejectsBlankHsnSac falsifies the guard from contracts
// 0.4.5: an invoice cannot issue without hsn_sac, so the loader must refuse
// a menu_item carrying a blank one rather than silently accepting it.
func TestLoadSeedFile_RejectsBlankHsnSac(t *testing.T) {
	path := writeMutatedFixture(t, func(s string) string {
		return strings.Replace(s, `"hsn_sac": "9963"`, `"hsn_sac": ""`, 1)
	})

	_, err := loadSeedFile(path)
	if err == nil {
		t.Fatal("expected loadSeedFile to reject a blank hsn_sac, got nil error")
	}
	if !strings.Contains(err.Error(), "hsn_sac") {
		t.Fatalf("expected the error to name hsn_sac, got: %v", err)
	}
}

// TestLoadSeedFile_RejectsBlankQuantityDimension falsifies the contracts
// 0.5.2 guard: quantity_dimension is the author's chosen unit and must never
// be silently absent.
func TestLoadSeedFile_RejectsBlankQuantityDimension(t *testing.T) {
	path := writeMutatedFixture(t, func(s string) string {
		return strings.Replace(s,
			`"quantity_micro": 5000000, "quantity_dimension": "MASS"`,
			`"quantity_micro": 5000000, "quantity_dimension": ""`, 1)
	})

	_, err := loadSeedFile(path)
	if err == nil {
		t.Fatal("expected loadSeedFile to reject a blank quantity_dimension, got nil error")
	}
	if !strings.Contains(err.Error(), "quantity_dimension") {
		t.Fatalf("expected the error to name quantity_dimension, got: %v", err)
	}
}

// TestLoadSeedFile_RejectsTenantIDMismatch falsifies the pinned-id check: a
// seed file describing a different tenant than the hand-pinned devseed
// constants must be refused loudly, not silently accepted with role/user
// rows then pointing at the wrong outlet.
func TestLoadSeedFile_RejectsTenantIDMismatch(t *testing.T) {
	path := writeMutatedFixture(t, func(s string) string {
		return strings.Replace(s,
			`"tenant": { "id": "0191a000-0000-7000-8000-000000000001"`,
			`"tenant": { "id": "0191a000-0000-7000-8000-0000000000ff"`, 1)
	})

	_, err := loadSeedFile(path)
	if err == nil {
		t.Fatal("expected loadSeedFile to reject a tenant id mismatch, got nil error")
	}
	if !strings.Contains(err.Error(), "tenant.id") {
		t.Fatalf("expected the error to name tenant.id, got: %v", err)
	}
}

// TestLoadSeedFile_RejectsUnknownField falsifies strict decoding: a field
// the JSON carries that this reader does not declare must fail the decode,
// never be silently discarded (the contracts 0.5.9 defect, one hop earlier
// -- see seedfile.go's loadSeedFile doc comment).
func TestLoadSeedFile_RejectsUnknownField(t *testing.T) {
	path := writeMutatedFixture(t, func(s string) string {
		return strings.Replace(s,
			`"schema_version": 1,`,
			`"schema_version": 1, "an_unrecognised_field": "should fail the decode",`, 1)
	})

	_, err := loadSeedFile(path)
	if err == nil {
		t.Fatal("expected loadSeedFile to reject an unknown top-level field, got nil error")
	}
}

func writeMutatedFixture(t *testing.T, mutate func(string) string) string {
	t.Helper()
	raw, err := os.ReadFile("testdata/demo-outlet-fixture.json")
	if err != nil {
		t.Fatalf("reading base fixture: %v", err)
	}
	mutated := mutate(string(raw))
	if mutated == string(raw) {
		t.Fatal("mutate function did not change the fixture -- the target string was not found")
	}
	path := filepath.Join(t.TempDir(), "mutated.json")
	if err := os.WriteFile(path, []byte(mutated), 0o600); err != nil {
		t.Fatalf("writing mutated fixture: %v", err)
	}
	return path
}
