package tables

import (
	"net/http"

	"github.com/go-chi/chi/v5"

	"github.com/holler/backend/internal/platform/httpx"
)

// Handlers wires the tables HTTP surface onto a shared router. Endpoints
// match packages/contracts/openapi/openapi.yaml exactly: GET/POST
// /outlets/{outletId}/tables for the RestaurantTable config path, and (as of
// contracts 0.2.1 / ADR-011 addendum) the envelope-wrapped
// /outlets/{outletId}/table-sessions[/{sessionId}] routes for the
// TableSession replay path — see http_envelope.go.
type Handlers struct {
	svc *Service
}

func NewHandlers(svc *Service) *Handlers {
	return &Handlers{svc: svc}
}

// Mount registers every tables route — both RestaurantTable config
// endpoints and the TableSession envelope-ingest endpoints — on r.
func (h *Handlers) Mount(r chi.Router) {
	r.Route("/outlets/{outletId}/tables", func(r chi.Router) {
		r.Get("/", h.listTables)
		r.Post("/", h.createTable)
	})
	h.MountEnvelopeIngest(r)
}

type tableWire struct {
	ID            string `json:"id"`
	OutletID      string `json:"outlet_id"`
	Section       string `json:"section"`
	Label         string `json:"label"`
	SeatCount     int    `json:"seat_count"`
	IsActive      bool   `json:"is_active"`
	ConfigVersion int    `json:"config_version"`
	SchemaVersion int    `json:"schema_version"`
}

func toWire(t RestaurantTable) tableWire {
	return tableWire{
		ID:            t.ID,
		OutletID:      t.OutletID,
		Section:       t.Section,
		Label:         t.Label,
		SeatCount:     t.SeatCount,
		IsActive:      t.IsActive,
		ConfigVersion: t.ConfigVersion,
		SchemaVersion: t.SchemaVersion,
	}
}

func toWireList(ts []RestaurantTable) []tableWire {
	out := make([]tableWire, len(ts))
	for i, t := range ts {
		out[i] = toWire(t)
	}
	return out
}

func (h *Handlers) listTables(w http.ResponseWriter, r *http.Request) {
	outletID := chi.URLParam(r, "outletId")
	tables, err := h.svc.ListTables(r.Context(), outletID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, toWireList(tables))
}

// createTableRequest accepts the RestaurantTable wire shape. outlet_id comes
// from the path, and id/is_active/config_version/schema_version are
// server-assigned, but a client that echoes the full schema (as the openapi
// RequestBody suggests) must not be rejected as "unknown field" for
// supplying them.
type createTableRequest struct {
	ID            string `json:"id,omitempty"`
	OutletID      string `json:"outlet_id,omitempty"`
	Section       string `json:"section"`
	Label         string `json:"label"`
	SeatCount     int    `json:"seat_count"`
	IsActive      *bool  `json:"is_active,omitempty"`
	ConfigVersion *int   `json:"config_version,omitempty"`
	SchemaVersion *int   `json:"schema_version,omitempty"`
}

func (h *Handlers) createTable(w http.ResponseWriter, r *http.Request) {
	outletID := chi.URLParam(r, "outletId")

	var req createTableRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}

	t, err := h.svc.CreateTable(r.Context(), NewTableInput{
		OutletID:  outletID,
		Section:   req.Section,
		Label:     req.Label,
		SeatCount: req.SeatCount,
	})
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusCreated, toWire(t))
}
