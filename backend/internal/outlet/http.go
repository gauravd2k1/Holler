package outlet

import (
	"net/http"

	"github.com/go-chi/chi/v5"
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
	ConfigVersion int    `json:"config_version"`
}

func toResponse(o Outlet) outletResponse {
	return outletResponse{
		ID:            o.ID,
		BrandID:       o.BrandID,
		Name:          o.Name,
		Timezone:      o.Timezone,
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
func (h *Handler) Mount(r chi.Router) {
	r.Get("/outlets", h.listOutlets)
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
