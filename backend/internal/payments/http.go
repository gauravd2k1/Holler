package payments

import (
	"encoding/json"
	"errors"
	"net/http"

	"github.com/go-chi/chi/v5"
	contracts "github.com/holler/contracts"

	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/httpx"
)

// Handler wires the payments HTTP surface: POST /invoices, POST /payments,
// POST /cash-shifts, POST /cash-shifts/{shiftId}/close. Every route here is
// edge->cloud replay by definition (ADR-016 §1) — there is no
// human-authored write path for any of these three aggregates, so unlike
// backend/internal/ordering/kitchen/tables this package has nothing to split
// into a separate human-auth Mount: everything Mount registers belongs
// behind outlet.DeviceAuthenticate (ADR-017's 0.4.3 amendment).
type Handler struct {
	svc *Service
}

func NewHandler(svc *Service) *Handler {
	return &Handler{svc: svc}
}

// Mount registers this context's routes. The caller (backend/cmd/api) is
// responsible for mounting this under outlet.DeviceAuthenticate — this
// package does not gate itself so it stays agnostic of the composition
// root's routing, mirroring how ordering/kitchen/tables leave grouping to
// backend/cmd/api/main.go.
func (h *Handler) Mount(r chi.Router) {
	r.Post("/invoices", h.ingestInvoice)
	r.Post("/payments", h.ingestPayment)
	r.Post("/cash-shifts", h.ingestCashShift)
	r.Post("/cash-shifts/{shiftId}/close", h.closeCashShift)
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

func writeMismatch(w http.ResponseWriter, code, message string) {
	httpx.JSON(w, http.StatusUnprocessableEntity, envelopeRouteMismatchBody{Code: code, Message: message})
}

// writeIngestError maps a Service error to the response the contract pins:
// ErrAuthorityViolation is 422 EnvelopeRouteMismatch; ErrRoundingViolation
// and ErrShiftNotAccounted are 422 naming the violated rule rather than a
// raw driver constraint error (ADR-016); every other error goes through the
// shared httpx.Error envelope (400/404/409/...).
func writeIngestError(w http.ResponseWriter, err error) {
	switch {
	case errors.Is(err, ErrAuthorityViolation):
		writeMismatch(w, "envelope_route_mismatch", err.Error())
	case errors.Is(err, ErrRoundingViolation):
		writeMismatch(w, "invoice_rounding_violation", "invoice does not satisfy the ADR-016 rounding policy: components must sum to grand_total_paise through round_off_paise, |round_off_paise| must not exceed 50, and grand_total_paise must settle in whole rupees")
	case errors.Is(err, ErrShiftNotAccounted):
		writeMismatch(w, "cash_shift_not_fully_accounted", "a CLOSED cash_shift must carry closed_at, expected_cash_paise, actual_cash_paise and variance_paise, and a variance_reason whenever variance_paise is non-zero (§39)")
	default:
		httpx.Error(w, err)
	}
}

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

// deviceCaller resolves the tenant_id/outlet_id the ingest caller is
// authorized to act as. Per ADR-017's 0.4.3 amendment these come from the
// verified device credential outlet.DeviceAuthenticate resolved — never from
// the envelope, the request body, or a query/path parameter.
func deviceCaller(r *http.Request) (tenantID, outletID string, ok bool) {
	p, ok := outlet.DevicePrincipalFromContext(r.Context())
	if !ok {
		return "", "", false
	}
	return p.TenantID, p.OutletID, true
}

// --- invoice ---------------------------------------------------------------

func (h *Handler) ingestInvoice(w http.ResponseWriter, r *http.Request) {
	tenantID, outletID, ok := deviceCaller(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	env, payload, err := decodeEnvelope(r)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	var inv Invoice
	if err := json.Unmarshal(payload, &inv); err != nil {
		httpx.Error(w, httpx.ErrInvalidInput)
		return
	}

	stored, err := h.svc.IngestInvoice(r.Context(), tenantID, outletID, env, inv)
	if err != nil {
		writeIngestError(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, stored)
}

// --- payment -----------------------------------------------------------

func (h *Handler) ingestPayment(w http.ResponseWriter, r *http.Request) {
	tenantID, outletID, ok := deviceCaller(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	env, payload, err := decodeEnvelope(r)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	var p Payment
	if err := json.Unmarshal(payload, &p); err != nil {
		httpx.Error(w, httpx.ErrInvalidInput)
		return
	}

	stored, err := h.svc.IngestPayment(r.Context(), tenantID, outletID, env, p)
	if err != nil {
		writeIngestError(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, stored)
}

// --- cash_shift --------------------------------------------------------

func (h *Handler) ingestCashShift(w http.ResponseWriter, r *http.Request) {
	tenantID, outletID, ok := deviceCaller(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	env, payload, err := decodeEnvelope(r)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	var s CashShift
	if err := json.Unmarshal(payload, &s); err != nil {
		httpx.Error(w, httpx.ErrInvalidInput)
		return
	}

	stored, err := h.svc.IngestCashShift(r.Context(), tenantID, outletID, env, s)
	if err != nil {
		writeIngestError(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, stored)
}

func (h *Handler) closeCashShift(w http.ResponseWriter, r *http.Request) {
	tenantID, outletID, ok := deviceCaller(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	shiftID := chi.URLParam(r, "shiftId")

	env, payload, err := decodeEnvelope(r)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	var s CashShift
	if err := json.Unmarshal(payload, &s); err != nil {
		httpx.Error(w, httpx.ErrInvalidInput)
		return
	}

	stored, err := h.svc.CloseCashShift(r.Context(), tenantID, outletID, env, shiftID, s)
	if err != nil {
		writeIngestError(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, stored)
}
