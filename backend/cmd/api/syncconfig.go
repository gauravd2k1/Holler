package main

import (
	"context"
	"net/http"
	"strconv"

	contracts "github.com/holler/contracts"

	"github.com/holler/backend/internal/compliance"
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
// never reimplements another context's query (task instruction), including
// `users`, populated via auth.Service.ListEdgeUserCache (ADR-015).
//
// This handler's ServeHTTP is the ONE place in backend/ that puts
// password_hash / pin_hash on the wire. No other route, event payload, log
// line or audit value may carry either field.

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

// complianceConfigProvider is the minimal seam onto backend/internal/
// compliance (T13): compliance_version, tax_profile (+ its tax_rule
// children), invoice_series, discount_definition and the outlet's current
// outlet_fiscal_profile, pre-filtered by since_version and pre-scoped to the
// caller's tenant (compliance.Service.SyncConfigBundle calls
// requireOutletInTenant itself, mirroring kitchen.Service.SyncConfigBundle).
type complianceConfigProvider interface {
	SyncConfigBundle(ctx context.Context, tenantID, outletID string, sinceVersion int) (compliance.ConfigBundle, error)
}

// edgeUserCacheProvider is the minimal seam onto backend/internal/auth this
// handler needs: users eligible to log in at outletID, with password_hash /
// pin_hash carried verbatim and permissions already flattened server-side
// (ADR-015). auth.Service.ListEdgeUserCache is the ONLY method in this
// backend that returns either hash — this handler's ServeHTTP is the ONLY
// place that puts one on the wire.
type edgeUserCacheProvider interface {
	ListEdgeUserCache(ctx context.Context, tenantID, outletID string, sinceVersion int) ([]contracts.EdgeUserCacheEntry, error)
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
	ConfigVersion       int                            `json:"config_version"`
	Users               []contracts.EdgeUserCacheEntry `json:"users"`
	Tables              []tableConfigWire              `json:"tables"`
	Categories          []categoryConfigWire           `json:"categories"`
	Items               []itemConfigWire               `json:"items"`
	Stations            []contracts.Station            `json:"stations"`
	ItemStations        []contracts.MenuItemStation    `json:"item_stations"`
	Printers            []contracts.Printer            `json:"printers"`
	StationPrinters     []contracts.StationPrinter     `json:"station_printers"`
	ComplianceVersions  []contracts.ComplianceVersion  `json:"compliance_versions"`
	TaxProfiles         []contracts.TaxProfile         `json:"tax_profiles"`
	TaxRules            []contracts.TaxRule            `json:"tax_rules"`
	InvoiceSeries       []contracts.InvoiceSeries      `json:"invoice_series"`
	DiscountDefinitions []contracts.DiscountDefinition `json:"discount_definitions"`
	FiscalProfile       *contracts.OutletFiscalProfile `json:"fiscal_profile"`
}

// syncConfigHandler assembles the composite bundle. It never runs SQL
// itself — every field comes from an owning context's already-exported,
// already-tenant/outlet-scoped Service method.
type syncConfigHandler struct {
	outlets    outletConfigProvider
	menu       menuConfigProvider
	tables     tablesConfigProvider
	kitchen    kitchenConfigProvider
	compliance complianceConfigProvider
	users      edgeUserCacheProvider
}

func newSyncConfigHandler(outlets outletConfigProvider, menuSvc menuConfigProvider, tablesSvc tablesConfigProvider, kitchenSvc kitchenConfigProvider, complianceSvc complianceConfigProvider, usersSvc edgeUserCacheProvider) *syncConfigHandler {
	return &syncConfigHandler{outlets: outlets, menu: menuSvc, tables: tablesSvc, kitchen: kitchenSvc, compliance: complianceSvc, users: usersSvc}
}

// ServeHTTP requires a verified DevicePrincipal in context (ADR-017 §2):
// GET /sync/config is the one route that carries Argon2id password and PIN
// hashes, and its cloud-side gate is now outlet.DeviceAuthenticate, not a
// human bearer token — see backend/cmd/api/main.go's router wiring. tenantID
// and outletID come from the verified device credential, never from the
// request: a device presenting outlet_id=X in the query string cannot pull
// another outlet's users just by typing a different id in.
func (h *syncConfigHandler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	devicePrincipal, ok := outlet.DevicePrincipalFromContext(r.Context())
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}

	outletID := r.URL.Query().Get("outlet_id")
	if outletID == "" {
		httpx.Error(w, httpx.ErrInvalidInput)
		return
	}
	// A caller-supplied outlet_id that does not match the enrolled device's
	// own outlet is treated exactly like every other cross-tenant lookup in
	// this codebase: httpx.ErrNotFound, never a 200 with another outlet's
	// data and never a 403 that confirms the id exists.
	if outletID != devicePrincipal.OutletID {
		httpx.Error(w, httpx.ErrNotFound)
		return
	}
	tenantID := devicePrincipal.TenantID

	sinceVersionRaw := r.URL.Query().Get("since_version")
	sinceVersion, err := strconv.Atoi(sinceVersionRaw)
	if err != nil || sinceVersion < 0 {
		httpx.Error(w, httpx.ErrInvalidInput)
		return
	}

	// GetOutlet's config_version becomes the bundle's top-level
	// config_version. tenantID here is the device credential's tenant, not
	// anything the caller supplied.
	o, err := h.outlets.GetOutlet(r.Context(), outlet.Principal{TenantID: tenantID}, outletID)
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
	bundle, err := h.kitchen.SyncConfigBundle(r.Context(), tenantID, outletID, sinceVersion)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	complianceBundle, err := h.compliance.SyncConfigBundle(r.Context(), tenantID, outletID, sinceVersion)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	users, err := h.users.ListEdgeUserCache(r.Context(), tenantID, outletID, sinceVersion)
	if err != nil {
		httpx.Error(w, err)
		return
	}

	resp := syncConfigResponse{
		ConfigVersion:       o.ConfigVersion,
		Users:               users,
		Tables:              filterTables(tbls, sinceVersion),
		Categories:          filterCategories(cats, sinceVersion),
		Items:               filterItems(items, sinceVersion),
		Stations:            bundle.Stations,
		ItemStations:        bundle.ItemStations,
		Printers:            bundle.Printers,
		StationPrinters:     bundle.StationPrinters,
		ComplianceVersions:  complianceBundle.ComplianceVersions,
		TaxProfiles:         complianceBundle.TaxProfiles,
		TaxRules:            complianceBundle.TaxRules,
		InvoiceSeries:       complianceBundle.InvoiceSeries,
		DiscountDefinitions: complianceBundle.DiscountDefinitions,
		FiscalProfile:       complianceBundle.FiscalProfile,
	}
	if resp.Users == nil {
		resp.Users = []contracts.EdgeUserCacheEntry{}
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
