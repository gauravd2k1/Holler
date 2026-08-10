package menu

import (
	"context"
	"net/http"

	"github.com/go-chi/chi/v5"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

// StationRouter is the narrow interface backend/internal/kitchen.Service
// satisfies for the item→station routing route. Milestone 2 (ADR-014) put
// this route's registration in backend/internal/menu — its path lives under
// /menu/items/{itemId}/... — while its business logic (station membership,
// config_version bump, audit) stays in backend/internal/kitchen next to the
// station/printer domain it routes into. menu depends only on this small
// interface, not on the kitchen package's full surface.
type StationRouter interface {
	ReplaceItemStations(ctx context.Context, tenantID, itemID string, stationIDs []string) ([]contracts.MenuItemStation, error)
}

// Handlers wires the menu HTTP surface onto a shared router. Endpoints match
// packages/contracts/openapi/openapi.yaml exactly: GET/POST /menu/categories
// and GET/POST /menu/items.
type Handlers struct {
	svc      *Service
	stations StationRouter
}

// NewHandlers wires the menu HTTP surface. stations may be nil only in tests
// that never exercise PUT /menu/items/{itemId}/stations.
func NewHandlers(svc *Service, stations StationRouter) *Handlers {
	return &Handlers{svc: svc, stations: stations}
}

// Mount registers the menu routes on r.
func (h *Handlers) Mount(r chi.Router) {
	r.Route("/menu", func(r chi.Router) {
		r.Get("/categories", h.listCategories)
		r.Post("/categories", h.createCategory)
		r.Get("/items", h.listItems)
		r.Post("/items", h.createItem)
		r.Post("/items/{itemId}/availability", h.setItemAvailability)
		r.With(auth.RequirePermission(auth.PermissionMenuManage)).Put("/items/{itemId}/stations", h.replaceItemStations)
	})
}

// replaceItemStations is ADR-014's item→station routing route (0.3.0):
// PUT, not POST — routing is a set, replaced wholesale, never merged
// (§50.1). The route lives here; the logic lives in backend/internal/kitchen
// behind the StationRouter interface above.
type replaceItemStationsRequest struct {
	StationIDs []string `json:"station_ids"`
}

func (h *Handlers) replaceItemStations(w http.ResponseWriter, r *http.Request) {
	if h.stations == nil {
		httpx.Error(w, httpx.ErrNotFound)
		return
	}
	principal, ok := auth.PrincipalFromContext(r.Context())
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	itemID := chi.URLParam(r, "itemId")

	var req replaceItemStationsRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}

	out, err := h.stations.ReplaceItemStations(r.Context(), principal.TenantID, itemID, req.StationIDs)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, out)
}

func (h *Handlers) listCategories(w http.ResponseWriter, r *http.Request) {
	outletID := r.URL.Query().Get("outlet_id")
	categories, err := h.svc.ListCategories(r.Context(), outletID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, categoriesToWire(categories))
}

type categoryWire struct {
	ID            string `json:"id"`
	OutletID      string `json:"outlet_id"`
	Name          string `json:"name"`
	SortOrder     int    `json:"sort_order"`
	ConfigVersion int    `json:"config_version"`
}

func categoryToWire(c Category) categoryWire {
	return categoryWire{
		ID:            c.ID,
		OutletID:      c.OutletID,
		Name:          c.Name,
		SortOrder:     c.SortOrder,
		ConfigVersion: c.ConfigVersion,
	}
}

func categoriesToWire(cs []Category) []categoryWire {
	out := make([]categoryWire, len(cs))
	for i, c := range cs {
		out[i] = categoryToWire(c)
	}
	return out
}

type createCategoryRequest struct {
	OutletID  string `json:"outlet_id"`
	Name      string `json:"name"`
	SortOrder int    `json:"sort_order"`
}

func (h *Handlers) createCategory(w http.ResponseWriter, r *http.Request) {
	var req createCategoryRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	c, err := h.svc.CreateCategory(r.Context(), NewCategoryInput{
		OutletID:  req.OutletID,
		Name:      req.Name,
		SortOrder: req.SortOrder,
	})
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusCreated, categoryToWire(c))
}

type itemWire struct {
	ID             string `json:"id"`
	OutletID       string `json:"outlet_id"`
	CategoryID     string `json:"category_id"`
	Name           string `json:"name"`
	BasePricePaise int64  `json:"base_price_paise"`
	IsAvailable    bool   `json:"is_available"`
	ConfigVersion  int    `json:"config_version"`
}

func itemToWire(i Item) itemWire {
	return itemWire{
		ID:             i.ID,
		OutletID:       i.OutletID,
		CategoryID:     i.CategoryID,
		Name:           i.Name,
		BasePricePaise: i.BasePricePaise,
		IsAvailable:    i.IsAvailable,
		ConfigVersion:  i.ConfigVersion,
	}
}

func itemsToWire(is []Item) []itemWire {
	out := make([]itemWire, len(is))
	for i, it := range is {
		out[i] = itemToWire(it)
	}
	return out
}

func (h *Handlers) listItems(w http.ResponseWriter, r *http.Request) {
	outletID := r.URL.Query().Get("outlet_id")
	items, err := h.svc.ListItems(r.Context(), outletID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, itemsToWire(items))
}

type createItemVariantRequest struct {
	Name            string `json:"name"`
	PriceDeltaPaise int64  `json:"price_delta_paise"`
}

type createItemModifierRequest struct {
	GroupName       string `json:"group_name"`
	OptionName      string `json:"option_name"`
	PriceDeltaPaise int64  `json:"price_delta_paise"`
	MinSelection    int    `json:"min_selection"`
	MaxSelection    int    `json:"max_selection"`
}

type createItemRequest struct {
	OutletID       string                      `json:"outlet_id"`
	CategoryID     string                      `json:"category_id"`
	Name           string                      `json:"name"`
	BasePricePaise int64                       `json:"base_price_paise"`
	Variants       []createItemVariantRequest  `json:"variants"`
	Modifiers      []createItemModifierRequest `json:"modifiers"`
}

type createItemResponse struct {
	itemWire
	Variants  []variantWire  `json:"variants,omitempty"`
	Modifiers []modifierWire `json:"modifiers,omitempty"`
}

type variantWire struct {
	ID              string `json:"id"`
	MenuItemID      string `json:"menu_item_id"`
	Name            string `json:"name"`
	PriceDeltaPaise int64  `json:"price_delta_paise"`
	ConfigVersion   int    `json:"config_version"`
}

type modifierWire struct {
	ID              string `json:"id"`
	MenuItemID      string `json:"menu_item_id"`
	GroupName       string `json:"group_name"`
	OptionName      string `json:"option_name"`
	PriceDeltaPaise int64  `json:"price_delta_paise"`
	MinSelection    int    `json:"min_selection"`
	MaxSelection    int    `json:"max_selection"`
	ConfigVersion   int    `json:"config_version"`
}

func (h *Handlers) createItem(w http.ResponseWriter, r *http.Request) {
	var req createItemRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}

	in := NewItemInput{
		OutletID:       req.OutletID,
		CategoryID:     req.CategoryID,
		Name:           req.Name,
		BasePricePaise: req.BasePricePaise,
	}
	for _, v := range req.Variants {
		in.Variants = append(in.Variants, NewVariantInput{Name: v.Name, PriceDeltaPaise: v.PriceDeltaPaise})
	}
	for _, m := range req.Modifiers {
		in.Modifiers = append(in.Modifiers, NewModifierInput{
			GroupName:       m.GroupName,
			OptionName:      m.OptionName,
			PriceDeltaPaise: m.PriceDeltaPaise,
			MinSelection:    m.MinSelection,
			MaxSelection:    m.MaxSelection,
		})
	}

	item, variants, modifiers, err := h.svc.CreateItem(r.Context(), in)
	if err != nil {
		httpx.Error(w, err)
		return
	}

	resp := createItemResponse{itemWire: itemToWire(item)}
	for _, v := range variants {
		resp.Variants = append(resp.Variants, variantWire{
			ID: v.ID, MenuItemID: v.MenuItemID, Name: v.Name,
			PriceDeltaPaise: v.PriceDeltaPaise, ConfigVersion: v.ConfigVersion,
		})
	}
	for _, m := range modifiers {
		resp.Modifiers = append(resp.Modifiers, modifierWire{
			ID: m.ID, MenuItemID: m.MenuItemID, GroupName: m.GroupName, OptionName: m.OptionName,
			PriceDeltaPaise: m.PriceDeltaPaise, MinSelection: m.MinSelection, MaxSelection: m.MaxSelection,
			ConfigVersion: m.ConfigVersion,
		})
	}
	httpx.JSON(w, http.StatusCreated, resp)
}

type setAvailabilityRequest struct {
	IsAvailable bool `json:"is_available"`
}

func (h *Handlers) setItemAvailability(w http.ResponseWriter, r *http.Request) {
	itemID := chi.URLParam(r, "itemId")
	outletID := r.URL.Query().Get("outlet_id")

	var req setAvailabilityRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}

	item, err := h.svc.SetItemAvailability(r.Context(), outletID, itemID, req.IsAvailable)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, itemToWire(item))
}
