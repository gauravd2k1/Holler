package procurement

import (
	"encoding/json"
	"errors"
	"net/http"

	"github.com/go-chi/chi/v5"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

// Handler wires the procurement HTTP surface. It contains no business logic —
// every request is delegated to Service (CLAUDE.md §Coding rules).
type Handler struct {
	svc *Service
}

func NewHandler(svc *Service) *Handler {
	return &Handler{svc: svc}
}

// Mount registers this context's HUMAN-authenticated routes per
// packages/contracts/openapi/openapi.yaml. All of them are CONFIG,
// cloud→edge, ordinary unwrapped writes — envelopes are the edge→cloud replay
// pattern and appear on no config route.
//
// EVERY PERMISSION THIS PACKAGE NAMES IS ENFORCED HERE. procurement.manage
// gates the two config writes and the supplier-accounts routes;
// procurement.approve gates the approve route as middleware AND is re-checked
// inside Service.ApprovePurchaseOrder so the refusal can carry the §64
// message. A permission with no check is a permission that does not exist.
func (h *Handler) Mount(r chi.Router) {
	r.With(auth.RequirePermission(PermissionManage)).
		Post("/procurement/suppliers", h.createSupplier)
	r.With(auth.RequirePermission(PermissionManage)).
		Post("/procurement/purchase-orders", h.createPurchaseOrder)

	r.With(auth.RequirePermission(PermissionApprove)).
		Post("/procurement/purchase-orders/{purchaseOrderId}/approve", h.approvePurchaseOrder)

	r.With(auth.RequirePermission(PermissionManage)).
		Get("/procurement/purchase-orders/{purchaseOrderId}", h.getPurchaseOrder)

	// supplier_invoice / supplier_credit: CREATE AND LIST ONLY in M5. There is
	// deliberately no status-transition, application or settlement route —
	// those are M7 (ADR-019 §8).
	r.With(auth.RequirePermission(PermissionManage)).
		Post("/procurement/supplier-invoices", h.createSupplierInvoice)
	r.With(auth.RequirePermission(PermissionManage)).
		Get("/procurement/supplier-invoices", h.listSupplierInvoices)
	r.With(auth.RequirePermission(PermissionManage)).
		Post("/procurement/supplier-credits", h.createSupplierCredit)
	r.With(auth.RequirePermission(PermissionManage)).
		Get("/procurement/supplier-credits", h.listSupplierCredits)
}

// MountIngest registers the three edge→cloud replay routes. The caller is
// always an enrolled device, never a browser (ADR-017 0.4.3), mirroring
// backend/internal/inventory.MountIngest.
func (h *Handler) MountIngest(r chi.Router) {
	r.Post("/procurement/goods-receipts", h.ingestGoodsReceipt)
	r.Post("/procurement/purchase-returns", h.ingestPurchaseReturn)
	r.Post("/procurement/stock-transfers-out", h.ingestStockTransferOut)
}

func (h *Handler) principal(r *http.Request) (auth.AuthenticatedPrincipal, bool) {
	return auth.PrincipalFromContext(r.Context())
}

func deviceCaller(r *http.Request) (tenantID, outletID string, ok bool) {
	p, ok := outlet.DevicePrincipalFromContext(r.Context())
	if !ok {
		return "", "", false
	}
	return p.TenantID, p.OutletID, true
}

// --- envelope plumbing, mirroring backend/internal/inventory/http.go --------

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

type errorEnvelopeBody struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

func writeEnvelopeRouteMismatch(w http.ResponseWriter, err error) {
	httpx.JSON(w, http.StatusUnprocessableEntity, errorEnvelopeBody{
		Code:    "envelope_route_mismatch",
		Message: err.Error(),
	})
}

func writeIngestError(w http.ResponseWriter, err error) {
	if errors.Is(err, ErrAuthorityViolation) {
		writeEnvelopeRouteMismatch(w, err)
		return
	}
	httpx.Error(w, err)
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

func requireEnvelopeOutletMatch(envOutletID, callerOutletID string) error {
	if envOutletID != "" && envOutletID != callerOutletID {
		return errors.New("envelope outlet_id does not match the authenticated device's outlet")
	}
	return nil
}

// writeConfigError maps the config-write errors that need a status other than
// httpx.Error's default. ErrDimensionMismatch is a distinguished 422 per the
// OpenAPI route summary — a mismatch is rejected, never converted.
func writeConfigError(w http.ResponseWriter, err error) {
	if errors.Is(err, ErrDimensionMismatch) {
		httpx.JSON(w, http.StatusUnprocessableEntity, errorEnvelopeBody{
			Code:    "dimension_mismatch",
			Message: err.Error(),
		})
		return
	}
	httpx.Error(w, err)
}

// --- POST /procurement/suppliers -------------------------------------------

type createSupplierRequest struct {
	Supplier Supplier       `json:"supplier"`
	Items    []SupplierItem `json:"items"`
}

type createSupplierResponse struct {
	Supplier
	Items []SupplierItem `json:"items"`
}

func (h *Handler) createSupplier(w http.ResponseWriter, r *http.Request) {
	p, ok := h.principal(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	var req createSupplierRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	// NOTE: quantity_dimension is passed through EXACTLY AS THE AUTHOR SENT
	// IT. This handler never fills it from the referenced inventory_item — an
	// auto-fill would make the service's comparison x == x and the guard could
	// never fire (ADR-019 §6).
	sup, items, err := h.svc.CreateSupplier(r.Context(), p.TenantID, NewSupplierInput{
		Supplier: req.Supplier, Items: req.Items,
	})
	if err != nil {
		writeConfigError(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, createSupplierResponse{Supplier: sup, Items: items})
}

// --- POST /procurement/purchase-orders --------------------------------------

type createPurchaseOrderRequest struct {
	PurchaseOrder PurchaseOrder `json:"purchase_order"`
}

func (h *Handler) createPurchaseOrder(w http.ResponseWriter, r *http.Request) {
	p, ok := h.principal(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	var req createPurchaseOrderRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	po, err := h.svc.CreatePurchaseOrder(r.Context(), p.TenantID, NewPurchaseOrderInput{PurchaseOrder: req.PurchaseOrder})
	if err != nil {
		writeConfigError(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, po)
}

// --- POST /procurement/purchase-orders/{id}/approve -------------------------

// approvalRefusalBody is the 403 shape pinned by the OpenAPI route: code,
// message, total_paise and a NULLABLE limit_paise.
//
// limit_paise null means "this caller may not approve any amount" — the NULL
// role limit, which is never read as unlimited. Null and 0 are different facts
// here and a consumer must not collapse them.
type approvalRefusalBody struct {
	Code         string   `json:"code"`
	Message      string   `json:"message"`
	TotalPaise   int64    `json:"total_paise"`
	LimitPaise   *int64   `json:"limit_paise"`
	Alternatives []string `json:"can_be_approved_by_roles"`
}

func (h *Handler) approvePurchaseOrder(w http.ResponseWriter, r *http.Request) {
	p, ok := h.principal(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	po, err := h.svc.ApprovePurchaseOrder(r.Context(), p, chi.URLParam(r, "purchaseOrderId"))
	if err != nil {
		var refusal *ApprovalRefusal
		if errors.As(err, &refusal) {
			httpx.JSON(w, http.StatusForbidden, approvalRefusalBody{
				Code:         refusal.Code,
				Message:      refusal.Error(),
				TotalPaise:   refusal.TotalPaise,
				LimitPaise:   refusal.LimitPaise,
				Alternatives: refusal.Alternatives,
			})
			return
		}
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, po)
}

// --- GET /procurement/purchase-orders/{id} ----------------------------------

// purchaseOrderDetailResponse carries the order AND its derived receipt
// progress, with the progress LABELLED by scope.
//
// The label is not decoration. The edge computes the same shape over its own
// grn_line rows and gets a different number, legitimately; an admin screen
// that shows this figure without saying it is cloud-wide invites someone to
// read the difference as drift and "fix" it by making one side authoritative,
// which is the second writer keeping receipt state off purchase_order exists
// to prevent (ADR-019 §4).
type purchaseOrderDetailResponse struct {
	PurchaseOrder   PurchaseOrder   `json:"purchase_order"`
	ReceiptProgress ReceiptProgress `json:"receipt_progress"`
}

func (h *Handler) getPurchaseOrder(w http.ResponseWriter, r *http.Request) {
	p, ok := h.principal(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	poID := chi.URLParam(r, "purchaseOrderId")
	po, err := h.svc.GetPurchaseOrder(r.Context(), p.TenantID, poID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	progress, err := h.svc.PurchaseOrderReceiptProgress(r.Context(), p.TenantID, poID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, purchaseOrderDetailResponse{PurchaseOrder: po, ReceiptProgress: progress})
}

// --- supplier_invoice / supplier_credit -------------------------------------

func (h *Handler) createSupplierInvoice(w http.ResponseWriter, r *http.Request) {
	p, ok := h.principal(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	var inv SupplierInvoice
	if err := httpx.DecodeJSON(r, &inv); err != nil {
		httpx.Error(w, err)
		return
	}
	stored, err := h.svc.CreateSupplierInvoice(r.Context(), p.TenantID, inv)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, stored)
}

func (h *Handler) listSupplierInvoices(w http.ResponseWriter, r *http.Request) {
	p, ok := h.principal(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	invoices, err := h.svc.ListSupplierInvoices(r.Context(), p.TenantID, r.URL.Query().Get("outlet_id"))
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, invoices)
}

func (h *Handler) createSupplierCredit(w http.ResponseWriter, r *http.Request) {
	p, ok := h.principal(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	var credit SupplierCredit
	if err := httpx.DecodeJSON(r, &credit); err != nil {
		httpx.Error(w, err)
		return
	}
	stored, err := h.svc.CreateSupplierCredit(r.Context(), p.TenantID, credit)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, stored)
}

func (h *Handler) listSupplierCredits(w http.ResponseWriter, r *http.Request) {
	p, ok := h.principal(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	credits, err := h.svc.ListSupplierCredits(r.Context(), p.TenantID, r.URL.Query().Get("outlet_id"))
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, credits)
}

// --- POST /procurement/goods-receipts ---------------------------------------

// ingestGoodsReceipt pins a SET of two aggregate types on one route —
// goods_receipt_note and grn_gap — for the reason
// /inventory/ledger-entries pins two: a gap records what could not be matched
// ABOUT THIS RECEIPT and belongs beside the receipt it explains. A gap
// arriving by a different path could not be joined to it.
//
// ANYTHING OUTSIDE THE SET IS 422 FROM THE DEFAULT ARM, never coerced onto one
// of the two shapes (§50.1).
func (h *Handler) ingestGoodsReceipt(w http.ResponseWriter, r *http.Request) {
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
	if err := requireEnvelopeOutletMatch(env.OutletID, outletID); err != nil {
		httpx.Error(w, httpx.ErrForbidden)
		return
	}

	switch env.AggregateType {
	case contracts.AggregateTypeGoodsReceiptNote:
		var grn GoodsReceiptNote
		if err := json.Unmarshal(payload, &grn); err != nil {
			httpx.Error(w, httpx.ErrInvalidInput)
			return
		}
		stored, err := h.svc.IngestGoodsReceiptNote(r.Context(), tenantID, env, grn)
		if err != nil {
			writeIngestError(w, err)
			return
		}
		httpx.JSON(w, http.StatusCreated, stored)

	case contracts.AggregateTypeGrnGap:
		var gap GrnGap
		if err := json.Unmarshal(payload, &gap); err != nil {
			httpx.Error(w, httpx.ErrInvalidInput)
			return
		}
		stored, err := h.svc.IngestGrnGap(r.Context(), tenantID, env, gap)
		if err != nil {
			writeIngestError(w, err)
			return
		}
		httpx.JSON(w, http.StatusCreated, stored)

	default:
		writeEnvelopeRouteMismatch(w, ErrAuthorityViolation)
	}
}

// --- POST /procurement/purchase-returns -------------------------------------

func (h *Handler) ingestPurchaseReturn(w http.ResponseWriter, r *http.Request) {
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
	if err := requireEnvelopeOutletMatch(env.OutletID, outletID); err != nil {
		httpx.Error(w, httpx.ErrForbidden)
		return
	}
	var ret PurchaseReturn
	if err := json.Unmarshal(payload, &ret); err != nil {
		httpx.Error(w, httpx.ErrInvalidInput)
		return
	}
	stored, err := h.svc.IngestPurchaseReturn(r.Context(), tenantID, env, ret)
	if err != nil {
		writeIngestError(w, err)
		return
	}
	httpx.JSON(w, http.StatusCreated, stored)
}

// --- POST /procurement/stock-transfers-out ----------------------------------

func (h *Handler) ingestStockTransferOut(w http.ResponseWriter, r *http.Request) {
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
	if err := requireEnvelopeOutletMatch(env.OutletID, outletID); err != nil {
		httpx.Error(w, httpx.ErrForbidden)
		return
	}
	var transfer StockTransferOut
	if err := json.Unmarshal(payload, &transfer); err != nil {
		httpx.Error(w, httpx.ErrInvalidInput)
		return
	}
	stored, err := h.svc.IngestStockTransferOut(r.Context(), tenantID, env, transfer)
	if err != nil {
		writeIngestError(w, err)
		return
	}
	httpx.JSON(w, http.StatusCreated, stored)
}
