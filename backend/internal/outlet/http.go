package outlet

import (
	"net/http"

	"github.com/go-chi/chi/v5"
	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/platform/httpx"
)

// outletResponse mirrors the Outlet schema in
// packages/contracts/openapi/openapi.yaml exactly (id, brand_id, name,
// timezone, config_version).
type outletResponse struct {
	ID            string `json:"id"`
	BrandID       string `json:"brand_id"`
	Name          string `json:"name"`
	Timezone      string `json:"timezone"`
	DayStartTime  string `json:"day_start_time"`
	ConfigVersion int    `json:"config_version"`
}

func toResponse(o Outlet) outletResponse {
	return outletResponse{
		ID:            o.ID,
		BrandID:       o.BrandID,
		Name:          o.Name,
		Timezone:      o.Timezone,
		DayStartTime:  o.DayStartTime,
		ConfigVersion: o.ConfigVersion,
	}
}

// Handler wires the outlet HTTP surface. It contains no business logic —
// every request is delegated to Service (CLAUDE.md §Coding rules).
type Handler struct {
	svc *Service
}

func NewHandler(svc *Service) *Handler {
	return &Handler{svc: svc}
}

// Mount registers this context's routes onto r, per
// packages/contracts/openapi/openapi.yaml: GET /outlets.
//
// PUT /outlets/{outletId}/day-start-time is NOT yet in openapi.yaml — it is
// the write path ADR-018 §9.2 describes but never specified as a route
// (contract gap, reported rather than worked around per this task's brief).
// It follows the existing config-write shape (gated on outlet.manage, bumps
// config_version) so a future contracts amendment can pin it verbatim.
func (h *Handler) Mount(r chi.Router) {
	r.Get("/outlets", h.listOutlets)
	r.With(auth.RequirePermission(auth.PermissionOutletManage)).
		Put("/outlets/{outletId}/day-start-time", h.setDayStartTime)
}

type setDayStartTimeRequest struct {
	DayStartTime string `json:"day_start_time"`
}

func (h *Handler) setDayStartTime(w http.ResponseWriter, r *http.Request) {
	principal, ok := PrincipalFromContext(r.Context())
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	outletID := chi.URLParam(r, "outletId")

	var req setDayStartTimeRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	o, err := h.svc.SetDayStartTime(r.Context(), principal, outletID, req.DayStartTime)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, toResponse(o))
}

func (h *Handler) listOutlets(w http.ResponseWriter, r *http.Request) {
	principal, ok := PrincipalFromContext(r.Context())
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}

	outlets, err := h.svc.ListOutlets(r.Context(), principal)
	if err != nil {
		httpx.Error(w, err)
		return
	}

	resp := make([]outletResponse, 0, len(outlets))
	for _, o := range outlets {
		resp = append(resp, toResponse(o))
	}
	httpx.JSON(w, http.StatusOK, resp)
}
