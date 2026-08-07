package outlet

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestListOutletsHTTP_UnauthorizedWithoutPrincipal(t *testing.T) {
	h := NewHandler(NewService(newFakeRepo()))

	req := httptest.NewRequest(http.MethodGet, "/outlets", nil)
	rec := httptest.NewRecorder()

	h.listOutlets(rec, req)

	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", rec.Code)
	}
}

func TestListOutletsHTTP_ReturnsOnlyCallerTenantOutlets(t *testing.T) {
	repo := newFakeRepo()
	repo.brandTenant["brand-a"] = "tenant-a"
	repo.brandTenant["brand-b"] = "tenant-b"
	svc := NewService(repo)
	ctx := context.Background()

	outletA, err := svc.CreateOutlet(ctx, Principal{TenantID: "tenant-a"}, "brand-a", "Outlet A", "")
	if err != nil {
		t.Fatalf("CreateOutlet A: %v", err)
	}
	if _, err := svc.CreateOutlet(ctx, Principal{TenantID: "tenant-b"}, "brand-b", "Outlet B", ""); err != nil {
		t.Fatalf("CreateOutlet B: %v", err)
	}

	h := NewHandler(svc)
	req := httptest.NewRequest(http.MethodGet, "/outlets", nil)
	req = req.WithContext(WithPrincipal(req.Context(), Principal{TenantID: "tenant-a"}))
	rec := httptest.NewRecorder()

	h.listOutlets(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d: %s", rec.Code, rec.Body.String())
	}

	var got []outletResponse
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil {
		t.Fatalf("decoding response: %v", err)
	}
	if len(got) != 1 || got[0].ID != outletA.ID {
		t.Fatalf("expected only tenant A's outlet in response, got %+v", got)
	}
}
