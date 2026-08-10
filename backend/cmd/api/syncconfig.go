package main

import (
	"context"
	"net/http"
	"strconv"
	"time"

	contracts "github.com/holler/contracts"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/kitchen"
	"github.com/holler/backend/internal/menu"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/tables"
)

// GET /sync/config is the one composite, cross-context route this repo
// defines (packages/contracts/openapi/openapi.yaml). It exists only in the
// composition root because no single bounded context owns all nine required
// fields: config_version, users, tables, categories, items, stations,
// item_stations, printers, station_printers.
//
// Every field this handler assembles comes from an existing exported method
// on the owning context's Service — this file never runs its own SQL and
// never reimplements another context's query (task instruction). The one
// field it CANNOT assemble is documented on edgeUserCacheEntryWire below.

// outletConfigProvider is the minimal seam onto backend/internal/outlet this
// handler needs: resolving outlet_id against the caller's tenant (so a
// request for another tenant's outlet_id gets the same
// httpx.ErrNotFound every other route in this codebase returns for
// cross-tenant access) and reading its current config_version, which this
// route surfaces at the top level of the bundle.
type outletConfigProvider interface {
	GetOutlet(ctx context.Context, principal outlet.Principal, outletID string) (outlet.Outlet, error)
}

// menuConfigProvider is the minimal seam onto backend/internal/menu.
type menuConfigProvider interface {
	ListCategories(ctx context.Context, outletID string) ([]menu.Category, error)
	ListItems(ctx context.Context, outletID string) ([]menu.Item, error)
}

// tablesConfigProvider is the minimal seam onto backend/internal/tables.
type tablesConfigProvider interface {
	ListTables(ctx context.Context, outletID string) ([]tables.RestaurantTable, error)
}

// kitchenConfigProvider is the minimal seam onto backend/internal/kitchen.
// kitchen.Service already exposes exactly the bundle this route needs,
// pre-filtered by since_version and pre-scoped to the caller's tenant
// (kitchen.Service.SyncConfigBundle calls requireOutletInTenant itself).
type kitchenConfigProvider interface {
	SyncConfigBundle(ctx context.Context, tenantID, outletID string, sinceVersion int) (kitchen.ConfigBundle, error)
}

// edgeUserCacheEntryWire mirrors packages/contracts/openapi/openapi.yaml's
// EdgeUserCacheEntry schema field-for-field. There is deliberately no Go
// mirror of this schema in packages/contracts/go (see
// packages/contracts/go/identity.go's package doc: "no struct here carries
// credential material") and backend/internal/auth exports no method that
// returns a password_hash outside its own login/refresh flow (the hash lives
// only in that package's unexported credentialRow). This handler therefore
// cannot populate `users` without either a contracts addition or a new
// exported auth method — both out of this task's owned directories
// (backend/cmd/api, backend/internal/platform). See the "users" gap in this
// task's final report; `users` is always `[]` until that lands.
type edgeUserCacheEntryWire struct {
	ID            string    `json:"id"`
	TenantID      string    `json:"tenant_id"`
	OutletID      string    `json:"outlet_id"`
	Email         string    `json:"email"`
	FullName      string    `json:"full_name"`
	PasswordHash  string    `json:"password_hash"`
	PinHash       *string   `json:"pin_hash,omitempty"`
	IsActive      bool      `json:"is_active"`
	Permissions   []string  `json:"permissions"`
	ConfigVersion int       `json:"config_version"`
	UpdatedAt     time.Time `json:"updated_at"`
}

// tableConfigWire, categoryConfigWire and itemConfigWire mirror the
// RestaurantTable/MenuCategory/MenuItem openapi schemas exactly. They exist
// here rather than being imported because backend/internal/tables already
// aliases the contract type directly (no gap) while backend/internal/menu's
// Category/Item are its own domain structs (no schema_version field, unlike
// the contracts.RestaurantTable path) — this file adapts them to the wire
// shape without touching menu's internal logic.
type tableConfigWire struct {
	ID            string `json:"id"`
	OutletID      string `json:"outlet_id"`
	Section       string `json:"section"`
	Label         string `json:"label"`
	SeatCount     int    `json:"seat_count"`
	IsActive      bool   `json:"is_active"`
	ConfigVersion int    `json:"config_version"`
	SchemaVersion int    `json:"schema_version"`
}

type categoryConfigWire struct {
	ID            string `json:"id"`
	OutletID      string `json:"outlet_id"`
	Name          string `json:"name"`
	SortOrder     int    `json:"sort_order"`
	ConfigVersion int    `json:"config_version"`
}

type itemConfigWire struct {
	ID             string `json:"id"`
	OutletID       string `json:"outlet_id"`
	CategoryID     string `json:"category_id"`
	Name           string `json:"name"`
	BasePricePaise int64  `json:"base_price_paise"`
	IsAvailable    bool   `json:"is_available"`
	ConfigVersion  int    `json:"config_version"`
}

// syncConfigResponse is packages/contracts/openapi/openapi.yaml's
// GET /sync/config 200 response, all nine required fields.
type syncConfigResponse struct {
	ConfigVersion   int                         `json:"config_version"`
	Users           []edgeUserCacheEntryWire    `json:"users"`
	Tables          []tableConfigWire           `json:"tables"`
	Categories      []categoryConfigWire        `json:"categories"`
	Items           []itemConfigWire            `json:"items"`
	Stations        []contracts.Station         `json:"stations"`
	ItemStations    []contracts.MenuItemStation `json:"item_stations"`
	Printers        []contracts.Printer         `json:"printers"`
	StationPrinters []contracts.StationPrinter  `json:"station_printers"`
}

// syncConfigHandler assembles the composite bundle. It never runs SQL
// itself — every field comes from an owning context's already-exported,
// already-tenant/outlet-scoped Service method.
type syncConfigHandler struct {
	outlets outletConfigProvider
	menu    menuConfigProvider
	tables  tablesConfigProvider
	kitchen kitchenConfigProvider
}

func newSyncConfigHandler(outlets outletConfigProvider, menuSvc menuConfigProvider, tablesSvc tablesConfigProvider, kitchenSvc kitchenConfigProvider) *syncConfigHandler {
	return &syncConfigHandler{outlets: outlets, menu: menuSvc, tables: tablesSvc, kitchen: kitchenSvc}
}

func (h *syncConfigHandler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	principal, ok := auth.PrincipalFromContext(r.Context())
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}

	outletID := r.URL.Query().Get("outlet_id")
	if outletID == "" {
		httpx.Error(w, httpx.ErrInvalidInput)
		return
	}
	sinceVersionRaw := r.URL.Query().Get("since_version")
	sinceVersion, err := strconv.Atoi(sinceVersionRaw)
	if err != nil || sinceVersion < 0 {
		httpx.Error(w, httpx.ErrInvalidInput)
		return
	}

	// GetOutlet is the tenant-scoping check for the whole route: it returns
	// httpx.ErrNotFound for an outlet_id that exists but belongs to another
	// tenant, exactly like every other route in this codebase, and its
	// config_version becomes the bundle's top-level config_version.
	o, err := h.outlets.GetOutlet(r.Context(), outlet.Principal{UserID: principal.UserID, TenantID: principal.TenantID}, outletID)
	if err != nil {
		httpx.Error(w, err)
		return
	}

	tbls, err := h.tables.ListTables(r.Context(), outletID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	cats, err := h.menu.ListCategories(r.Context(), outletID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	items, err := h.menu.ListItems(r.Context(), outletID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	bundle, err := h.kitchen.SyncConfigBundle(r.Context(), principal.TenantID, outletID, sinceVersion)
	if err != nil {
		httpx.Error(w, err)
		return
	}

	resp := syncConfigResponse{
		ConfigVersion:   o.ConfigVersion,
		Users:           []edgeUserCacheEntryWire{},
		Tables:          filterTables(tbls, sinceVersion),
		Categories:      filterCategories(cats, sinceVersion),
		Items:           filterItems(items, sinceVersion),
		Stations:        bundle.Stations,
		ItemStations:    bundle.ItemStations,
		Printers:        bundle.Printers,
		StationPrinters: bundle.StationPrinters,
	}
	if resp.Stations == nil {
		resp.Stations = []contracts.Station{}
	}
	if resp.ItemStations == nil {
		resp.ItemStations = []contracts.MenuItemStation{}
	}
	if resp.Printers == nil {
		resp.Printers = []contracts.Printer{}
	}
	if resp.StationPrinters == nil {
		resp.StationPrinters = []contracts.StationPrinter{}
	}

	httpx.JSON(w, http.StatusOK, resp)
}

// filterTables/filterCategories/filterItems apply "only newer than
// since_version" (openapi.yaml summary) to the full outlet lists
// ListTables/ListCategories/ListItems return today — those methods have no
// since_version parameter of their own (unlike
// kitchen.Repository.StationsSince and friends), so filtering the already
// tenant/outlet-scoped result here is a plain slice filter on a field the
// owning context already returns, not a reimplementation of its query.
func filterTables(in []tables.RestaurantTable, sinceVersion int) []tableConfigWire {
	out := make([]tableConfigWire, 0, len(in))
	for _, t := range in {
		if t.ConfigVersion <= sinceVersion {
			continue
		}
		out = append(out, tableConfigWire{
			ID: t.ID, OutletID: t.OutletID, Section: t.Section, Label: t.Label,
			SeatCount: t.SeatCount, IsActive: t.IsActive, ConfigVersion: t.ConfigVersion,
			SchemaVersion: t.SchemaVersion,
		})
	}
	return out
}

func filterCategories(in []menu.Category, sinceVersion int) []categoryConfigWire {
	out := make([]categoryConfigWire, 0, len(in))
	for _, c := range in {
		if c.ConfigVersion <= sinceVersion {
			continue
		}
		out = append(out, categoryConfigWire{
			ID: c.ID, OutletID: c.OutletID, Name: c.Name, SortOrder: c.SortOrder, ConfigVersion: c.ConfigVersion,
		})
	}
	return out
}

func filterItems(in []menu.Item, sinceVersion int) []itemConfigWire {
	out := make([]itemConfigWire, 0, len(in))
	for _, i := range in {
		if i.ConfigVersion <= sinceVersion {
			continue
		}
		out = append(out, itemConfigWire{
			ID: i.ID, OutletID: i.OutletID, CategoryID: i.CategoryID, Name: i.Name,
			BasePricePaise: i.BasePricePaise, IsAvailable: i.IsAvailable, ConfigVersion: i.ConfigVersion,
		})
	}
	return out
}
