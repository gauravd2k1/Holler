package tables

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/holler/backend/internal/platform/id"
)

func newTestRouter(svc *Service) *chi.Mux {
	r := chi.NewRouter()
	NewHandlers(svc).Mount(r)
	return r
}

func doJSON(t *testing.T, r http.Handler, method, path string, body any) *httptest.ResponseRecorder {
	t.Helper()
	var reader *bytes.Reader
	if body != nil {
		b, err := json.Marshal(body)
		if err != nil {
			t.Fatalf("marshalling request body: %v", err)
		}
		reader = bytes.NewReader(b)
	} else {
		reader = bytes.NewReader(nil)
	}
	req := httptest.NewRequest(method, path, reader)
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	r.ServeHTTP(rec, req)
	return rec
}

func openEnvelope(recordID, outletID, tableID string, guestCount, version int) map[string]any {
	now := time.Now().UTC().Format(time.RFC3339Nano)
	return map[string]any{
		"record_id":      recordID,
		"tenant_id":      id.New(),
		"outlet_id":      outletID,
		"device_id":      id.New(),
		"aggregate_type": "table_session",
		"direction":      "EDGE_TO_CLOUD",
		"created_at":     now,
		"updated_at":     now,
		"version":        version,
		"sync_status":    "PENDING",
		"payload": map[string]any{
			"id":             recordID,
			"outlet_id":      outletID,
			"table_id":       tableID,
			"state":          "OCCUPIED",
			"guest_count":    guestCount,
			"opened_at":      now,
			"created_at":     now,
			"updated_at":     now,
			"version":        version,
			"schema_version": 1,
		},
	}
}

func transitionEnvelope(recordID, outletID, tableID, state string, version int) map[string]any {
	now := time.Now().UTC().Format(time.RFC3339Nano)
	return map[string]any{
		"record_id":      recordID,
		"tenant_id":      id.New(),
		"outlet_id":      outletID,
		"device_id":      id.New(),
		"aggregate_type": "table_session",
		"direction":      "EDGE_TO_CLOUD",
		"created_at":     now,
		"updated_at":     now,
		"version":        version,
		"sync_status":    "PENDING",
		"payload": map[string]any{
			"id":             recordID,
			"outlet_id":      outletID,
			"table_id":       tableID,
			"state":          state,
			"guest_count":    2,
			"opened_at":      now,
			"created_at":     now,
			"updated_at":     now,
			"version":        version,
			"schema_version": 1,
		},
	}
}

func setUpTableForSessions(t *testing.T, repo *fakeRepository, outletID string) string {
	t.Helper()
	svc := NewService(repo)
	tbl, err := svc.CreateTable(authorizedContext(), NewTableInput{OutletID: outletID, Section: "GROUND", Label: "T1", SeatCount: 4})
	if err != nil {
		t.Fatalf("CreateTable: %v", err)
	}
	return tbl.ID
}

func TestEnvelopeOpen_HappyPath(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	tableID := setUpTableForSessions(t, repo, outletID)
	svc := NewService(repo)
	router := newTestRouter(svc)

	recordID := id.New()
	rec := doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions", openEnvelope(recordID, outletID, tableID, 2, 1))
	if rec.Code != http.StatusCreated {
		t.Fatalf("expected 201, got %d: %s", rec.Code, rec.Body.String())
	}
	var got sessionWire
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil {
		t.Fatalf("decoding response: %v", err)
	}
	if got.ID != recordID || got.State != "OCCUPIED" {
		t.Fatalf("unexpected session: %+v", got)
	}
}

func TestEnvelopeOpen_RejectsRawNonEnvelopedBody(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	tableID := setUpTableForSessions(t, repo, outletID)
	svc := NewService(repo)
	router := newTestRouter(svc)

	raw := map[string]any{
		"id":          id.New(),
		"outlet_id":   outletID,
		"table_id":    tableID,
		"state":       "OCCUPIED",
		"guest_count": 2,
	}
	rec := doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions", raw)
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("expected 400 for a raw non-enveloped body, got %d: %s", rec.Code, rec.Body.String())
	}
}

func TestEnvelopeOpen_WrongAggregateTypeRejected422(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	tableID := setUpTableForSessions(t, repo, outletID)
	svc := NewService(repo)
	router := newTestRouter(svc)

	env := openEnvelope(id.New(), outletID, tableID, 2, 1)
	env["aggregate_type"] = "order"
	rec := doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions", env)
	if rec.Code != http.StatusUnprocessableEntity {
		t.Fatalf("expected 422 for wrong aggregate_type, got %d: %s", rec.Code, rec.Body.String())
	}
	var body envelopeMismatchBody
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decoding mismatch body: %v", err)
	}
	if body.Code != "envelope_route_mismatch" {
		t.Fatalf("expected code envelope_route_mismatch, got %q", body.Code)
	}
}

func TestEnvelopeOpen_WrongDirectionRejected422(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	tableID := setUpTableForSessions(t, repo, outletID)
	svc := NewService(repo)
	router := newTestRouter(svc)

	env := openEnvelope(id.New(), outletID, tableID, 2, 1)
	env["direction"] = "CLOUD_TO_EDGE"
	rec := doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions", env)
	if rec.Code != http.StatusUnprocessableEntity {
		t.Fatalf("expected 422 for wrong direction, got %d: %s", rec.Code, rec.Body.String())
	}
}

func TestEnvelopeOpen_DuplicateReplayIsIdempotent(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	tableID := setUpTableForSessions(t, repo, outletID)
	svc := NewService(repo)
	router := newTestRouter(svc)

	recordID := id.New()
	env := openEnvelope(recordID, outletID, tableID, 2, 1)

	first := doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions", env)
	if first.Code != http.StatusCreated {
		t.Fatalf("expected 201 on first replay, got %d: %s", first.Code, first.Body.String())
	}
	second := doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions", env)
	if second.Code != http.StatusCreated {
		t.Fatalf("expected 201 on duplicate replay, got %d: %s", second.Code, second.Body.String())
	}

	sessions, err := svc.ListOpenSessions(authorizedContext(), outletID)
	if err != nil {
		t.Fatalf("ListOpenSessions: %v", err)
	}
	if len(sessions) != 1 {
		t.Fatalf("expected exactly one open session after duplicate replay, got %d", len(sessions))
	}
	if sessions[0].State != "OCCUPIED" {
		t.Fatalf("expected OCCUPIED after duplicate replay, got %s", sessions[0].State)
	}
}

func TestEnvelopeTransition_HappyPath(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	tableID := setUpTableForSessions(t, repo, outletID)
	svc := NewService(repo)
	router := newTestRouter(svc)

	recordID := id.New()
	doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions", openEnvelope(recordID, outletID, tableID, 2, 1))

	rec := doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions/"+recordID, transitionEnvelope(recordID, outletID, tableID, "ORDERED", 2))
	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d: %s", rec.Code, rec.Body.String())
	}
	var got sessionWire
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil {
		t.Fatalf("decoding response: %v", err)
	}
	if got.State != "ORDERED" || got.Version != 2 {
		t.Fatalf("unexpected session after transition: %+v", got)
	}
}

func TestEnvelopeTransition_IllegalTransitionRejected409(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	tableID := setUpTableForSessions(t, repo, outletID)
	svc := NewService(repo)
	router := newTestRouter(svc)

	recordID := id.New()
	doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions", openEnvelope(recordID, outletID, tableID, 2, 1))

	// OCCUPIED -> PAID is not a legal edge.
	rec := doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions/"+recordID, transitionEnvelope(recordID, outletID, tableID, "PAID", 2))
	if rec.Code != http.StatusConflict {
		t.Fatalf("expected 409 for illegal transition, got %d: %s", rec.Code, rec.Body.String())
	}
}

func TestEnvelopeTransition_DuplicateReplayIsIdempotent(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	tableID := setUpTableForSessions(t, repo, outletID)
	svc := NewService(repo)
	router := newTestRouter(svc)

	recordID := id.New()
	doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions", openEnvelope(recordID, outletID, tableID, 2, 1))

	env := transitionEnvelope(recordID, outletID, tableID, "ORDERED", 2)
	first := doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions/"+recordID, env)
	if first.Code != http.StatusOK {
		t.Fatalf("expected 200 on first transition replay, got %d: %s", first.Code, first.Body.String())
	}
	second := doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions/"+recordID, env)
	if second.Code != http.StatusOK {
		t.Fatalf("expected 200 on duplicate transition replay, got %d: %s", second.Code, second.Body.String())
	}

	sess, err := svc.GetSession(authorizedContext(), outletID, recordID)
	if err != nil {
		t.Fatalf("GetSession: %v", err)
	}
	if sess.State != "ORDERED" || sess.Version != 2 {
		t.Fatalf("expected session to remain at ORDERED/version 2 after duplicate replay, got state=%s version=%d", sess.State, sess.Version)
	}
}

func TestEnvelopeGet_UnwrappedReadPaths(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	tableID := setUpTableForSessions(t, repo, outletID)
	svc := NewService(repo)
	router := newTestRouter(svc)

	recordID := id.New()
	doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions", openEnvelope(recordID, outletID, tableID, 2, 1))

	listRec := doJSON(t, router, http.MethodGet, "/outlets/"+outletID+"/table-sessions", nil)
	if listRec.Code != http.StatusOK {
		t.Fatalf("expected 200 from list, got %d: %s", listRec.Code, listRec.Body.String())
	}
	var list []sessionWire
	if err := json.Unmarshal(listRec.Body.Bytes(), &list); err != nil {
		t.Fatalf("decoding list response: %v", err)
	}
	if len(list) != 1 || list[0].ID != recordID {
		t.Fatalf("expected unwrapped list containing the session, got %+v", list)
	}

	getRec := doJSON(t, router, http.MethodGet, "/outlets/"+outletID+"/table-sessions/"+recordID, nil)
	if getRec.Code != http.StatusOK {
		t.Fatalf("expected 200 from get, got %d: %s", getRec.Code, getRec.Body.String())
	}
	var got sessionWire
	if err := json.Unmarshal(getRec.Body.Bytes(), &got); err != nil {
		t.Fatalf("decoding get response: %v", err)
	}
	if got.ID != recordID {
		t.Fatalf("expected unwrapped session, got %+v", got)
	}
}

// --- existing guarantees, exercised through the HTTP surface ---------------

func TestEnvelopeOpen_NeverBumpsConfigVersion(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	tableID := setUpTableForSessions(t, repo, outletID)
	svc := NewService(repo)
	router := newTestRouter(svc)

	versionBeforeSession := repo.outletVersions[outletID]

	doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions", openEnvelope(id.New(), outletID, tableID, 2, 1))

	if repo.outletVersions[outletID] != versionBeforeSession {
		t.Fatalf("expected session ingest never to bump outlet config_version, before=%d after=%d",
			versionBeforeSession, repo.outletVersions[outletID])
	}
	if repo.bumpCalls != 1 { // exactly the one bump from CreateTable in setup
		t.Fatalf("expected exactly one config_version bump (from table creation), got %d", repo.bumpCalls)
	}
}

func TestEnvelopeOpen_RejectsSecondOpenSessionForDifferentRecord(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	tableID := setUpTableForSessions(t, repo, outletID)
	svc := NewService(repo)
	router := newTestRouter(svc)

	doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions", openEnvelope(id.New(), outletID, tableID, 2, 1))
	rec := doJSON(t, router, http.MethodPost, "/outlets/"+outletID+"/table-sessions", openEnvelope(id.New(), outletID, tableID, 3, 1))
	if rec.Code != http.StatusConflict {
		t.Fatalf("expected 409 conflict for a second distinct session on the same table, got %d: %s", rec.Code, rec.Body.String())
	}
}
