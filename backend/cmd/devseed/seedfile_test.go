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

// TestLoadSeedFile_DecodesTheOutletIdentity pins the schema_version 2 block.
// The cloud writes none of these fields today -- outlet_fiscal_profile is
// edge-seeded -- so nothing downstream of the decode would notice if the
// emitter stopped filling them. This test is the only thing that would.
func TestLoadSeedFile_DecodesTheOutletIdentity(t *testing.T) {
	sf, err := loadSeedFile("testdata/demo-outlet-fixture.json")
	if err != nil {
		t.Fatalf("loadSeedFile: %v", err)
	}
	if sf.OutletSourceSHA256 == "" {
		t.Fatal("outlet_source_sha256 did not decode -- the catalogue cannot say which seed/outlet.toml produced it")
	}
	if sf.OutletIdentity.GSTIN == "" || sf.OutletIdentity.InvoicePrefix == "" {
		t.Fatalf("outlet_identity did not decode: %+v", sf.OutletIdentity)
	}
	if sf.OutletIdentity.AddressLine2 != nil {
		t.Fatalf("address_line2 is null in the fixture and must decode as nil, got %q", *sf.OutletIdentity.AddressLine2)
	}
}

// TestLoadSeedFile_RejectsStateCodeGstinMismatch falsifies the cross-field
// rule on the READER side. The emitter checks it too, but this reader runs
// against a COMMITTED file a person can edit by hand, and a wrong
// place-of-supply is not a defect any screen shows. Both values below are
// individually well-formed -- 29 is Karnataka's real GST state code -- so
// only the cross-field comparison can catch it.
func TestLoadSeedFile_RejectsStateCodeGstinMismatch(t *testing.T) {
	path := writeMutatedFixture(t, func(s string) string {
		return strings.Replace(s, `"state_code": "27"`, `"state_code": "29"`, 1)
	})
	_, err := loadSeedFile(path)
	if err == nil {
		t.Fatal("expected a state_code/gstin mismatch to be rejected")
	}
	if !strings.Contains(err.Error(), "place-of-supply") {
		t.Fatalf("expected the error to explain place-of-supply, got: %v", err)
	}
}

// TestLoadSeedFile_RejectsNameDisagreement falsifies the rule that the rows
// this seeder writes must agree with the identity they claim to come from. A
// tenant named for one restaurant and an invoice footer for another is
// internally consistent on every screen and wrong on the bill.
func TestLoadSeedFile_RejectsNameDisagreement(t *testing.T) {
	path := writeMutatedFixture(t, func(s string) string {
		return strings.Replace(s, `"outlet_name": "Pune Test Outlet"`, `"outlet_name": "Some Other Outlet"`, 1)
	})
	_, err := loadSeedFile(path)
	if err == nil {
		t.Fatal("expected a name disagreement to be rejected")
	}
	if !strings.Contains(err.Error(), "outlet.name") {
		t.Fatalf("expected the error to name the disagreeing field, got: %v", err)
	}
}

// TestLoadSeedFile_RejectsAnOlderSchemaVersion falsifies the exact-version
// pin. A reader that accepted schema_version 1 would accept a file with no
// outlet_identity block at all, and every field in it would read as an empty
// string -- the failure this whole change exists to prevent, arriving through
// the back door.
func TestLoadSeedFile_RejectsAnOlderSchemaVersion(t *testing.T) {
	path := writeMutatedFixture(t, func(s string) string {
		return strings.Replace(s, `"schema_version": 2,`, `"schema_version": 1,`, 1)
	})
	_, err := loadSeedFile(path)
	if err == nil {
		t.Fatal("expected schema_version 1 to be rejected")
	}
	if !strings.Contains(err.Error(), "re-emit") {
		t.Fatalf("expected the error to carry its own remedy, got: %v", err)
	}
}

// TestLoadSeedFile_RejectsUnknownField falsifies strict decoding: a field
// the JSON carries that this reader does not declare must fail the decode,
// never be silently discarded (the contracts 0.5.9 defect, one hop earlier
// -- see seedfile.go's loadSeedFile doc comment).
func TestLoadSeedFile_RejectsUnknownField(t *testing.T) {
	path := writeMutatedFixture(t, func(s string) string {
		return strings.Replace(s,
			`"schema_version": 2,`,
			`"schema_version": 2, "an_unrecognised_field": "should fail the decode",`, 1)
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
