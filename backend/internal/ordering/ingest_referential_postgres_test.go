package ordering_test

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/ordering"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/id"
	"github.com/holler/backend/internal/platform/postgres"
	contracts "github.com/holler/contracts"
)

// M6 A1 / M6 C7 — a client-data failure must be reported as 4xx with a
// reason the edge can record.
//
// THE DEFECT THIS PINS. `order_item.menu_item_id` is a foreign key. The
// cloud menu seed holds 2 rows for the seeded outlet and the edge seeds 43,
// so a replayed order item routinely references a menu_item the cloud has
// never had. Postgres raises SQLSTATE 23503 (foreign_key_violation), the
// repository wraps it with fmt.Errorf, nothing matches it in httpx.Error's
// switch, and the client is told 500 "internal_error" — a permanent
// referential fault reported as a transient server fault. The edge then
// classifies 5xx as transient (edge/sync/src/ranged.rs), retries forever,
// and because the general outbox drains in order, ONE such row strands every
// row behind it. 120 rows were observed pending this way on 2026-09-02.
//
// THIS TEST ASSERTS THE POST-FIX CONTRACT, NOT CURRENT BEHAVIOUR. It is
// written to be watched RED on the pre-fix binary (it observes 500
// "internal_error") and to go green only when 23503 is mapped at the httpx
// boundary. Asserting today's 500 would produce a test that passes now and
// can never fail — the shape §66 exists to forbid.
//
// WHY 422 AND NOT 409. 409 says "state conflict, a retry may resolve it". A
// menu_item the cloud has never held is not a conflict and no retry will
// resolve it, so 422 is the honest code. The edge's is_permanent_rejection
// treats every 400..=499 alike, so the wire consequence is identical either
// way — which is exactly why the code has to be chosen for what it MEANS.
//
// WHAT THIS TEST DOES NOT PROVE. M6 C7 also requires the reason to be
// STORED and the row SURFACED to a human. Both are A3 work: the general
// outbox has no per-entry budget at all today and nothing surfaces a
// permanently-rejected row. M6 C7 stays OPEN when this test is green.
func TestIngest_AppendItem_MissingMenuItemIsClientErrorNotServerError(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)
	ctx := context.Background()

	svc := ordering.NewService(ordering.NewPostgresRepository(pool))
	router := newIngestRouter(svc, fx)

	// An order that exists, so the only thing wrong with the item append is
	// the reference itself.
	orderID := id.New()
	createOrder(ctx, t, router, fx, orderID)

	// A menu_item id that is well-formed and absent: no row has ever carried
	// it, in this tenant or any other. This is the replayed-order case, not
	// a malformed payload.
	missingMenuItemID := id.New()

	item := contracts.OrderItem{
		ID:             id.New(),
		MenuItemID:     missingMenuItemID,
		Quantity:       1,
		UnitPricePaise: 32000,
		LineTotalPaise: 32000,
	}
	env := envelopeFor(id.New(), fx.tenantID, fx.outletID, 2)
	resp := postEnvelope(t, router, "/orders/"+orderID+"/items", env, item)

	var body struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	}
	raw := resp.Body.Bytes()
	if err := json.Unmarshal(raw, &body); err != nil {
		t.Fatalf("decoding error envelope %q: %v", string(raw), err)
	}

	if resp.Code >= 500 {
		t.Errorf("status = %d (%s), want a 4xx: an FK violation on a replayed row is the "+
			"CLIENT's data being wrong, and reporting it as a server fault makes the edge "+
			"retry it forever", resp.Code, body.Code)
	}
	if resp.Code != http.StatusUnprocessableEntity {
		t.Errorf("status = %d, want 422: a referenced row the cloud has never held is not a "+
			"state conflict a retry can resolve", resp.Code)
	}

	// The code is what the edge branches on; the message is for a human and
	// must never carry SQL. Both halves matter: a 422 whose code is
	// "internal_error" tells the edge nothing it can record.
	if body.Code != "missing_reference" {
		t.Errorf("code = %q, want %q — this string is what the edge records and classifies on",
			body.Code, "missing_reference")
	}
	if body.Message == "" {
		t.Errorf("message is empty; the operator-facing half of the reason is missing")
	}
	for _, leak := range []string{"SQLSTATE", "23503", "INSERT", "SELECT", "pgx", "fkey"} {
		if bytes.Contains(bytes.ToUpper(raw), bytes.ToUpper([]byte(leak))) {
			t.Errorf("error body leaks internal detail %q: %s", leak, string(raw))
		}
	}

	// The order itself must still be readable: refusing one item append is
	// not a reason to have lost the order it belonged to.
	var stored contracts.CanonicalOrder
	get := httptest.NewRecorder()
	router.ServeHTTP(get, httptest.NewRequest(http.MethodGet, "/orders/"+orderID, nil))
	if get.Code != http.StatusOK {
		t.Fatalf("GET /orders/%s after the refused append = %d, want 200", orderID, get.Code)
	}
	if err := json.Unmarshal(get.Body.Bytes(), &stored); err != nil {
		t.Fatalf("decoding order: %v", err)
	}
	if len(stored.Items) != 0 {
		t.Errorf("order carries %d items after the refused append, want 0", len(stored.Items))
	}
}

// newIngestRouter mounts the read and ingest routes behind the same
// principals backend/cmd/api/main.go uses, so the status code this test
// observes is the one a device replaying an envelope would receive.
func newIngestRouter(svc *ordering.Service, fx fixture) *chi.Mux {
	h := ordering.NewHandler(svc)
	// The read routes (Mount) run under a human principal in production and
	// the ingest routes (MountIngest) under a device credential. Both are
	// wrapped here so the post-refusal GET is answered by the same route a
	// person would hit, rather than 401ing for a reason unrelated to what
	// this test is about.
	principal := auth.AuthenticatedPrincipal{
		UserID:      "principal-user",
		TenantID:    fx.tenantID,
		OutletID:    fx.outletID,
		Permissions: []auth.Permission{auth.PermissionOrderCreate, auth.PermissionOrderModify, auth.PermissionOrderCancel},
	}
	devicePrincipal := outlet.DevicePrincipal{
		DeviceID: "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa",
		TenantID: fx.tenantID,
		OutletID: fx.outletID,
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
	h.Mount(r)
	return r
}

func createOrder(ctx context.Context, t *testing.T, router *chi.Mux, fx fixture, orderID string) {
	t.Helper()
	_ = ctx

	order := orderFor(orderID, fx.outletID)
	env := envelopeFor(orderID, fx.tenantID, fx.outletID, 1)
	resp := postEnvelope(t, router, "/orders", env, order)
	if resp.Code != http.StatusCreated && resp.Code != http.StatusOK {
		t.Fatalf("POST /orders = %d, body %s", resp.Code, resp.Body.String())
	}
}

func postEnvelope(t *testing.T, router *chi.Mux, path string, env contracts.SyncEnvelope, payload any) *httptest.ResponseRecorder {
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

// compile-time guard: this file must keep using the real Postgres
// repository. A fake cannot raise 23503, so a fake here would make the test
// green for the wrong reason.
var _ = func() *ordering.PostgresRepository { return ordering.NewPostgresRepository(postgres.Pool(nil)) }
