package menu

import (
	"encoding/json"
	"testing"
)

// THE DEFECT THIS PINS, and it is a class rather than an instance.
//
// itemWire carried 7 fields while MenuItemSchema declared 10. tax_profile_id
// (contracts 0.4.2), hsn_sac (0.4.5) and schema_version were present in the
// database, in the domain Item and in the repository's SELECT, and absent from
// the struct that goes on the wire. So every HTTP reader of a menu item got a
// row with no tax profile and no HSN/SAC code, from the version each was added
// until 2026-09-08.
//
// WHY IT SURVIVED. Go's encoder omits a field that is not on the struct: no
// error, no warning, nothing to notice. The POS reads the menu from its own
// SQLite, not from this route, and the KDS does not read the menu at all — so
// until the M6 Phase B admin console there was no strict client anywhere in the
// system that could tell the difference. The first Zod parse found it
// immediately.
//
// This is contracts 0.5.9's lesson in a second place: THE ADDITIVE-CHANGE
// CONSUMER LIST REACHES THE WIRE TYPES, NOT JUST THE SCHEMAS. There it was
// source_stock_count_id, dropped in silence between the edge and Postgres;
// here it is two tax fields dropped between Postgres and the browser.
//
// The assertion is by KEY SET, not by value. A test that checked "hsn_sac
// round-trips when set" would pass on a struct that omitted the key whenever
// the value was nil — which is exactly how the 0.5.9 hole stayed green for four
// versions against a fixture whose provenance fields were legitimately null.
func TestItemWire_CarriesEveryFieldTheContractDeclares(t *testing.T) {
	// The full set MenuItemSchema declares. Kept as a literal rather than
	// derived from the Go struct: deriving it from the thing under test makes
	// the test agree with itself no matter what either side does.
	want := []string{
		"id", "outlet_id", "category_id", "name", "base_price_paise",
		"is_available", "tax_profile_id", "hsn_sac", "config_version",
		"schema_version",
	}

	// A NIL tax_profile_id and a NIL hsn_sac, deliberately. Null is the case
	// that hides a missing key, so it is the case this asserts against.
	raw, err := json.Marshal(itemToWire(Item{
		ID: "i", OutletID: "o", CategoryID: "c", Name: "Masala Chai",
		BasePricePaise: 4000, IsAvailable: true, ConfigVersion: 1,
	}))
	if err != nil {
		t.Fatalf("marshalling: %v", err)
	}

	var got map[string]json.RawMessage
	if err := json.Unmarshal(raw, &got); err != nil {
		t.Fatalf("unmarshalling: %v", err)
	}

	for _, key := range want {
		if _, present := got[key]; !present {
			t.Errorf("%s is absent from the serialised menu item; MenuItemSchema requires it and a strict client rejects the whole response", key)
		}
	}
	if len(got) != len(want) {
		t.Errorf("serialised %d keys, contract declares %d — an EXTRA field is drift too, in the direction no client will complain about", len(got), len(want))
	}

	// Null, not omitted. "Absent" and "explicitly unset" must stay
	// distinguishable: a null tax_profile_id means "use the outlet default",
	// and a null hsn_sac means "not yet classified", which is why an invoice
	// refuses to issue against it.
	if string(got["tax_profile_id"]) != "null" {
		t.Errorf("tax_profile_id serialised as %s, want null", got["tax_profile_id"])
	}
	if string(got["hsn_sac"]) != "null" {
		t.Errorf("hsn_sac serialised as %s, want null", got["hsn_sac"])
	}
	if string(got["schema_version"]) != "1" {
		t.Errorf("schema_version = %s, want the literal 1", got["schema_version"])
	}
}

func TestCategoryWire_CarriesEveryFieldTheContractDeclares(t *testing.T) {
	want := []string{"id", "outlet_id", "name", "sort_order", "config_version", "schema_version"}

	raw, err := json.Marshal(categoryToWire(Category{
		ID: "c", OutletID: "o", Name: "Beverages", SortOrder: 1, ConfigVersion: 1,
	}))
	if err != nil {
		t.Fatalf("marshalling: %v", err)
	}
	var got map[string]json.RawMessage
	if err := json.Unmarshal(raw, &got); err != nil {
		t.Fatalf("unmarshalling: %v", err)
	}
	for _, key := range want {
		if _, present := got[key]; !present {
			t.Errorf("%s is absent from the serialised category; MenuCategorySchema requires it", key)
		}
	}
	if len(got) != len(want) {
		t.Errorf("serialised %d keys, contract declares %d", len(got), len(want))
	}
}

// A populated item as well as a null one. The pair matters: the null case
// proves the KEY is present, this proves the VALUE reaches the wire. Neither
// alone is sufficient, which is the whole 0.5.9 finding.
func TestItemWire_PopulatedTaxFieldsReachTheWire(t *testing.T) {
	profile := "0191a000-0000-7000-8000-0000000000f1"
	code := "9963"

	raw, err := json.Marshal(itemToWire(Item{
		ID: "i", OutletID: "o", CategoryID: "c", Name: "Veg Thali",
		BasePricePaise: 22000, IsAvailable: true,
		TaxProfileID: &profile, HSNSAC: &code, ConfigVersion: 3,
	}))
	if err != nil {
		t.Fatalf("marshalling: %v", err)
	}
	var got struct {
		TaxProfileID *string `json:"tax_profile_id"`
		HSNSAC       *string `json:"hsn_sac"`
	}
	if err := json.Unmarshal(raw, &got); err != nil {
		t.Fatalf("unmarshalling: %v", err)
	}
	if got.TaxProfileID == nil || *got.TaxProfileID != profile {
		t.Errorf("tax_profile_id = %v, want %s", got.TaxProfileID, profile)
	}
	if got.HSNSAC == nil || *got.HSNSAC != code {
		t.Errorf("hsn_sac = %v, want %s — an invoice cannot issue without it", got.HSNSAC, code)
	}
}
