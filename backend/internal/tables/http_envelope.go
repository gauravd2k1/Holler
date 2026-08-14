package tables

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"time"

	"github.com/go-chi/chi/v5"
	contracts "github.com/holler/contracts"

	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/httpx"
)

// MountEnvelopeIngest registers the envelope-wrapped table_session WRITE
// routes (contracts 0.2.1, ADR-011 addendum §1-2): POST
// /outlets/{outletId}/table-sessions and
// /outlets/{outletId}/table-sessions/{sessionId}. table_session rides the
// single edge→cloud replay pattern — there is no bespoke unwrapped write
// route.
//
// These are edge->cloud replay by definition (§50.1) — the caller is always
// an enrolled device, never a browser — so backend/cmd/api mounts this under
// outlet.DeviceAuthenticate rather than auth.Authenticate (ADR-017's 0.4.3
// amendment, the same fix applied to backend/internal/ordering and
// backend/internal/kitchen). The read side lives in MountEnvelopeRead,
// mounted separately under the human-auth group.
// Deliberately NOT r.Route(): chi.Mux.Route() always calls Mount(), which
// claims its pattern for ALL HTTP methods in the routing tree (see
// go-chi/chi/v5 mux.go's own comment: "if you define two Mount() routes on
// the exact same pattern the mount will panic"). MountEnvelopeIngest and
// MountEnvelopeRead register different HTTP methods on the SAME pattern from
// two different middleware groups in backend/cmd/api/main.go, which would
// collide if either used Route() — plain method-specific registration
// (r.Post/r.Get) has no such conflict, which is exactly REST routing.
//
// Each pattern is registered twice — with and without a trailing slash — to
// preserve the tolerance the old r.Route(...).Post("/") form gave for free
// (chi's Mount() accepts both "/outlets/x/table-sessions" and
// "/outlets/x/table-sessions/"). Plain r.Post/r.Get is exact-match only, so
// without the explicit second registration the trailing-slash variant 404s
// — a silent behaviour change from before this route was split, found in
// T8's gate. Neither registration uses r.Route/Mount, so the panic risk
// above does not reapply.
func (h *Handlers) MountEnvelopeIngest(r chi.Router) {
	r.Post("/outlets/{outletId}/table-sessions", h.replayOpenSession)
	r.Post("/outlets/{outletId}/table-sessions/", h.replayOpenSession)
	r.Post("/outlets/{outletId}/table-sessions/{sessionId}", h.replaySessionTransition)
	r.Post("/outlets/{outletId}/table-sessions/{sessionId}/", h.replaySessionTransition)
}

// MountEnvelopeRead registers the unwrapped GET side of the table_session
// surface: it returns the aggregate, not an envelope, and is HUMAN-
// authenticated (there is no protocol reason to gate a read behind a device
// credential). Each pattern is registered with and without a trailing slash
// for the same reason as MountEnvelopeIngest above.
func (h *Handlers) MountEnvelopeRead(r chi.Router) {
	r.Get("/outlets/{outletId}/table-sessions", h.listOpenSessions)
	r.Get("/outlets/{outletId}/table-sessions/", h.listOpenSessions)
	r.Get("/outlets/{outletId}/table-sessions/{sessionId}", h.getSession)
	r.Get("/outlets/{outletId}/table-sessions/{sessionId}/", h.getSession)
}

// envelopeWire mirrors contracts.SyncEnvelope (packages/contracts/go/sync.go)
// field-for-field, with payload kept as raw JSON so it can be decoded into
// the aggregate-specific shape only after the envelope itself is validated.
type envelopeWire struct {
	RecordID      string                  `json:"record_id"`
	TenantID      string                  `json:"tenant_id"`
	OutletID      string                  `json:"outlet_id"`
	DeviceID      string                  `json:"device_id"`
	AggregateType contracts.AggregateType `json:"aggregate_type"`
	Direction     contracts.SyncDirection `json:"direction"`
	CreatedAt     time.Time               `json:"created_at"`
	UpdatedAt     time.Time               `json:"updated_at"`
	Version       int                     `json:"version"`
	SyncStatus    contracts.SyncStatus    `json:"sync_status"`
	Payload       json.RawMessage         `json:"payload"`
}

// tableSessionPayload mirrors contracts.TableSession's JSON shape for
// decoding the envelope payload.
type tableSessionPayload struct {
	ID             string            `json:"id"`
	OutletID       string            `json:"outlet_id"`
	TableID        string            `json:"table_id"`
	State          TableSessionState `json:"state"`
	CurrentOrderID *string           `json:"current_order_id"`
	GuestCount     int               `json:"guest_count"`
	OpenedByUserID *string           `json:"opened_by_user_id"`
	OpenedAt       time.Time         `json:"opened_at"`
	ClosedAt       *time.Time        `json:"closed_at"`
	Version        int               `json:"version"`
	CreatedAt      time.Time         `json:"created_at"`
	UpdatedAt      time.Time         `json:"updated_at"`
	SchemaVersion  int               `json:"schema_version"`
}

// envelopeMismatchBody is the contracted EnvelopeRouteMismatch response
// shape (openapi.yaml components.responses.EnvelopeRouteMismatch).
type envelopeMismatchBody struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

func writeEnvelopeMismatch(w http.ResponseWriter, message string) {
	httpx.JSON(w, http.StatusUnprocessableEntity, envelopeMismatchBody{
		Code:    "envelope_route_mismatch",
		Message: message,
	})
}

// decodeTableSessionEnvelope decodes and validates the envelope for a
// table_session ingest route. It never coerces a mismatched aggregate_type
// or direction into what the route expects — a mismatch is reported to the
// caller via ok=false, mismatch=true so the handler can return 422. A
// malformed/undecodable body reports ok=false, mismatch=false so the handler
// returns 400.
//
// The required direction is read from contracts.AggregateAuthority, not
// hand-written here, so §50.1 stays encoded in exactly one place.
func decodeTableSessionEnvelope(r *http.Request) (env envelopeWire, payload tableSessionPayload, mismatchMsg string, err error) {
	if decErr := httpx.DecodeJSON(r, &env); decErr != nil {
		return envelopeWire{}, tableSessionPayload{}, "", decErr
	}

	requiredDirection, known := contracts.AggregateAuthority[contracts.AggregateTypeTableSession]
	if !known {
		// contracts.AggregateAuthority is asserted total by a contract drift
		// test; this branch exists only to fail closed if that ever regresses.
		return envelopeWire{}, tableSessionPayload{}, "table_session has no configured sync direction", nil
	}
	if env.AggregateType != contracts.AggregateTypeTableSession {
		return envelopeWire{}, tableSessionPayload{}, fmt.Sprintf(
			"expected aggregate_type %q for this route, got %q", contracts.AggregateTypeTableSession, env.AggregateType,
		), nil
	}
	if env.Direction != requiredDirection {
		return envelopeWire{}, tableSessionPayload{}, fmt.Sprintf(
			"expected direction %q for table_session, got %q", requiredDirection, env.Direction,
		), nil
	}

	dec := json.NewDecoder(bytes.NewReader(env.Payload))
	dec.DisallowUnknownFields()
	if decErr := dec.Decode(&payload); decErr != nil {
		return envelopeWire{}, tableSessionPayload{}, "", fmt.Errorf("%w: undecodable table_session payload", httpx.ErrInvalidInput)
	}
	return env, payload, "", nil
}

// requireDeviceOutlet resolves the caller's verified device credential
// (outlet.DeviceAuthenticate) and rejects a request whose route outletID
// does not match the outlet that credential resolves to. Per ADR-017's
// 0.4.3 amendment, outlet_id is never trusted from the route or the
// envelope on its own — it must agree with what the device credential
// resolves to.
func requireDeviceOutlet(r *http.Request, outletID string) error {
	principal, ok := outlet.DevicePrincipalFromContext(r.Context())
	if !ok {
		return httpx.ErrUnauthorized
	}
	if principal.OutletID != outletID {
		return fmt.Errorf("%w: this device is not enrolled at outlet %s", httpx.ErrForbidden, outletID)
	}
	return nil
}

func (h *Handlers) replayOpenSession(w http.ResponseWriter, r *http.Request) {
	outletID := chi.URLParam(r, "outletId")
	if err := requireDeviceOutlet(r, outletID); err != nil {
		httpx.Error(w, err)
		return
	}

	env, payload, mismatch, err := decodeTableSessionEnvelope(r)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	if mismatch != "" {
		writeEnvelopeMismatch(w, mismatch)
		return
	}
	if env.OutletID != "" && env.OutletID != outletID {
		httpx.Error(w, fmt.Errorf("%w: envelope outlet_id does not match route", httpx.ErrInvalidInput))
		return
	}
	if payload.State != "" && payload.State != contractsOccupiedState() {
		httpx.Error(w, fmt.Errorf("%w: an opened table_session must carry state OCCUPIED", httpx.ErrInvalidInput))
		return
	}

	sess, err := h.svc.OpenSession(r.Context(), OpenSessionInput{
		SessionID:      env.RecordID,
		Version:        env.Version,
		OutletID:       outletID,
		TableID:        payload.TableID,
		GuestCount:     payload.GuestCount,
		OpenedByUserID: payload.OpenedByUserID,
		OpenedAt:       payload.OpenedAt,
	})
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusCreated, sessionToWire(sess))
}

func (h *Handlers) replaySessionTransition(w http.ResponseWriter, r *http.Request) {
	outletID := chi.URLParam(r, "outletId")
	sessionID := chi.URLParam(r, "sessionId")
	if err := requireDeviceOutlet(r, outletID); err != nil {
		httpx.Error(w, err)
		return
	}

	env, payload, mismatch, err := decodeTableSessionEnvelope(r)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	if mismatch != "" {
		writeEnvelopeMismatch(w, mismatch)
		return
	}
	if env.RecordID != "" && env.RecordID != sessionID {
		httpx.Error(w, fmt.Errorf("%w: envelope record_id does not match route session id", httpx.ErrInvalidInput))
		return
	}

	sess, err := h.svc.ReplayTransition(r.Context(), outletID, sessionID, payload.State, payload.CurrentOrderID, env.Version)
	if err != nil {
		if errors.Is(err, ErrIllegalTransition) {
			httpx.JSON(w, http.StatusConflict, httpx.ErrorBody{Code: "conflict", Message: "illegal state transition"})
			return
		}
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, sessionToWire(sess))
}

func (h *Handlers) listOpenSessions(w http.ResponseWriter, r *http.Request) {
	outletID := chi.URLParam(r, "outletId")
	sessions, err := h.svc.ListOpenSessions(r.Context(), outletID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	out := make([]sessionWire, len(sessions))
	for i, s := range sessions {
		out[i] = sessionToWire(s)
	}
	httpx.JSON(w, http.StatusOK, out)
}

func (h *Handlers) getSession(w http.ResponseWriter, r *http.Request) {
	outletID := chi.URLParam(r, "outletId")
	sessionID := chi.URLParam(r, "sessionId")
	sess, err := h.svc.GetSession(r.Context(), outletID, sessionID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, sessionToWire(sess))
}

// sessionWire mirrors the TableSession openapi schema exactly.
type sessionWire struct {
	ID             string     `json:"id"`
	OutletID       string     `json:"outlet_id"`
	TableID        string     `json:"table_id"`
	State          string     `json:"state"`
	CurrentOrderID *string    `json:"current_order_id"`
	GuestCount     int        `json:"guest_count"`
	OpenedByUserID *string    `json:"opened_by_user_id"`
	OpenedAt       time.Time  `json:"opened_at"`
	ClosedAt       *time.Time `json:"closed_at"`
	Version        int        `json:"version"`
	CreatedAt      time.Time  `json:"created_at"`
	UpdatedAt      time.Time  `json:"updated_at"`
	SchemaVersion  int        `json:"schema_version"`
}

func sessionToWire(s TableSession) sessionWire {
	return sessionWire{
		ID:             s.ID,
		OutletID:       s.OutletID,
		TableID:        s.TableID,
		State:          string(s.State),
		CurrentOrderID: s.CurrentOrderID,
		GuestCount:     s.GuestCount,
		OpenedByUserID: s.OpenedByUserID,
		OpenedAt:       s.OpenedAt,
		ClosedAt:       s.ClosedAt,
		Version:        s.Version,
		CreatedAt:      s.CreatedAt,
		UpdatedAt:      s.UpdatedAt,
		SchemaVersion:  s.SchemaVersion,
	}
}

func contractsOccupiedState() TableSessionState {
	return contracts.TableSessionStateOccupied
}
