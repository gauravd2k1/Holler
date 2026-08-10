package kitchen

import (
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

// Handler wires the kitchen HTTP surface. It contains no business logic —
// every request is delegated to Service (CLAUDE.md §Coding rules).
type Handler struct {
	svc *Service
}

func NewHandler(svc *Service) *Handler {
	return &Handler{svc: svc}
}

// Mount registers this context's routes per
// packages/contracts/openapi/openapi.yaml: POST /orders/{id}/kots,
// POST /kots/{kotId}/status, GET/POST /stations, GET/POST /printers,
// PUT /stations/{stationId}/printers. PUT /menu/items/{itemId}/stations is
// registered by backend/internal/menu (ADR-014 task split) against this
// package's Service.
func (h *Handler) Mount(r chi.Router) {
	r.With(auth.RequirePermission(auth.PermissionOrderModify)).Post("/orders/{id}/kots", h.ingestKot)
	r.With(auth.RequirePermission(auth.PermissionOrderModify)).Post("/kots/{kotId}/status", h.ingestKotStatus)

	r.Get("/stations", h.listStations)
	r.With(auth.RequirePermission(auth.PermissionMenuManage)).Post("/stations", h.createStation)

	r.Get("/printers", h.listPrinters)
	r.With(auth.RequirePermission(auth.PermissionOutletManage)).Post("/printers", h.createPrinter)

	r.With(auth.RequirePermission(auth.PermissionOutletManage)).Put("/stations/{stationId}/printers", h.replaceStationPrinters)
}

// --- envelope plumbing, mirroring backend/internal/ordering/http.go --------

type envelopeWire struct {
	RecordID      string          `json:"record_id"`
	TenantID      string          `json:"tenant_id"`
	OutletID      string          `json:"outlet_id"`
	DeviceID      string          `json:"device_id"`
	AggregateType string          `json:"aggregate_type"`
	Direction     string          `json:"direction"`
	CreatedAt     string          `json:"created_at"`
	UpdatedAt     string          `json:"updated_at"`
	Version       int             `json:"version"`
	SyncStatus    string          `json:"sync_status"`
	Payload       json.RawMessage `json:"payload"`
}

func (w envelopeWire) toEnvelope() contracts.SyncEnvelope {
	return contracts.SyncEnvelope{
		RecordID:      w.RecordID,
		TenantID:      w.TenantID,
		OutletID:      w.OutletID,
		DeviceID:      w.DeviceID,
		AggregateType: contracts.AggregateType(w.AggregateType),
		Direction:     contracts.SyncDirection(w.Direction),
		Version:       w.Version,
		SyncStatus:    contracts.SyncStatus(w.SyncStatus),
	}
}

type envelopeRouteMismatchBody struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

func writeEnvelopeRouteMismatch(w http.ResponseWriter, err error) {
	httpx.JSON(w, http.StatusUnprocessableEntity, envelopeRouteMismatchBody{
		Code:    "envelope_route_mismatch",
		Message: err.Error(),
	})
}

// writeIngestError maps a Service error to the response the contract pins:
// ErrAuthorityViolation (aggregate_type/direction mismatch against the
// route, §50.1) is 422 EnvelopeRouteMismatch; every other error goes through
// the shared httpx.Error envelope.
func writeIngestError(w http.ResponseWriter, err error) {
	if errors.Is(err, ErrAuthorityViolation) {
		writeEnvelopeRouteMismatch(w, err)
		return
	}
	httpx.Error(w, err)
}

// decodeEnvelope reads the request body strictly as a SyncEnvelope: unknown
// fields are rejected so a bare, unwrapped Kot body is refused outright as
// 400 rather than silently half-parsed into a mostly-empty envelope.
func decodeEnvelope(r *http.Request) (contracts.SyncEnvelope, json.RawMessage, error) {
	var wire envelopeWire
	dec := json.NewDecoder(r.Body)
	dec.DisallowUnknownFields()
	if err := dec.Decode(&wire); err != nil {
		return contracts.SyncEnvelope{}, nil, httpx.ErrInvalidInput
	}
	if wire.RecordID == "" || wire.AggregateType == "" || wire.Direction == "" || len(wire.Payload) == 0 {
		return contracts.SyncEnvelope{}, nil, httpx.ErrInvalidInput
	}
	return wire.toEnvelope(), wire.Payload, nil
}

// --- KOT ingest --------------------------------------------------------

func kotToWire(k Kot) contracts.Kot {
	if k.Items == nil {
		k.Items = []KotTicketItem{}
	}
	return k
}

func (h *Handler) principalTenant(r *http.Request) (string, bool) {
	p, ok := auth.PrincipalFromContext(r.Context())
	if !ok {
		return "", false
	}
	return p.TenantID, true
}

func (h *Handler) ingestKot(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	orderID := chi.URLParam(r, "id")

	env, payload, err := decodeEnvelope(r)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	var kot contracts.Kot
	if err := json.Unmarshal(payload, &kot); err != nil {
		httpx.Error(w, httpx.ErrInvalidInput)
		return
	}
	if kot.OrderID == "" {
		kot.OrderID = orderID
	}
	if kot.OrderID != orderID {
		httpx.Error(w, fmt.Errorf("%w: payload order_id must match the route order id", httpx.ErrInvalidInput))
		return
	}

	stored, err := h.svc.IngestKot(r.Context(), tenantID, env, kot)
	if err != nil {
		writeIngestError(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, kotToWire(stored))
}

type kotStatusPayload struct {
	Status            string    `json:"status"`
	ChangedAt         time.Time `json:"changed_at"`
	ChangedByDeviceID string    `json:"changed_by_device_id"`
}

func (h *Handler) ingestKotStatus(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	kotID := chi.URLParam(r, "kotId")

	env, payload, err := decodeEnvelope(r)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	var body kotStatusPayload
	if err := json.Unmarshal(payload, &body); err != nil {
		httpx.Error(w, httpx.ErrInvalidInput)
		return
	}

	stored, err := h.svc.IngestKotStatus(r.Context(), tenantID, env, kotID, KotStatusTransition{
		Status:            KotStatus(body.Status),
		ChangedAt:         body.ChangedAt,
		ChangedByDeviceID: body.ChangedByDeviceID,
	})
	if err != nil {
		writeIngestError(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, kotToWire(stored))
}

// --- Station -------------------------------------------------------------

func stationToWire(s Station) contracts.Station {
	return s
}

func (h *Handler) listStations(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	outletID := r.URL.Query().Get("outlet_id")

	stations, err := h.svc.ListStations(r.Context(), tenantID, outletID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	out := make([]contracts.Station, len(stations))
	for i, s := range stations {
		out[i] = stationToWire(s)
	}
	httpx.JSON(w, http.StatusOK, out)
}

type createStationRequest struct {
	ID        string `json:"id"`
	OutletID  string `json:"outlet_id"`
	Code      string `json:"code"`
	Name      string `json:"name"`
	SortOrder int    `json:"sort_order"`
	IsActive  bool   `json:"is_active"`
}

func (h *Handler) createStation(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	var req createStationRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	st, err := h.svc.CreateStation(r.Context(), tenantID, NewStationInput{
		ID: req.ID, OutletID: req.OutletID, Code: req.Code, Name: req.Name,
		SortOrder: req.SortOrder, IsActive: req.IsActive,
	})
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusCreated, stationToWire(st))
}

// --- Printer ---------------------------------------------------------------

func printerToWire(p Printer) contracts.Printer {
	return p
}

func (h *Handler) listPrinters(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	outletID := r.URL.Query().Get("outlet_id")

	printers, err := h.svc.ListPrinters(r.Context(), tenantID, outletID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	out := make([]contracts.Printer, len(printers))
	for i, p := range printers {
		out[i] = printerToWire(p)
	}
	httpx.JSON(w, http.StatusOK, out)
}

type createPrinterRequest struct {
	ID             string `json:"id"`
	OutletID       string `json:"outlet_id"`
	Name           string `json:"name"`
	ConnectionKind string `json:"connection_kind"`
	Address        string `json:"address"`
	PaperWidthMM   int    `json:"paper_width_mm"`
	IsActive       bool   `json:"is_active"`
}

func (h *Handler) createPrinter(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	var req createPrinterRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	p, err := h.svc.CreatePrinter(r.Context(), tenantID, NewPrinterInput{
		ID: req.ID, OutletID: req.OutletID, Name: req.Name,
		ConnectionKind: PrinterConnectionKind(req.ConnectionKind),
		Address:        req.Address, PaperWidthMM: req.PaperWidthMM, IsActive: req.IsActive,
	})
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusCreated, printerToWire(p))
}

// --- Routing ---------------------------------------------------------------

type replacePrintersRequest struct {
	PrinterIDs []string `json:"printer_ids"`
}

func (h *Handler) replaceStationPrinters(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	stationID := chi.URLParam(r, "stationId")

	var req replacePrintersRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	out, err := h.svc.ReplaceStationPrinters(r.Context(), tenantID, stationID, req.PrinterIDs)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	wire := make([]contracts.StationPrinter, len(out))
	for i, sp := range out {
		wire[i] = sp
	}
	httpx.JSON(w, http.StatusOK, wire)
}
