package kitchen_test

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/kitchen"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/id"
	contracts "github.com/holler/contracts"
)

// M6 A1b — a KOT replayed for an order the cloud does not have must be
// refused as a CLIENT-DATA fault the edge can act on.
//
// WHY THIS ROUTE, AND WHAT THE AUDIT GOT WRONG ABOUT IT.
// docs/m6-a1-sink-audit.md listed kitchen.InsertKot as unclassified on the
// strength of `fmt.Errorf` wrapping its INSERT, with `kot.order_id
// REFERENCES "order"(id)` as the foreign key that would fire. Reading the
// service path rather than the repository line shows that is NOT how this
// route fails: IngestKot calls repo.OrderOutlet FIRST, which returns
// httpx.ErrNotFound for a missing order, so the reply is 404 and the FK is
// effectively unreachable outside a race. **The audit row's mechanism was
// wrong; its conclusion — this route mishandles a missing parent — was
// right, and worse than recorded.**
//
// WHY 404 IS WORSE THAN THE 500 IT WAS ASSUMED TO BE. The edge's
// is_permanent_rejection deliberately treats 404 as TRANSIENT: a route that
// is not there is a deployment problem, every row would get the same answer,
// and charging it to one row would abandon good rows. So a KOT for an order
// the cloud will never accept is retried forever — and because 404 is
// transient, M6 A2's per-aggregate blocking does NOT catch it: the drain
// takes a global stop instead. One such KOT stops the whole outbox, every
// pump, permanently.
//
// THE FIX IS A DISTINCTION, NOT A STATUS SWAP. On an ingest route, 404 must
// keep meaning "no such route" — that is what makes it safe for the edge to
// treat as transient. A REFERENCED ROW that does not exist is a different
// fact about a different thing, and it is permanent. It becomes 422
// missing_reference, exactly as the FK path already does since 99875cc.
//
// This is not a contract change: openapi.yaml documents only 200 and 422 for
// POST /orders/{id}/kots (packages/contracts/openapi/openapi.yaml:139-147).
// The 404 this route returns today is undocumented, so moving to 422 brings
// it TOWARD the frozen contract rather than away from it.
//
// Watched failing first on the pre-fix binary: 404 "not_found".
func TestIngest_Kot_ForUnknownOrderIsPermanentNotTransient(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)

	svc := kitchen.NewService(kitchen.NewRepository(pool), auth.NewAuditor(nil))
	router := newKitchenIngestRouter(svc, fx)

	// An order id that is well-formed and absent. This is the replayed-KOT
	// case: the till made a ticket for an order whose own envelope the cloud
	// refused, so the order will never exist here.
	missingOrderID := id.New()
	kotID := id.New()
	now := time.Now().UTC()

	payload := map[string]any{
		"id":                   kotID,
		"order_id":             missingOrderID,
		"station":              "HOT",
		"sequence":             1,
		"status":               "NEW",
		"items":                []any{},
		"created_by_device_id": testDeviceID,
		"created_at":           now.Format(time.RFC3339Nano),
		"updated_at":           now.Format(time.RFC3339Nano),
	}
	env := kotEnvelope(kotID, fx.tenantID, fx.outletID, 1)
	resp := postKotEnvelope(t, router, "/orders/"+missingOrderID+"/kots", env, payload)

	var body struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	}
	raw := resp.Body.Bytes()
	if err := json.Unmarshal(raw, &body); err != nil {
		t.Fatalf("decoding error envelope %q: %v", string(raw), err)
	}

	if resp.Code == http.StatusNotFound {
		t.Errorf("status = 404 (%s): the edge classifies 404 as TRANSIENT (a route that is not "+
			"there), so this KOT is retried forever and — since A2 does not block on a transient "+
			"status — it takes a GLOBAL stop with it, wedging the whole outbox", body.Code)
	}
	if resp.Code != http.StatusUnprocessableEntity {
		t.Errorf("status = %d, want 422: an order the cloud has never accepted is a permanent "+
			"client-data fault, not a missing route and not a server fault", resp.Code)
	}
	if body.Code != "missing_reference" {
		t.Errorf("code = %q, want %q — the edge branches on this string", body.Code, "missing_reference")
	}
	for _, leak := range []string{"SQLSTATE", "23503", "INSERT", "SELECT", "fkey"} {
		if containsFold(raw, leak) {
			t.Errorf("error body leaks internal detail %q: %s", leak, string(raw))
		}
	}
}

// The other half of the distinction: a route that genuinely does not exist
// must STILL be 404, or the edge loses the signal that tells it to keep
// retrying through a bad deployment instead of abandoning rows.
func TestIngest_UnknownRouteIsStill404(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)

	svc := kitchen.NewService(kitchen.NewRepository(pool), auth.NewAuditor(nil))
	router := newKitchenIngestRouter(svc, fx)

	env := kotEnvelope(id.New(), fx.tenantID, fx.outletID, 1)
	resp := postKotEnvelope(t, router, "/orders/"+id.New()+"/kots-that-do-not-exist", env, map[string]any{})

	if resp.Code != http.StatusNotFound {
		t.Errorf("status = %d for an unmounted path, want 404: 404 must keep meaning "+
			"\"no such route\", which is what makes it safe for the edge to treat as transient",
			resp.Code)
	}
}

func newKitchenIngestRouter(svc *kitchen.Service, fx fixture) *chi.Mux {
	h := kitchen.NewHandler(svc)
	devicePrincipal := outlet.DevicePrincipal{
		DeviceID: testDeviceID,
		TenantID: fx.tenantID,
		OutletID: fx.outletID,
	}
	principal := auth.AuthenticatedPrincipal{
		UserID:      "principal-user",
		TenantID:    fx.tenantID,
		OutletID:    fx.outletID,
		Permissions: []auth.Permission{auth.PermissionOrderModify, auth.PermissionOutletManage},
	}

	r := chi.NewRouter()
	r.Use(func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
			ctx := auth.WithPrincipal(req.Context(), principal)
			ctx = outlet.WithDevicePrincipal(ctx, devicePrincipal)
			next.ServeHTTP(w, req.WithContext(ctx))
		})
	})
	h.MountIngest(r)
	return r
}

func containsFold(haystack []byte, needle string) bool {
	return len(needle) > 0 &&
		len(haystack) >= len(needle) &&
		bytesContainsFold(haystack, []byte(needle))
}

func bytesContainsFold(h, n []byte) bool {
	lower := func(b byte) byte {
		if b >= 'A' && b <= 'Z' {
			return b + 32
		}
		return b
	}
	for i := 0; i+len(n) <= len(h); i++ {
		match := true
		for j := range n {
			if lower(h[i+j]) != lower(n[j]) {
				match = false
				break
			}
		}
		if match {
			return true
		}
	}
	return false
}

var _ = contracts.AggregateTypeKot

func postKotEnvelope(t *testing.T, router *chi.Mux, path string, env contracts.SyncEnvelope, payload any) *httptest.ResponseRecorder {
	t.Helper()

	raw, err := json.Marshal(payload)
	if err != nil {
		t.Fatalf("marshalling payload: %v", err)
	}
	body, err := json.Marshal(map[string]any{
		"record_id":      env.RecordID,
		"tenant_id":      env.TenantID,
		"outlet_id":      env.OutletID,
		"device_id":      env.DeviceID,
		"aggregate_type": env.AggregateType,
		"direction":      env.Direction,
		"created_at":     env.CreatedAt.Format(time.RFC3339Nano),
		"updated_at":     env.UpdatedAt.Format(time.RFC3339Nano),
		"version":        env.Version,
		"sync_status":    env.SyncStatus,
		"payload":        json.RawMessage(raw),
	})
	if err != nil {
		t.Fatalf("marshalling envelope: %v", err)
	}

	req := httptest.NewRequest(http.MethodPost, path, bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	router.ServeHTTP(rec, req)
	return rec
}
