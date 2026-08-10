package main

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	contracts "github.com/holler/contracts"

	"github.com/holler/backend/internal/auth"
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
}

func (f fakeMenuProvider) ListCategories(ctx context.Context, outletID string) ([]menu.Category, error) {
	return f.categories, nil
}
func (f fakeMenuProvider) ListItems(ctx context.Context, outletID string) ([]menu.Item, error) {
	return f.items, nil
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
				{ID: "item-new", OutletID: scOutletID, ConfigVersion: 6, BasePricePaise: 12550},
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
	)
}

func doSyncConfigRequest(t *testing.T, h http.Handler, principal *auth.AuthenticatedPrincipal, query string) *httptest.ResponseRecorder {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, "/sync/config?"+query, nil)
	if principal != nil {
		req = req.WithContext(auth.WithPrincipal(req.Context(), *principal))
	}
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	return rec
}

func testPrincipal() auth.AuthenticatedPrincipal {
	return auth.AuthenticatedPrincipal{
		UserID:      "user-1",
		TenantID:    scTenantID,
		OutletID:    scOutletID,
		Permissions: []auth.Permission{auth.PermissionUserManage},
	}
}

// TestSyncConfig_AllNineFieldsPresent verifies the response has exactly the
// nine fields packages/contracts/openapi/openapi.yaml requires, and that
// credential material never appears (users is always [] pending the gap
// documented on edgeUserCacheEntryWire).
func TestSyncConfig_AllNineFieldsPresent(t *testing.T) {
	h := newTestSyncConfigHandler()
	p := testPrincipal()
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
	if len(resp.Users) != 0 {
		t.Errorf("expected users to be empty (credential-material gap), got %d entries", len(resp.Users))
	}

	if contains(rec.Body.String(), "password_hash") {
		t.Errorf("response must never mention password_hash while users is empty: %s", rec.Body.String())
	}
}

// TestSyncConfig_SinceVersionFiltering verifies only rows newer than
// since_version are returned for the fields this handler filters itself
// (tables/categories/items) — stations/printers/item_stations/
// station_printers filtering is kitchen.Service's own responsibility and is
// covered by backend/internal/kitchen's tests, not duplicated here.
func TestSyncConfig_SinceVersionFiltering(t *testing.T) {
	h := newTestSyncConfigHandler()
	p := testPrincipal()
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
	p := testPrincipal()
	p.TenantID = "other-tenant"
	rec := doSyncConfigRequest(t, h, &p, "outlet_id="+scOutletID+"&since_version=0")
	if rec.Code != http.StatusNotFound {
		t.Fatalf("expected 404 for cross-tenant outlet_id, got %d: %s", rec.Code, rec.Body.String())
	}
}

func TestSyncConfig_MissingQueryParamsAreRejected(t *testing.T) {
	h := newTestSyncConfigHandler()
	p := testPrincipal()

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
