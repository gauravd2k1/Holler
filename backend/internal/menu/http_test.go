package menu

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/holler/backend/internal/auth"
	contracts "github.com/holler/contracts"
)

// fakeStationRouter is a minimal StationRouter double so this route's
// wiring (permission, tenant extraction, path param) can be tested without
// pulling in backend/internal/kitchen.
type fakeStationRouter struct {
	gotTenantID string
	gotItemID   string
	gotStations []string
	err         error
}

func (f *fakeStationRouter) ReplaceItemStations(ctx context.Context, tenantID, itemID string, stationIDs []string) ([]contracts.MenuItemStation, error) {
	f.gotTenantID = tenantID
	f.gotItemID = itemID
	f.gotStations = stationIDs
	if f.err != nil {
		return nil, f.err
	}
	out := make([]contracts.MenuItemStation, len(stationIDs))
	for i, sid := range stationIDs {
		out[i] = contracts.MenuItemStation{MenuItemID: itemID, StationID: sid, ConfigVersion: 1, SchemaVersion: 1}
	}
	return out, nil
}

func TestReplaceItemStationsHTTP_HappyPath(t *testing.T) {
	stations := &fakeStationRouter{}
	h := NewHandlers(nil, stations)

	principal := auth.AuthenticatedPrincipal{
		UserID:      "principal-user",
		TenantID:    "11111111-1111-7111-8111-111111111111",
		Permissions: []auth.Permission{auth.PermissionMenuManage},
	}
	r := chi.NewRouter()
	r.Use(func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
			next.ServeHTTP(w, req.WithContext(auth.WithPrincipal(req.Context(), principal)))
		})
	})
	h.Mount(r)

	itemID := "22222222-2222-7222-8222-222222222222"
	body, _ := json.Marshal(map[string]interface{}{"station_ids": []string{"33333333-3333-7333-8333-333333333333"}})
	req := httptest.NewRequest(http.MethodPut, "/menu/items/"+itemID+"/stations", bytes.NewReader(body))
	rec := httptest.NewRecorder()
	r.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d: %s", rec.Code, rec.Body.String())
	}
	if stations.gotItemID != itemID {
		t.Fatalf("expected itemID %s, got %s", itemID, stations.gotItemID)
	}
	if stations.gotTenantID != principal.TenantID {
		t.Fatalf("expected tenantID %s, got %s", principal.TenantID, stations.gotTenantID)
	}
}

func TestReplaceItemStationsHTTP_MissingPermissionIsForbidden(t *testing.T) {
	stations := &fakeStationRouter{}
	h := NewHandlers(nil, stations)

	principal := auth.AuthenticatedPrincipal{UserID: "u", TenantID: "t"}
	r := chi.NewRouter()
	r.Use(func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
			next.ServeHTTP(w, req.WithContext(auth.WithPrincipal(req.Context(), principal)))
		})
	})
	h.Mount(r)

	body, _ := json.Marshal(map[string]interface{}{"station_ids": []string{}})
	req := httptest.NewRequest(http.MethodPut, "/menu/items/x/stations", bytes.NewReader(body))
	rec := httptest.NewRecorder()
	r.ServeHTTP(rec, req)

	if rec.Code != http.StatusForbidden {
		t.Fatalf("expected 403, got %d: %s", rec.Code, rec.Body.String())
	}
}
