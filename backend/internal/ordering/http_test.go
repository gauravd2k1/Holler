package ordering

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/outlet"
	contracts "github.com/holler/contracts"
)

// newTestRouter mounts the ordering handler's read routes behind a human
// principal and its ingest routes behind a device principal, mirroring how
// backend/cmd/api/main.go splits auth.Authenticate from
// outlet.DeviceAuthenticate across the two Mount methods (ADR-017 0.4.3
// amendment). Every write route this test suite exercises goes through
// MountIngest and therefore the device principal, not the human one.
func newTestRouter(t *testing.T) (*chi.Mux, *fakeRepo) {
	t.Helper()
	repo := newFakeRepo()
	repo.outletOK[testOutletID] = true
	svc := NewService(repo)
	h := NewHandler(svc)

	principal := auth.AuthenticatedPrincipal{
		UserID:      "principal-user",
		TenantID:    testTenantID,
		OutletID:    testOutletID,
		Permissions: []auth.Permission{auth.PermissionOrderCreate, auth.PermissionOrderModify, auth.PermissionOrderCancel},
	}
	devicePrincipal := outlet.DevicePrincipal{
		DeviceID: testDeviceID,
		TenantID: testTenantID,
		OutletID: testOutletID,
	}

	r := chi.NewRouter()
	r.Use(func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
			ctx := auth.WithPrincipal(req.Context(), principal)
			ctx = outlet.WithDevicePrincipal(ctx, devicePrincipal)
			next.ServeHTTP(w, req.WithContext(ctx))
		})
	})
	h.Mount(r)
	h.MountIngest(r)
	return r, repo
}

type wireEnvelope struct {
	RecordID      string      `json:"record_id"`
	TenantID      string      `json:"tenant_id"`
	OutletID      string      `json:"outlet_id"`
	DeviceID      string      `json:"device_id"`
	AggregateType string      `json:"aggregate_type"`
	Direction     string      `json:"direction"`
	CreatedAt     time.Time   `json:"created_at"`
	UpdatedAt     time.Time   `json:"updated_at"`
	Version       int         `json:"version"`
	SyncStatus    string      `json:"sync_status"`
	Payload       interface{} `json:"payload"`
}

func wireEnvelopeFor(recordID string, version int, payload interface{}) wireEnvelope {
	now := time.Now().UTC()
	return wireEnvelope{
		RecordID:      recordID,
		TenantID:      testTenantID,
		OutletID:      testOutletID,
		DeviceID:      testDeviceID,
		AggregateType: string(contracts.AggregateTypeOrder),
		Direction:     string(contracts.SyncDirectionEdgeToCloud),
		CreatedAt:     now,
		UpdatedAt:     now,
		Version:       version,
		SyncStatus:    string(contracts.SyncStatusPending),
		Payload:       payload,
	}
}

func doPost(t *testing.T, r http.Handler, path string, body interface{}) *httptest.ResponseRecorder {
	t.Helper()
	raw, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("marshalling request body: %v", err)
	}
	req := httptest.NewRequest(http.MethodPost, path, bytes.NewReader(raw))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	r.ServeHTTP(rec, req)
	return rec
}

// TestCreateOrderHTTP_HappyPath posts a well-formed OrderEnvelope and checks
// the persisted order matches the payload.
func TestCreateOrderHTTP_HappyPath(t *testing.T) {
	r, _ := newTestRouter(t)

	env := wireEnvelopeFor(testOrderID, 1, baseOrder())
	rec := doPost(t, r, "/orders", env)

	if rec.Code != http.StatusCreated {
		t.Fatalf("expected 201, got %d: %s", rec.Code, rec.Body.String())
	}
	var got contracts.CanonicalOrder
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil {
		t.Fatalf("decoding response: %v", err)
	}
	if got.HollerOrderID != testOrderID {
		t.Fatalf("expected holler_order_id %s, got %s", testOrderID, got.HollerOrderID)
	}
	if got.OutletID != testOutletID {
		t.Fatalf("expected outlet_id %s, got %s", testOutletID, got.OutletID)
	}
	if got.Status != contracts.OrderStatusDraft {
		t.Fatalf("expected status DRAFT, got %s", got.Status)
	}
}

// TestCreateOrderHTTP_RawCanonicalOrderBodyIsRejected proves the pre-0.2.1
// unwrapped-CanonicalOrder body is genuinely rejected (400), not silently
// half-parsed into a mostly-empty envelope.
func TestCreateOrderHTTP_RawCanonicalOrderBodyIsRejected(t *testing.T) {
	r, repo := newTestRouter(t)

	rec := doPost(t, r, "/orders", baseOrder())

	if rec.Code != http.StatusBadRequest {
		t.Fatalf("expected 400 for a bare CanonicalOrder body, got %d: %s", rec.Code, rec.Body.String())
	}
	if len(repo.orders) != 0 {
		t.Fatalf("expected no order to have been created, got %d", len(repo.orders))
	}
}

// TestCreateOrderHTTP_WrongAggregateTypeIsRejected proves an envelope whose
// aggregate_type is not `order` is rejected with the contracted 422
// EnvelopeRouteMismatch response, not coerced.
func TestCreateOrderHTTP_WrongAggregateTypeIsRejected(t *testing.T) {
	r, repo := newTestRouter(t)

	env := wireEnvelopeFor(testOrderID, 1, baseOrder())
	env.AggregateType = string(contracts.AggregateTypeKot)
	rec := doPost(t, r, "/orders", env)

	if rec.Code != http.StatusUnprocessableEntity {
		t.Fatalf("expected 422, got %d: %s", rec.Code, rec.Body.String())
	}
	var body envelopeRouteMismatchBody
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decoding response: %v", err)
	}
	if body.Code != "envelope_route_mismatch" {
		t.Fatalf("expected code envelope_route_mismatch, got %q", body.Code)
	}
	if len(repo.orders) != 0 {
		t.Fatalf("expected no order to have been created, got %d", len(repo.orders))
	}
}

// TestCreateOrderHTTP_CloudToEdgeDirectionIsRejected proves an order
// envelope carrying CLOUD_TO_EDGE — a direction violating §50.1's authority
// rule for the order aggregate — is rejected with 422, not coerced.
func TestCreateOrderHTTP_CloudToEdgeDirectionIsRejected(t *testing.T) {
	r, repo := newTestRouter(t)

	env := wireEnvelopeFor(testOrderID, 1, baseOrder())
	env.Direction = string(contracts.SyncDirectionCloudToEdge)
	rec := doPost(t, r, "/orders", env)

	if rec.Code != http.StatusUnprocessableEntity {
		t.Fatalf("expected 422, got %d: %s", rec.Code, rec.Body.String())
	}
	var body envelopeRouteMismatchBody
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decoding response: %v", err)
	}
	if body.Code != "envelope_route_mismatch" {
		t.Fatalf("expected code envelope_route_mismatch, got %q", body.Code)
	}
	if len(repo.orders) != 0 {
		t.Fatalf("expected no order to have been created, got %d", len(repo.orders))
	}
}

// TestCreateOrderHTTP_DuplicateEnvelopeIsIdempotent replays the identical
// envelope twice through the HTTP layer and asserts exactly one order
// results — idempotency proven at the HTTP boundary, not only the service
// layer.
func TestCreateOrderHTTP_DuplicateEnvelopeIsIdempotent(t *testing.T) {
	r, repo := newTestRouter(t)

	env := wireEnvelopeFor(testOrderID, 1, baseOrder())

	first := doPost(t, r, "/orders", env)
	if first.Code != http.StatusCreated {
		t.Fatalf("expected 201 on first delivery, got %d: %s", first.Code, first.Body.String())
	}
	second := doPost(t, r, "/orders", env)
	if second.Code != http.StatusCreated {
		t.Fatalf("expected 201 on duplicate delivery (idempotent replay), got %d: %s", second.Code, second.Body.String())
	}

	if len(repo.orders) != 1 {
		t.Fatalf("expected exactly one order after duplicate replay, got %d", len(repo.orders))
	}
}

// confirmPayloadFor builds the OrderConfirmEnvelope payload shape.
func confirmPayloadFor(confirmedAt time.Time) map[string]interface{} {
	return map[string]interface{}{"confirmed_at": confirmedAt.Format(time.RFC3339Nano)}
}

// TestConfirmOrderHTTP_HappyPath posts a well-formed OrderConfirmEnvelope
// against a DRAFT order and asserts the stored confirmed_at equals the
// envelope payload's value, not the server clock.
func TestConfirmOrderHTTP_HappyPath(t *testing.T) {
	r, _ := newTestRouter(t)

	createEnv := wireEnvelopeFor(testOrderID, 1, baseOrder())
	if rec := doPost(t, r, "/orders", createEnv); rec.Code != http.StatusCreated {
		t.Fatalf("setup: expected 201 creating order, got %d: %s", rec.Code, rec.Body.String())
	}

	confirmedAt := time.Date(2026, 8, 8, 10, 30, 0, 0, time.UTC)
	confirmEnv := wireEnvelopeFor(testOrderID, 2, confirmPayloadFor(confirmedAt))
	rec := doPost(t, r, "/orders/"+testOrderID+"/confirm", confirmEnv)
	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d: %s", rec.Code, rec.Body.String())
	}
	var got contracts.CanonicalOrder
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil {
		t.Fatalf("decoding response: %v", err)
	}
	if got.Status != contracts.OrderStatusConfirmed {
		t.Fatalf("expected CONFIRMED, got %s", got.Status)
	}
	if got.Timestamps.ConfirmedAt == nil || !got.Timestamps.ConfirmedAt.Equal(confirmedAt) {
		t.Fatalf("expected confirmed_at %v (envelope payload), got %v", confirmedAt, got.Timestamps.ConfirmedAt)
	}
}

// TestConfirmOrderHTTP_NonDraftIsConflict proves confirming an order that
// left DRAFT WITHOUT EVER BEING CONFIRMED is rejected with 409, not silently
// applied — and that the same call against an order that WAS confirmed and has
// since moved on is absorbed instead (contracts 0.8.3, ADR-028).
//
// The difference is `confirmed_at`, not the status. A confirm arriving at a
// SENT_TO_KITCHEN order is the edge redelivering a row whose acknowledgement
// was lost; refusing it is a permanent 409 that strands every row queued
// behind it. A confirm arriving at an order that was cancelled straight out of
// DRAFT is a command that never happened, and stays refused.
func TestConfirmOrderHTTP_NonDraftIsConflict(t *testing.T) {
	r, _ := newTestRouter(t)

	createEnv := wireEnvelopeFor(testOrderID, 1, baseOrder())
	if rec := doPost(t, r, "/orders", createEnv); rec.Code != http.StatusCreated {
		t.Fatalf("setup: expected 201 creating order, got %d: %s", rec.Code, rec.Body.String())
	}
	firstConfirm := wireEnvelopeFor(testOrderID, 2, confirmPayloadFor(time.Now().UTC()))
	if rec := doPost(t, r, "/orders/"+testOrderID+"/confirm", firstConfirm); rec.Code != http.StatusOK {
		t.Fatalf("setup: expected 200 confirming order, got %d: %s", rec.Code, rec.Body.String())
	}
	if rec := doPost(t, r, "/orders/"+testOrderID+"/send-to-kitchen", wireEnvelopeFor(testOrderID, 3, map[string]interface{}{})); rec.Code != http.StatusOK {
		t.Fatalf("setup: expected 200 sending to kitchen, got %d: %s", rec.Code, rec.Body.String())
	}

	// Redelivered after the order moved on: absorbed, because this order
	// carries the evidence that the confirm WAS applied.
	rec := doPost(t, r, "/orders/"+testOrderID+"/confirm", wireEnvelopeFor(testOrderID, 4, confirmPayloadFor(time.Now().UTC())))
	if rec.Code != http.StatusOK {
		t.Fatalf("expected a redelivered confirm to be absorbed with 200, got %d: %s", rec.Code, rec.Body.String())
	}

	// An order that left DRAFT and was NEVER confirmed is still a 409.
	r2, repo := newTestRouter(t)
	if rec := doPost(t, r2, "/orders", wireEnvelopeFor(testOrderID, 1, baseOrder())); rec.Code != http.StatusCreated {
		t.Fatalf("setup: expected 201 creating order, got %d: %s", rec.Code, rec.Body.String())
	}
	cancelled := repo.orders[testOrderID]
	cancelled.Status = contracts.OrderStatusCancelled
	repo.orders[testOrderID] = cancelled
	rec = doPost(t, r2, "/orders/"+testOrderID+"/confirm", wireEnvelopeFor(testOrderID, 2, confirmPayloadFor(time.Now().UTC())))
	if rec.Code != http.StatusConflict {
		t.Fatalf("expected 409 confirming an order cancelled out of DRAFT, got %d: %s", rec.Code, rec.Body.String())
	}
}

// TestConfirmOrderHTTP_WrongAggregateTypeIsRejected proves the 422
// EnvelopeRouteMismatch path on the confirm route.
func TestConfirmOrderHTTP_WrongAggregateTypeIsRejected(t *testing.T) {
	r, _ := newTestRouter(t)

	createEnv := wireEnvelopeFor(testOrderID, 1, baseOrder())
	if rec := doPost(t, r, "/orders", createEnv); rec.Code != http.StatusCreated {
		t.Fatalf("setup: expected 201 creating order, got %d: %s", rec.Code, rec.Body.String())
	}

	confirmEnv := wireEnvelopeFor(testOrderID, 2, confirmPayloadFor(time.Now().UTC()))
	confirmEnv.AggregateType = string(contracts.AggregateTypeKot)
	rec := doPost(t, r, "/orders/"+testOrderID+"/confirm", confirmEnv)
	if rec.Code != http.StatusUnprocessableEntity {
		t.Fatalf("expected 422, got %d: %s", rec.Code, rec.Body.String())
	}
	var body envelopeRouteMismatchBody
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decoding response: %v", err)
	}
	if body.Code != "envelope_route_mismatch" {
		t.Fatalf("expected code envelope_route_mismatch, got %q", body.Code)
	}
}

// TestConfirmOrderHTTP_CloudToEdgeDirectionIsRejected proves a confirm
// envelope carrying CLOUD_TO_EDGE — violating §50.1's authority rule for the
// order aggregate — is rejected with 422, not coerced.
func TestConfirmOrderHTTP_CloudToEdgeDirectionIsRejected(t *testing.T) {
	r, _ := newTestRouter(t)

	createEnv := wireEnvelopeFor(testOrderID, 1, baseOrder())
	if rec := doPost(t, r, "/orders", createEnv); rec.Code != http.StatusCreated {
		t.Fatalf("setup: expected 201 creating order, got %d: %s", rec.Code, rec.Body.String())
	}

	confirmEnv := wireEnvelopeFor(testOrderID, 2, confirmPayloadFor(time.Now().UTC()))
	confirmEnv.Direction = string(contracts.SyncDirectionCloudToEdge)
	rec := doPost(t, r, "/orders/"+testOrderID+"/confirm", confirmEnv)
	if rec.Code != http.StatusUnprocessableEntity {
		t.Fatalf("expected 422, got %d: %s", rec.Code, rec.Body.String())
	}
	var body envelopeRouteMismatchBody
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decoding response: %v", err)
	}
	if body.Code != "envelope_route_mismatch" {
		t.Fatalf("expected code envelope_route_mismatch, got %q", body.Code)
	}
}

// TestConfirmOrderHTTP_RawNonEnvelopedBodyIsRejected proves a bare
// {confirmed_at} body (not wrapped in a SyncEnvelope) is refused as 400.
func TestConfirmOrderHTTP_RawNonEnvelopedBodyIsRejected(t *testing.T) {
	r, _ := newTestRouter(t)

	createEnv := wireEnvelopeFor(testOrderID, 1, baseOrder())
	if rec := doPost(t, r, "/orders", createEnv); rec.Code != http.StatusCreated {
		t.Fatalf("setup: expected 201 creating order, got %d: %s", rec.Code, rec.Body.String())
	}

	rec := doPost(t, r, "/orders/"+testOrderID+"/confirm", confirmPayloadFor(time.Now().UTC()))
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("expected 400 for a bare confirm payload, got %d: %s", rec.Code, rec.Body.String())
	}
}

// TestConfirmOrderHTTP_DuplicateReplayIsIdempotent replays the identical
// confirm envelope twice through the HTTP layer and asserts the stored
// confirmed_at is unchanged.
func TestConfirmOrderHTTP_DuplicateReplayIsIdempotent(t *testing.T) {
	r, _ := newTestRouter(t)

	createEnv := wireEnvelopeFor(testOrderID, 1, baseOrder())
	if rec := doPost(t, r, "/orders", createEnv); rec.Code != http.StatusCreated {
		t.Fatalf("setup: expected 201 creating order, got %d: %s", rec.Code, rec.Body.String())
	}

	confirmedAt := time.Date(2026, 8, 8, 10, 30, 0, 0, time.UTC)
	confirmEnv := wireEnvelopeFor(testOrderID, 2, confirmPayloadFor(confirmedAt))

	first := doPost(t, r, "/orders/"+testOrderID+"/confirm", confirmEnv)
	if first.Code != http.StatusOK {
		t.Fatalf("expected 200 on first confirm, got %d: %s", first.Code, first.Body.String())
	}
	second := doPost(t, r, "/orders/"+testOrderID+"/confirm", confirmEnv)
	if second.Code != http.StatusOK {
		t.Fatalf("expected 200 on duplicate confirm replay, got %d: %s", second.Code, second.Body.String())
	}
	var got contracts.CanonicalOrder
	if err := json.Unmarshal(second.Body.Bytes(), &got); err != nil {
		t.Fatalf("decoding response: %v", err)
	}
	if got.Timestamps.ConfirmedAt == nil || !got.Timestamps.ConfirmedAt.Equal(confirmedAt) {
		t.Fatalf("expected confirmed_at to remain %v after replay, got %v", confirmedAt, got.Timestamps.ConfirmedAt)
	}
}

// TestAppendItemHTTP_WrongDirectionIsRejected exercises the 422 path on a
// second envelope-ingest route, not just /orders.
func TestAppendItemHTTP_WrongDirectionIsRejected(t *testing.T) {
	r, _ := newTestRouter(t)

	createEnv := wireEnvelopeFor(testOrderID, 1, baseOrder())
	if rec := doPost(t, r, "/orders", createEnv); rec.Code != http.StatusCreated {
		t.Fatalf("setup: expected 201 creating order, got %d: %s", rec.Code, rec.Body.String())
	}

	item := contracts.OrderItem{
		ID:             "55555555-5555-7555-8555-555555555555",
		MenuItemID:     "66666666-6666-7666-8666-666666666666",
		Quantity:       1,
		UnitPricePaise: 10000,
		LineTotalPaise: 10000,
	}
	itemEnv := wireEnvelopeFor(testOrderID, 1, item)
	itemEnv.Direction = string(contracts.SyncDirectionCloudToEdge)

	rec := doPost(t, r, "/orders/"+testOrderID+"/items", itemEnv)
	if rec.Code != http.StatusUnprocessableEntity {
		t.Fatalf("expected 422, got %d: %s", rec.Code, rec.Body.String())
	}
}

// TestOrderLifecycle_ReplayedThroughTheSyncPath drives ONE order's COMPLETE
// edge-legal life through the ingest routes the way the edge's outbox
// actually sends it, and asserts the cloud ends in the state the edge is in.
//
// WHY A WHOLE LIFECYCLE AND NOT ANOTHER SINGLE-TRANSITION TEST. Two defects
// shipped together on 2026-09-16 (ADR-028) and every single-transition test on
// both sides passed throughout, because each one sets up the exact predecessor
// state it needs and then makes one call. Neither defect lives in one call:
//
//   - The envelope's version is read from the LIVE aggregate row when the row
//     is sent, so it only takes a wrong value once several events are queued
//     for one order. A test that posts one transition at the version it
//     "should" have can never construct that.
//   - The cloud's DRAFT-only line-amendment rule was unreachable while a
//     separate defect kept cloud-side orders in DRAFT. Defect A masked defect
//     B. Only a run that gets the order OUT of DRAFT and then appends reaches
//     it.
//
// The sequence below is what an outlet that was offline all service produces:
// four events recorded locally (the edge row reaching version 4), then one
// drain in which EVERY envelope carries version 4 — the live row's version, the
// same number on all four — plus one row redelivered because its
// acknowledgement was lost. All of it is legal at the edge and all of it must
// land.
func TestOrderLifecycle_ReplayedThroughTheSyncPath(t *testing.T) {
	r, _ := newTestRouter(t)

	// What the edge recorded locally, in order. The order row reached version
	// 4; nothing synced while it was happening.
	const edgeVersionAtDrainTime = 4
	confirmedAt := time.Date(2026, 9, 16, 12, 30, 0, 0, time.UTC)

	firstItem := contracts.OrderItem{
		ID:             "55555555-5555-7555-8555-555555555555",
		MenuItemID:     "66666666-6666-7666-8666-666666666666",
		Quantity:       1,
		UnitPricePaise: 44500,
		LineTotalPaise: 44500,
	}
	// The second round, added AFTER the order went to the kitchen — #132-A,
	// and the second Send on a table the captain page produces. Legal at the
	// edge; refused by the cloud until contracts 0.8.3.
	secondItem := contracts.OrderItem{
		ID:             "77777777-7777-7777-8777-777777777777",
		MenuItemID:     "88888888-8888-7888-8888-888888888888",
		Quantity:       2,
		UnitPricePaise: 52500,
		LineTotalPaise: 105000,
	}

	post := func(step, path string, payload interface{}, want int) {
		t.Helper()
		env := wireEnvelopeFor(testOrderID, edgeVersionAtDrainTime, payload)
		rec := doPost(t, r, path, env)
		if rec.Code != want {
			t.Fatalf("%s: expected %d, got %d: %s", step, want, rec.Code, rec.Body.String())
		}
	}

	post("create", "/orders", baseOrder(), http.StatusCreated)
	post("first line", "/orders/"+testOrderID+"/items", firstItem, http.StatusOK)
	post("confirm", "/orders/"+testOrderID+"/confirm",
		map[string]interface{}{"confirmed_at": confirmedAt}, http.StatusOK)
	post("send to kitchen", "/orders/"+testOrderID+"/send-to-kitchen",
		map[string]interface{}{}, http.StatusOK)
	// The append that defect B refused. It arrives with the order already
	// SENT_TO_KITCHEN cloud-side, which is the whole point.
	post("second round, after the kitchen already has the ticket",
		"/orders/"+testOrderID+"/items", secondItem, http.StatusOK)

	// AT-LEAST-ONCE: the confirm's acknowledgement was lost, so the edge sends
	// that row again. It must be absorbed, not refused — a 409 here is
	// permanent and strands every row behind it.
	post("redelivered confirm", "/orders/"+testOrderID+"/confirm",
		map[string]interface{}{"confirmed_at": confirmedAt.Add(7 * time.Minute)},
		http.StatusOK)

	// The cloud must now hold what the edge holds.
	req := httptest.NewRequest(http.MethodGet, "/orders/"+testOrderID, nil)
	rec := httptest.NewRecorder()
	r.ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("reading the order back: expected 200, got %d: %s", rec.Code, rec.Body.String())
	}
	var got contracts.CanonicalOrder
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil {
		t.Fatalf("decoding the order: %v", err)
	}

	if got.Status != contracts.OrderStatusSentToKitchen {
		t.Fatalf("expected the cloud to end at SENT_TO_KITCHEN like the edge, got %s", got.Status)
	}
	if len(got.Items) != 2 {
		t.Fatalf("expected both lines to have landed, got %d", len(got.Items))
	}
	if got.Items[0].ID != firstItem.ID || got.Items[1].ID != secondItem.ID {
		t.Fatalf("expected the lines in the order the edge appended them, got %s then %s",
			got.Items[0].ID, got.Items[1].ID)
	}
	// The redelivered confirm must not have moved the moment the EDGE
	// recorded (§50.1) — the cloud never substitutes a later timestamp.
	if got.Timestamps.ConfirmedAt == nil || !got.Timestamps.ConfirmedAt.Equal(confirmedAt) {
		t.Fatalf("expected confirmed_at to stay %v after redelivery, got %v",
			confirmedAt, got.Timestamps.ConfirmedAt)
	}
}
