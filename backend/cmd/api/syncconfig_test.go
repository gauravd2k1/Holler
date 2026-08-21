package main

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	contracts "github.com/holler/contracts"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/compliance"
	"github.com/holler/backend/internal/inventory"
	"github.com/holler/backend/internal/kitchen"
	"github.com/holler/backend/internal/menu"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/tables"
)

const (
	scTenantID = "tenant-1"
	scOutletID = "outlet-1"
)

type fakeOutletProvider struct {
	outlet outlet.Outlet
}

func (f fakeOutletProvider) GetOutlet(ctx context.Context, principal outlet.Principal, outletID string) (outlet.Outlet, error) {
	if principal.TenantID != scTenantID || outletID != scOutletID {
		return outlet.Outlet{}, httpx.ErrNotFound
	}
	return f.outlet, nil
}

type fakeMenuProvider struct {
	categories []menu.Category
	items      []menu.Item
	variants   []menu.Variant
	modifiers  []menu.Modifier
}

func (f fakeMenuProvider) ListCategories(ctx context.Context, outletID string) ([]menu.Category, error) {
	return f.categories, nil
}
func (f fakeMenuProvider) ListItems(ctx context.Context, outletID string) ([]menu.Item, error) {
	return f.items, nil
}
func (f fakeMenuProvider) ListVariantsSince(ctx context.Context, outletID string, sinceVersion int) ([]menu.Variant, error) {
	var out []menu.Variant
	for _, v := range f.variants {
		if v.ConfigVersion > sinceVersion {
			out = append(out, v)
		}
	}
	return out, nil
}
func (f fakeMenuProvider) ListModifiersSince(ctx context.Context, outletID string, sinceVersion int) ([]menu.Modifier, error) {
	var out []menu.Modifier
	for _, m := range f.modifiers {
		if m.ConfigVersion > sinceVersion {
			out = append(out, m)
		}
	}
	return out, nil
}

type fakeTablesProvider struct {
	tables []tables.RestaurantTable
}

func (f fakeTablesProvider) ListTables(ctx context.Context, outletID string) ([]tables.RestaurantTable, error) {
	return f.tables, nil
}

type fakeKitchenProvider struct {
	bundle kitchen.ConfigBundle
}

func (f fakeKitchenProvider) SyncConfigBundle(ctx context.Context, tenantID, outletID string, sinceVersion int) (kitchen.ConfigBundle, error) {
	if tenantID != scTenantID || outletID != scOutletID {
		return kitchen.ConfigBundle{}, httpx.ErrNotFound
	}
	return f.bundle, nil
}

type fakeComplianceProvider struct {
	bundle compliance.ConfigBundle
}

func (f fakeComplianceProvider) SyncConfigBundle(ctx context.Context, tenantID, outletID string, sinceVersion int) (compliance.ConfigBundle, error) {
	if tenantID != scTenantID || outletID != scOutletID {
		return compliance.ConfigBundle{}, httpx.ErrNotFound
	}
	return f.bundle, nil
}

type fakeInventoryProvider struct {
	bundle inventory.ConfigBundle
}

func (f fakeInventoryProvider) SyncConfigBundle(ctx context.Context, tenantID, outletID string, sinceVersion int) (inventory.ConfigBundle, error) {
	if tenantID != scTenantID || outletID != scOutletID {
		return inventory.ConfigBundle{}, httpx.ErrNotFound
	}
	return f.bundle, nil
}

// fakeUsersProvider stands in for auth.Service.ListEdgeUserCache. It applies
// the same tenant/since_version filtering a real implementation must, so
// tests can assert the handler passes both through untouched.
type fakeUsersProvider struct {
	entries []contracts.EdgeUserCacheEntry
}

func (f fakeUsersProvider) ListEdgeUserCache(ctx context.Context, tenantID, outletID string, sinceVersion int) ([]contracts.EdgeUserCacheEntry, error) {
	if tenantID != scTenantID || outletID != scOutletID {
		return nil, httpx.ErrNotFound
	}
	var out []contracts.EdgeUserCacheEntry
	for _, e := range f.entries {
		if e.ConfigVersion > sinceVersion {
			out = append(out, e)
		}
	}
	return out, nil
}

type fakeDeviceCredentialProvider struct {
	entries []contracts.EdgeDeviceCredential
}

func (f fakeDeviceCredentialProvider) ListEdgeDeviceCredentials(ctx context.Context, tenantID, outletID string, sinceVersion int) ([]contracts.EdgeDeviceCredential, error) {
	if tenantID != scTenantID || outletID != scOutletID {
		return nil, httpx.ErrNotFound
	}
	var out []contracts.EdgeDeviceCredential
	for _, e := range f.entries {
		if e.ConfigVersion > sinceVersion {
			out = append(out, e)
		}
	}
	return out, nil
}

func pinHashPtr(s string) *string { return &s }

func newTestSyncConfigHandler() *syncConfigHandler {
	return newSyncConfigHandler(
		fakeOutletProvider{outlet: outlet.Outlet{ID: scOutletID, ConfigVersion: 7}},
		fakeMenuProvider{
			categories: []menu.Category{
				{ID: "cat-old", OutletID: scOutletID, Name: "Old", ConfigVersion: 1},
				{ID: "cat-new", OutletID: scOutletID, Name: "New", ConfigVersion: 5},
			},
			items: []menu.Item{
				{ID: "item-old", OutletID: scOutletID, ConfigVersion: 1},
				{ID: "item-new", OutletID: scOutletID, ConfigVersion: 6, BasePricePaise: 12550, TaxProfileID: pinHashPtr("tax-profile-1"), HSNSAC: pinHashPtr("9963")},
			},
			variants: []menu.Variant{
				{ID: "variant-old", MenuItemID: "item-old", Name: "Old Variant", ConfigVersion: 1},
				{ID: "variant-new", MenuItemID: "item-new", Name: "Regular", IsDefault: true, ConfigVersion: 6},
			},
			modifiers: []menu.Modifier{
				{ID: "modifier-old", MenuItemID: "item-old", GroupName: "Old Group", OptionName: "Old Option", ConfigVersion: 1},
				{ID: "modifier-new", MenuItemID: "item-new", GroupName: "Extras", OptionName: "Extra Paneer", ConfigVersion: 6},
			},
		},
		fakeTablesProvider{
			tables: []tables.RestaurantTable{
				{ID: "table-old", OutletID: scOutletID, ConfigVersion: 1, SchemaVersion: 1},
				{ID: "table-new", OutletID: scOutletID, ConfigVersion: 4, SchemaVersion: 1},
			},
		},
		fakeKitchenProvider{
			bundle: kitchen.ConfigBundle{
				Stations:        []contracts.Station{{ID: "station-1", OutletID: scOutletID}},
				ItemStations:    []contracts.MenuItemStation{{MenuItemID: "item-new", StationID: "station-1"}},
				Printers:        []contracts.Printer{{ID: "printer-1", OutletID: scOutletID}},
				StationPrinters: []contracts.StationPrinter{{StationID: "station-1", PrinterID: "printer-1"}},
			},
		},
		fakeComplianceProvider{
			bundle: compliance.ConfigBundle{
				ComplianceVersions:  []contracts.ComplianceVersion{},
				TaxProfiles:         []contracts.TaxProfile{},
				TaxRules:            []contracts.TaxRule{},
				InvoiceSeries:       []contracts.InvoiceSeries{},
				DiscountDefinitions: []contracts.DiscountDefinition{},
			},
		},
		fakeInventoryProvider{
			bundle: inventory.ConfigBundle{
				InventoryItems:           []contracts.InventoryItem{},
				ItemUnitConversions:      []contracts.ItemUnitConversion{},
				Recipes:                  []contracts.Recipe{},
				RecipeIngredients:        []contracts.RecipeIngredient{},
				ModifierIngredientDeltas: []contracts.ModifierIngredientDelta{},
			},
		},
		fakeUsersProvider{
			entries: []contracts.EdgeUserCacheEntry{
				{
					ID: "user-old", TenantID: scTenantID, OutletID: scOutletID,
					Email: "old@example.com", FullName: "Old User",
					PasswordHash: "argon2id-hash-old", PinHash: nil,
					IsActive: true, Permissions: []contracts.Permission{auth.PermissionOrderCreate},
					ConfigVersion: 1, SchemaVersion: 1,
				},
				{
					ID: "user-new", TenantID: scTenantID, OutletID: scOutletID,
					Email: "new@example.com", FullName: "New User",
					PasswordHash: "argon2id-hash-new", PinHash: pinHashPtr("argon2id-pin-new"),
					IsActive: true, Permissions: []contracts.Permission{auth.PermissionOrderCreate, auth.PermissionUserManage},
					ConfigVersion: 6, SchemaVersion: 1,
				},
			},
		},
		fakeDeviceCredentialProvider{
			entries: []contracts.EdgeDeviceCredential{
				{
					CredentialID: "cred-old", DeviceID: "device-old", TenantID: scTenantID, OutletID: scOutletID,
					CredentialHash: "$argon2id$v=19$fake-old", DeviceKind: "KDS",
					ConfigVersion: 1, SchemaVersion: 1,
				},
				{
					CredentialID: "cred-new", DeviceID: "device-new", TenantID: scTenantID, OutletID: scOutletID,
					CredentialHash: "$argon2id$v=19$fake-new", DeviceKind: "POS",
					ConfigVersion: 6, SchemaVersion: 1,
				},
			},
		},
	)
}

// doSyncConfigRequest injects a DevicePrincipal directly into the request
// context, exactly like outlet.DeviceAuthenticate would after verifying a
// real device token (ADR-017 §2) — this handler no longer reads
// auth.AuthenticatedPrincipal at all.
func doSyncConfigRequest(t *testing.T, h http.Handler, principal *outlet.DevicePrincipal, query string) *httptest.ResponseRecorder {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, "/sync/config?"+query, nil)
	if principal != nil {
		req = req.WithContext(outlet.WithDevicePrincipal(req.Context(), *principal))
	}
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	return rec
}

func testDevicePrincipal() outlet.DevicePrincipal {
	return outlet.DevicePrincipal{
		DeviceID: "device-1",
		TenantID: scTenantID,
		OutletID: scOutletID,
	}
}

// TestSyncConfig_AllNineFieldsPresent verifies the response has exactly the
// nine fields packages/contracts/openapi/openapi.yaml requires, and that the
// users array is populated with credential material intact.
func TestSyncConfig_AllNineFieldsPresent(t *testing.T) {
	h := newTestSyncConfigHandler()
	p := testDevicePrincipal()
	rec := doSyncConfigRequest(t, h, &p, "outlet_id="+scOutletID+"&since_version=0")

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d: %s", rec.Code, rec.Body.String())
	}

	var body map[string]json.RawMessage
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decoding response: %v", err)
	}

	required := []string{
		"config_version", "users", "tables", "categories", "items",
		"stations", "item_stations", "printers", "station_printers",
	}
	for _, field := range required {
		if _, ok := body[field]; !ok {
			t.Errorf("missing required field %q", field)
		}
	}

	var resp syncConfigResponse
	if err := json.Unmarshal(rec.Body.Bytes(), &resp); err != nil {
		t.Fatalf("decoding into syncConfigResponse: %v", err)
	}
	if resp.ConfigVersion != 7 {
		t.Errorf("expected config_version 7, got %d", resp.ConfigVersion)
	}
	if len(resp.Users) != 2 {
		t.Fatalf("expected 2 users, got %d", len(resp.Users))
	}
	if !contains(rec.Body.String(), "argon2id-hash-old") || !contains(rec.Body.String(), "argon2id-hash-new") {
		t.Errorf("expected password_hash values on the wire: %s", rec.Body.String())
	}
}

// TestSyncConfig_MenuItemVariantsAndModifiersDeliveredSinceVersion is the M4
// T4 delivery-fix follow-up's own regression test: menu_item_variant and
// menu_item_modifier never reached this response before, and
// recipe.menu_item_variant_id is NOT NULL, so a cloud-synced outlet missing
// its own variants gapped NO_VARIANT on every sale. Also covers the
// itemConfigWire tax_profile_id/hsn_sac gap closed in the same pass.
func TestSyncConfig_MenuItemVariantsAndModifiersDeliveredSinceVersion(t *testing.T) {
	h := newTestSyncConfigHandler()
	p := testDevicePrincipal()

	rec := doSyncConfigRequest(t, h, &p, "outlet_id="+scOutletID+"&since_version=0")
	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d: %s", rec.Code, rec.Body.String())
	}
	var resp syncConfigResponse
	if err := json.Unmarshal(rec.Body.Bytes(), &resp); err != nil {
		t.Fatalf("decoding into syncConfigResponse: %v", err)
	}
	if len(resp.MenuItemVariants) != 2 {
		t.Fatalf("expected 2 variants at since_version=0, got %d: %+v", len(resp.MenuItemVariants), resp.MenuItemVariants)
	}
	if len(resp.MenuItemModifiers) != 2 {
		t.Fatalf("expected 2 modifiers at since_version=0, got %d: %+v", len(resp.MenuItemModifiers), resp.MenuItemModifiers)
	}
	var foundDefault bool
	for _, v := range resp.MenuItemVariants {
		if v.ID == "variant-new" {
			if !v.IsDefault {
				t.Error("expected variant-new.is_default to survive to the wire")
			}
			foundDefault = true
		}
	}
	if !foundDefault {
		t.Fatal("variant-new missing from the response entirely")
	}

	for _, it := range resp.Items {
		if it.ID == "item-new" {
			if it.TaxProfileID == nil || *it.TaxProfileID != "tax-profile-1" {
				t.Errorf("expected item-new.tax_profile_id to survive to the wire, got %v", it.TaxProfileID)
			}
			if it.HSNSAC == nil || *it.HSNSAC != "9963" {
				t.Errorf("expected item-new.hsn_sac to survive to the wire, got %v", it.HSNSAC)
			}
		}
	}

	// since_version filtering: only the "new" (config_version 6) rows
	// should survive a since_version=5 request.
	rec2 := doSyncConfigRequest(t, h, &p, "outlet_id="+scOutletID+"&since_version=5")
	var resp2 syncConfigResponse
	if err := json.Unmarshal(rec2.Body.Bytes(), &resp2); err != nil {
		t.Fatalf("decoding into syncConfigResponse: %v", err)
	}
	if len(resp2.MenuItemVariants) != 1 || resp2.MenuItemVariants[0].ID != "variant-new" {
		t.Fatalf("expected only variant-new at since_version=5, got %+v", resp2.MenuItemVariants)
	}
	if len(resp2.MenuItemModifiers) != 1 || resp2.MenuItemModifiers[0].ID != "modifier-new" {
		t.Fatalf("expected only modifier-new at since_version=5, got %+v", resp2.MenuItemModifiers)
	}
}

// TestSyncConfig_DeviceCredentialsPresentWithHashAndSinceVersionFiltering
// (T13): device_credentials is required in the same response and must
// carry credential_hash intact, filtered by since_version exactly like
// tables/categories/items above.
func TestSyncConfig_DeviceCredentialsPresentWithHashAndSinceVersionFiltering(t *testing.T) {
	h := newTestSyncConfigHandler()
	p := testDevicePrincipal()
	rec := doSyncConfigRequest(t, h, &p, "outlet_id="+scOutletID+"&since_version=0")
	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d: %s", rec.Code, rec.Body.String())
	}

	var body map[string]json.RawMessage
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decoding response: %v", err)
	}
	if _, ok := body["device_credentials"]; !ok {
		t.Fatal("missing required field \"device_credentials\"")
	}

	var resp syncConfigResponse
	if err := json.Unmarshal(rec.Body.Bytes(), &resp); err != nil {
		t.Fatalf("decoding into syncConfigResponse: %v", err)
	}
	if len(resp.DeviceCredentials) != 2 {
		t.Fatalf("expected 2 device credentials at since_version=0, got %d", len(resp.DeviceCredentials))
	}
	if !contains(rec.Body.String(), "$argon2id$v=19$fake-old") || !contains(rec.Body.String(), "$argon2id$v=19$fake-new") {
		t.Errorf("expected credential_hash values on the wire: %s", rec.Body.String())
	}

	filteredRec := doSyncConfigRequest(t, h, &p, "outlet_id="+scOutletID+"&since_version=1")
	var filtered syncConfigResponse
	if err := json.Unmarshal(filteredRec.Body.Bytes(), &filtered); err != nil {
		t.Fatalf("decoding filtered response: %v", err)
	}
	if len(filtered.DeviceCredentials) != 1 || filtered.DeviceCredentials[0].CredentialID != "cred-new" {
		t.Fatalf("expected since_version=1 to exclude cred-old, got %+v", filtered.DeviceCredentials)
	}
}

// TestSyncConfig_UsersPinHashRoundTripsBothStates asserts pin_hash is
// present-and-null for a user with no PIN, and present-and-set for a user
// with one — an omitted key round-trips to a different object, so both
// states are checked against the raw JSON, not just the decoded struct.
func TestSyncConfig_UsersPinHashRoundTripsBothStates(t *testing.T) {
	h := newTestSyncConfigHandler()
	p := testDevicePrincipal()
	rec := doSyncConfigRequest(t, h, &p, "outlet_id="+scOutletID+"&since_version=0")

	var raw struct {
		Users []map[string]json.RawMessage `json:"users"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &raw); err != nil {
		t.Fatalf("decoding response: %v", err)
	}
	if len(raw.Users) != 2 {
		t.Fatalf("expected 2 users, got %d", len(raw.Users))
	}

	byEmail := map[string]map[string]json.RawMessage{}
	for _, u := range raw.Users {
		var email string
		if err := json.Unmarshal(u["email"], &email); err != nil {
			t.Fatalf("decoding email: %v", err)
		}
		byEmail[email] = u
	}

	oldRaw, ok := byEmail["old@example.com"]["pin_hash"]
	if !ok {
		t.Fatal("pin_hash key must be present, not omitted, when nil")
	}
	if string(oldRaw) != "null" {
		t.Errorf("expected pin_hash null for old@example.com, got %s", string(oldRaw))
	}

	newRaw, ok := byEmail["new@example.com"]["pin_hash"]
	if !ok {
		t.Fatal("pin_hash key must be present when set")
	}
	var newPinHash string
	if err := json.Unmarshal(newRaw, &newPinHash); err != nil {
		t.Fatalf("decoding pin_hash: %v", err)
	}
	if newPinHash != "argon2id-pin-new" {
		t.Errorf("expected pin_hash argon2id-pin-new, got %q", newPinHash)
	}
}

// TestSyncConfig_UsersPermissionsFlattened asserts each user's permissions
// arrive as a resolved, flat claim list rather than role references — the
// edge has no role table to resolve against.
func TestSyncConfig_UsersPermissionsFlattened(t *testing.T) {
	h := newTestSyncConfigHandler()
	p := testDevicePrincipal()
	rec := doSyncConfigRequest(t, h, &p, "outlet_id="+scOutletID+"&since_version=0")

	var resp syncConfigResponse
	if err := json.Unmarshal(rec.Body.Bytes(), &resp); err != nil {
		t.Fatalf("decoding response: %v", err)
	}

	var newUser *contracts.EdgeUserCacheEntry
	for i := range resp.Users {
		if resp.Users[i].Email == "new@example.com" {
			newUser = &resp.Users[i]
		}
	}
	if newUser == nil {
		t.Fatal("expected new@example.com in users")
	}
	if len(newUser.Permissions) != 2 {
		t.Fatalf("expected 2 flattened permissions, got %v", newUser.Permissions)
	}
}

// TestSyncConfig_SinceVersionFiltering verifies only rows newer than
// since_version are returned for the fields this handler filters itself
// (tables/categories/items) — stations/printers/item_stations/
// station_printers filtering is kitchen.Service's own responsibility and is
// covered by backend/internal/kitchen's tests, not duplicated here.
func TestSyncConfig_SinceVersionFiltering(t *testing.T) {
	h := newTestSyncConfigHandler()
	p := testDevicePrincipal()
	rec := doSyncConfigRequest(t, h, &p, "outlet_id="+scOutletID+"&since_version=3")

	var resp syncConfigResponse
	if err := json.Unmarshal(rec.Body.Bytes(), &resp); err != nil {
		t.Fatalf("decoding response: %v", err)
	}

	if len(resp.Tables) != 1 || resp.Tables[0].ID != "table-new" {
		t.Errorf("expected only table-new (config_version 4 > 3), got %+v", resp.Tables)
	}
	if len(resp.Categories) != 1 || resp.Categories[0].ID != "cat-new" {
		t.Errorf("expected only cat-new (config_version 5 > 3), got %+v", resp.Categories)
	}
	if len(resp.Items) != 1 || resp.Items[0].ID != "item-new" {
		t.Errorf("expected only item-new (config_version 6 > 3), got %+v", resp.Items)
	}
	if len(resp.Users) != 1 || resp.Users[0].ID != "user-new" {
		t.Errorf("expected only user-new (config_version 6 > 3), got %+v", resp.Users)
	}
}

func TestSyncConfig_RequiresAuthentication(t *testing.T) {
	h := newTestSyncConfigHandler()
	rec := doSyncConfigRequest(t, h, nil, "outlet_id="+scOutletID+"&since_version=0")
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401 with no principal, got %d", rec.Code)
	}
}

func TestSyncConfig_CrossTenantOutletIsNotFound(t *testing.T) {
	h := newTestSyncConfigHandler()
	p := testDevicePrincipal()
	p.TenantID = "other-tenant"
	rec := doSyncConfigRequest(t, h, &p, "outlet_id="+scOutletID+"&since_version=0")
	if rec.Code != http.StatusNotFound {
		t.Fatalf("expected 404 for cross-tenant outlet_id, got %d: %s", rec.Code, rec.Body.String())
	}
}

// TestSyncConfig_DeviceOutletMismatchIsNotFound is the direct ADR-017 hole-1
// falsification: a device enrolled at scOutletID cannot pull another
// outlet's config just by putting a different outlet_id in the query
// string. Authority comes from the verified credential, not the request.
func TestSyncConfig_DeviceOutletMismatchIsNotFound(t *testing.T) {
	h := newTestSyncConfigHandler()
	p := testDevicePrincipal()
	rec := doSyncConfigRequest(t, h, &p, "outlet_id=some-other-outlet&since_version=0")
	if rec.Code != http.StatusNotFound {
		t.Fatalf("expected 404 when outlet_id does not match the device's own outlet, got %d: %s", rec.Code, rec.Body.String())
	}
}

func TestSyncConfig_MissingQueryParamsAreRejected(t *testing.T) {
	h := newTestSyncConfigHandler()
	p := testDevicePrincipal()

	if rec := doSyncConfigRequest(t, h, &p, "since_version=0"); rec.Code != http.StatusBadRequest {
		t.Errorf("expected 400 with missing outlet_id, got %d", rec.Code)
	}
	if rec := doSyncConfigRequest(t, h, &p, "outlet_id="+scOutletID); rec.Code != http.StatusBadRequest {
		t.Errorf("expected 400 with missing since_version, got %d", rec.Code)
	}
	if rec := doSyncConfigRequest(t, h, &p, "outlet_id="+scOutletID+"&since_version=-1"); rec.Code != http.StatusBadRequest {
		t.Errorf("expected 400 with negative since_version, got %d", rec.Code)
	}
}

func contains(haystack, needle string) bool {
	return len(haystack) >= len(needle) && (func() bool {
		for i := 0; i+len(needle) <= len(haystack); i++ {
			if haystack[i:i+len(needle)] == needle {
				return true
			}
		}
		return false
	})()
}
