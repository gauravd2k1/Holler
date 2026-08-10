package kitchen

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/holler/backend/internal/auth"
	contracts "github.com/holler/contracts"
)

func newTestRouter(t *testing.T) (*chi.Mux, *fakeRepository) {
	t.Helper()
	repo := newFakeRepository()
	repo.outletTenant[testOutletID] = testTenantID
	svc := NewService(repo, nil)
	h := NewHandler(svc)

	principal := auth.AuthenticatedPrincipal{
		UserID:   "principal-user",
		TenantID: testTenantID,
		OutletID: testOutletID,
		Permissions: []auth.Permission{
			auth.PermissionOrderModify, auth.PermissionMenuManage, auth.PermissionOutletManage,
		},
	}

	r := chi.NewRouter()
	r.Use(func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
			next.ServeHTTP(w, req.WithContext(auth.WithPrincipal(req.Context(), principal)))
		})
	})
	h.Mount(r)
	return r, repo
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
		AggregateType: string(contracts.AggregateTypeKot),
		Direction:     string(contracts.SyncDirectionEdgeToCloud),
		CreatedAt:     now,
		UpdatedAt:     now,
		Version:       version,
		SyncStatus:    string(contracts.SyncStatusPending),
		Payload:       payload,
	}
}

func kotPayload() map[string]interface{} {
	k := baseKot()
	return map[string]interface{}{
		"id":                   k.ID,
		"order_id":             k.OrderID,
		"station":              k.Station,
		"sequence":             k.Sequence,
		"status":               string(k.Status),
		"items":                k.Items,
		"created_by_device_id": k.CreatedByDeviceID,
		"created_at":           k.CreatedAt.Format(time.RFC3339Nano),
		"updated_at":           k.UpdatedAt.Format(time.RFC3339Nano),
		"schema_version":       1,
	}
}

func TestIngestKotHTTP_HappyPath(t *testing.T) {
	r, repo := newTestRouter(t)
	repo.orderOutlet[testOrderID] = testOutletID

	env := wireEnvelopeFor(testKotID, 1, kotPayload())
	rec := doPost(t, r, "/orders/"+testOrderID+"/kots", env)
	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d: %s", rec.Code, rec.Body.String())
	}
	var got contracts.Kot
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil {
		t.Fatalf("decoding response: %v", err)
	}
	if got.ID != testKotID || got.Status != contracts.KotStatusNew {
		t.Fatalf("unexpected kot: %+v", got)
	}
}

func TestIngestKotHTTP_RawKotBodyIsRejected(t *testing.T) {
	r, repo := newTestRouter(t)
	repo.orderOutlet[testOrderID] = testOutletID

	rec := doPost(t, r, "/orders/"+testOrderID+"/kots", kotPayload())
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("expected 400 for a bare Kot body, got %d: %s", rec.Code, rec.Body.String())
	}
}

func TestIngestKotHTTP_WrongAggregateTypeIsRejected(t *testing.T) {
	r, repo := newTestRouter(t)
	repo.orderOutlet[testOrderID] = testOutletID

	env := wireEnvelopeFor(testKotID, 1, kotPayload())
	env.AggregateType = string(contracts.AggregateTypeOrder)
	rec := doPost(t, r, "/orders/"+testOrderID+"/kots", env)
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

func TestIngestKotHTTP_CloudToEdgeDirectionIsRejected(t *testing.T) {
	r, repo := newTestRouter(t)
	repo.orderOutlet[testOrderID] = testOutletID

	env := wireEnvelopeFor(testKotID, 1, kotPayload())
	env.Direction = string(contracts.SyncDirectionCloudToEdge)
	rec := doPost(t, r, "/orders/"+testOrderID+"/kots", env)
	if rec.Code != http.StatusUnprocessableEntity {
		t.Fatalf("expected 422, got %d: %s", rec.Code, rec.Body.String())
	}
}

func TestIngestKotStatusHTTP_HappyPath(t *testing.T) {
	r, repo := newTestRouter(t)
	repo.orderOutlet[testOrderID] = testOutletID

	if rec := doPost(t, r, "/orders/"+testOrderID+"/kots", wireEnvelopeFor(testKotID, 1, kotPayload())); rec.Code != http.StatusOK {
		t.Fatalf("setup: expected 200 creating kot, got %d: %s", rec.Code, rec.Body.String())
	}

	statusPayload := map[string]interface{}{
		"status":               "ACKNOWLEDGED",
		"changed_at":           time.Now().UTC().Format(time.RFC3339Nano),
		"changed_by_device_id": testDeviceID,
	}
	env := wireEnvelopeFor(testKotID, 2, statusPayload)
	rec := doPost(t, r, "/kots/"+testKotID+"/status", env)
	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d: %s", rec.Code, rec.Body.String())
	}
	var got contracts.Kot
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil {
		t.Fatalf("decoding response: %v", err)
	}
	if got.Status != contracts.KotStatusAcknowledged {
		t.Fatalf("expected ACKNOWLEDGED, got %s", got.Status)
	}
}

func TestIngestKotStatusHTTP_IllegalTransitionIsConflict(t *testing.T) {
	r, repo := newTestRouter(t)
	repo.orderOutlet[testOrderID] = testOutletID

	if rec := doPost(t, r, "/orders/"+testOrderID+"/kots", wireEnvelopeFor(testKotID, 1, kotPayload())); rec.Code != http.StatusOK {
		t.Fatalf("setup: expected 200 creating kot, got %d: %s", rec.Code, rec.Body.String())
	}

	statusPayload := map[string]interface{}{
		"status":               "SERVED",
		"changed_at":           time.Now().UTC().Format(time.RFC3339Nano),
		"changed_by_device_id": testDeviceID,
	}
	rec := doPost(t, r, "/kots/"+testKotID+"/status", wireEnvelopeFor(testKotID, 2, statusPayload))
	if rec.Code != http.StatusConflict {
		t.Fatalf("expected 409, got %d: %s", rec.Code, rec.Body.String())
	}
}

func TestCreateStationHTTP_HappyPath(t *testing.T) {
	r, _ := newTestRouter(t)

	rec := doPost(t, r, "/stations", map[string]interface{}{
		"id": testStationID, "outlet_id": testOutletID, "code": "TANDOOR", "name": "Tandoor",
	})
	if rec.Code != http.StatusCreated {
		t.Fatalf("expected 201, got %d: %s", rec.Code, rec.Body.String())
	}
}

func TestCreatePrinterHTTP_HappyPath(t *testing.T) {
	r, _ := newTestRouter(t)

	rec := doPost(t, r, "/printers", map[string]interface{}{
		"id": testPrinterID, "outlet_id": testOutletID, "name": "Kitchen Printer",
		"connection_kind": "ESCPOS_NETWORK", "address": "192.168.1.50:9100", "paper_width_mm": 80,
	})
	if rec.Code != http.StatusCreated {
		t.Fatalf("expected 201, got %d: %s", rec.Code, rec.Body.String())
	}
}
