package ordering

import (
	"encoding/json"
	"errors"
	"net/http"

	"github.com/go-chi/chi/v5"
	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

// Handler wires the ordering HTTP surface. It contains no business logic —
// every request is delegated to Service (CLAUDE.md §Coding rules).
type Handler struct {
	svc *Service
}

func NewHandler(svc *Service) *Handler {
	return &Handler{svc: svc}
}

// Mount registers this context's routes per
// packages/contracts/openapi/openapi.yaml: POST /orders, GET /orders,
// GET /orders/{id}, POST /orders/{id}/items, POST /orders/{id}/send-to-kitchen,
// POST /orders/{id}/cancel.
func (h *Handler) Mount(r chi.Router) {
	r.With(auth.RequirePermission(auth.PermissionOrderCreate)).Post("/orders", h.createOrder)
	r.Get("/orders", h.listOrders)
	r.Get("/orders/{id}", h.getOrder)
	r.With(auth.RequirePermission(auth.PermissionOrderModify)).Post("/orders/{id}/items", h.appendItem)
	r.With(auth.RequirePermission(auth.PermissionOrderModify)).Post("/orders/{id}/send-to-kitchen", h.sendToKitchen)
	r.With(auth.RequirePermission(auth.PermissionOrderCancel)).Post("/orders/{id}/cancel", h.cancelOrder)
}

// envelopeWire is the wire shape of contracts.SyncEnvelope with Payload kept
// as raw JSON so handlers can decode it into the right payload type per
// aggregate — Go's json package cannot do that generically through
// interface{}.
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

// envelopeRouteMismatchBody is the wire shape of
// packages/contracts/openapi/openapi.yaml's EnvelopeRouteMismatch response.
type envelopeRouteMismatchBody struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

// writeEnvelopeRouteMismatch writes the contracted 422 response for an
// envelope whose aggregate_type or direction does not match the route
// (packages/contracts/openapi/openapi.yaml EnvelopeRouteMismatch). This
// status is not one of platform/httpx's shared sentinels (that package is
// out of ordering's scope), so ordering writes it directly here rather than
// through httpx.Error.
func writeEnvelopeRouteMismatch(w http.ResponseWriter, err error) {
	httpx.JSON(w, http.StatusUnprocessableEntity, envelopeRouteMismatchBody{
		Code:    "envelope_route_mismatch",
		Message: err.Error(),
	})
}

// writeIngestError maps a Service error to the response the contract
// pins: ErrAuthorityViolation (aggregate_type/direction mismatch against
// the route, §50.1) is 422 EnvelopeRouteMismatch; every other error goes
// through the shared httpx.Error envelope (400 for malformed/missing
// input, 404, 409, ...).
func writeIngestError(w http.ResponseWriter, err error) {
	if errors.Is(err, ErrAuthorityViolation) {
		writeEnvelopeRouteMismatch(w, err)
		return
	}
	httpx.Error(w, err)
}

// decodeEnvelope reads the request body strictly as a SyncEnvelope: unknown
// fields are rejected (DisallowUnknownFields) so a bare, unwrapped
// CanonicalOrder/OrderItem body — the pre-0.2.1 contract shape — is refused
// outright as 400 rather than silently half-parsed into a mostly-empty
// envelope. The envelope-defining fields (record_id, aggregate_type,
// direction, payload) must also be present.
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

func toOrderResponse(o StoredOrder) contracts.CanonicalOrder {
	order := o.Order
	if order.Items == nil {
		order.Items = []contracts.OrderItem{}
	}
	return order
}

func (h *Handler) createOrder(w http.ResponseWriter, r *http.Request) {
	principal, ok := auth.PrincipalFromContext(r.Context())
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}

	env, payload, err := decodeEnvelope(r)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	var order contracts.CanonicalOrder
	if err := json.Unmarshal(payload, &order); err != nil {
		httpx.Error(w, httpx.ErrInvalidInput)
		return
	}

	stored, err := h.svc.IngestOrder(r.Context(), principal.TenantID, env, order)
	if err != nil {
		writeIngestError(w, err)
		return
	}
	httpx.JSON(w, http.StatusCreated, toOrderResponse(stored))
}

func (h *Handler) getOrder(w http.ResponseWriter, r *http.Request) {
	principal, ok := auth.PrincipalFromContext(r.Context())
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	orderID := chi.URLParam(r, "id")

	stored, err := h.svc.GetOrder(r.Context(), principal.TenantID, orderID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, toOrderResponse(stored))
}

func (h *Handler) listOrders(w http.ResponseWriter, r *http.Request) {
	principal, ok := auth.PrincipalFromContext(r.Context())
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	outletID := r.URL.Query().Get("outlet_id")

	orders, err := h.svc.ListOrders(r.Context(), principal.TenantID, outletID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	resp := make([]contracts.CanonicalOrder, 0, len(orders))
	for _, o := range orders {
		resp = append(resp, toOrderResponse(o))
	}
	httpx.JSON(w, http.StatusOK, resp)
}

func (h *Handler) appendItem(w http.ResponseWriter, r *http.Request) {
	principal, ok := auth.PrincipalFromContext(r.Context())
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
	var item contracts.OrderItem
	if err := json.Unmarshal(payload, &item); err != nil {
		httpx.Error(w, httpx.ErrInvalidInput)
		return
	}

	stored, err := h.svc.AppendItem(r.Context(), principal.TenantID, env, orderID, item)
	if err != nil {
		writeIngestError(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, toOrderResponse(stored))
}

func (h *Handler) sendToKitchen(w http.ResponseWriter, r *http.Request) {
	principal, ok := auth.PrincipalFromContext(r.Context())
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	orderID := chi.URLParam(r, "id")

	env, _, err := decodeEnvelope(r)
	if err != nil {
		httpx.Error(w, err)
		return
	}

	stored, err := h.svc.SendToKitchen(r.Context(), principal.TenantID, env, orderID)
	if err != nil {
		writeIngestError(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, map[string]interface{}{
		"order": toOrderResponse(stored),
		"kots":  []interface{}{}, // KOT generation is Milestone 2.
	})
}

type cancelPayload struct {
	Reason string `json:"reason"`
}

func (h *Handler) cancelOrder(w http.ResponseWriter, r *http.Request) {
	principal, ok := auth.PrincipalFromContext(r.Context())
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
	var body cancelPayload
	if len(payload) > 0 {
		if err := json.Unmarshal(payload, &body); err != nil {
			httpx.Error(w, httpx.ErrInvalidInput)
			return
		}
	}

	stored, err := h.svc.Cancel(r.Context(), principal.TenantID, env, orderID, body.Reason)
	if err != nil {
		writeIngestError(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, toOrderResponse(stored))
}
