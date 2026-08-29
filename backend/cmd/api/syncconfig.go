package main

import (
	"context"
	"net/http"
	"strconv"

	contracts "github.com/holler/contracts"

	"github.com/holler/backend/internal/compliance"
	"github.com/holler/backend/internal/inventory"
	"github.com/holler/backend/internal/kitchen"
	"github.com/holler/backend/internal/menu"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/procurement"
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
// ListVariantsSince/ListModifiersSince close the M4 T4 delivery-fix gap:
// menu_item_variant and menu_item_modifier never reached GET /sync/config
// before this — the most load-bearing instance of the class, since
// recipe.menu_item_variant_id is NOT NULL and recipes now sync (T4).
type menuConfigProvider interface {
	ListCategories(ctx context.Context, outletID string) ([]menu.Category, error)
	ListItems(ctx context.Context, outletID string) ([]menu.Item, error)
	ListVariantsSince(ctx context.Context, outletID string, sinceVersion int) ([]menu.Variant, error)
	ListModifiersSince(ctx context.Context, outletID string, sinceVersion int) ([]menu.Modifier, error)
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

// inventoryConfigProvider is the minimal seam onto backend/internal/
// inventory (T4, ADR-018): inventory_item, item_unit_conversion, recipe,
// recipe_ingredient and modifier_ingredient_delta, pre-filtered by
// since_version and pre-scoped to the caller's tenant
// (inventory.Service.SyncConfigBundle calls requireOutletInTenant itself,
// mirroring kitchen.Service.SyncConfigBundle).
type inventoryConfigProvider interface {
	SyncConfigBundle(ctx context.Context, tenantID, outletID string, sinceVersion int) (inventory.ConfigBundle, error)
}

// procurementConfigProvider is the minimal seam onto
// backend/internal/procurement (T1, ADR-019): supplier, supplier_item,
// purchase_order and purchase_order_line, pre-filtered by since_version and
// pre-scoped to the caller's tenant (procurement.Service.SyncConfigBundle
// calls requireOutletInTenant itself, mirroring
// inventory.Service.SyncConfigBundle).
//
// The edge holds ALL of it READ-ONLY. A purchase order reaches an outlet so a
// receiving screen can prefill from it; the edge never writes one back, and it
// never approves one — role.po_approval_limit_paise is Postgres-only and there
// is no role table in SQLite at all (ADR-019 §7).
type procurementConfigProvider interface {
	SyncConfigBundle(ctx context.Context, tenantID, outletID string, sinceVersion int) (procurement.ConfigBundle, error)
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

// edgeDeviceCredentialProvider is the minimal seam onto
// backend/internal/outlet.DeviceService (T13, ADR-017 0.4.3 amendment):
// device_credential rows enrolled at outletID, with credential_hash carried
// verbatim so a KDS can verify a LAN handshake against its local cache with
// the uplink down. outlet.DeviceService.ListEdgeDeviceCredentials is the
// ONLY method in this backend that returns a device credential hash —
// mirrors edgeUserCacheProvider's identical carve-out for password_hash/
// pin_hash immediately above. Revoked and expired credentials are NEVER
// filtered out by this seam (see the method's own doc comment) — the edge
// learns a credential is dead by syncing it, not by its absence.
type edgeDeviceCredentialProvider interface {
	ListEdgeDeviceCredentials(ctx context.Context, tenantID, outletID string, sinceVersion int) ([]contracts.EdgeDeviceCredential, error)
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
	// TaxProfileID/HSNSAC: filed gap closed (M4 T4 delivery-fix follow-up —
	// found by TestSyncConfigGuard_EveryCloudAuthoritativeColumnIsWiredOrExempted).
	// contracts.MenuItem has carried both since 0.4.2/0.4.5; nothing wrote
	// them through to this wire shape until now. hsn_sac is nil until
	// backend/internal/menu grows a write path for it — no route sets it
	// today — but a NULL hsn_sac here is now genuinely "not configured
	// yet", not "the sync bundle forgot to carry it": the edge already
	// refuses to issue an invoice on a NULL hsn_sac line, which is the
	// correct failure now that the column travels at all.
	TaxProfileID  *string `json:"tax_profile_id"`
	HSNSAC        *string `json:"hsn_sac"`
	ConfigVersion int     `json:"config_version"`
}

// variantConfigWire mirrors contracts.MenuItemVariant plus IsDefault, which
// contracts 0.5.6's Go mirror does not carry even though
// postgres/0014_menu_default_variant.sql added the column at 0.5.0 (a
// contract gap, not fixed here — packages/contracts is read-only to this
// task). Local wire type for the same reason itemConfigWire is one:
// backend/internal/menu's Variant is its own domain struct.
type variantConfigWire struct {
	ID              string `json:"id"`
	MenuItemID      string `json:"menu_item_id"`
	Name            string `json:"name"`
	PriceDeltaPaise int64  `json:"price_delta_paise"`
	IsDefault       bool   `json:"is_default"`
	ConfigVersion   int    `json:"config_version"`
	SchemaVersion   int    `json:"schema_version"`
}

// modifierConfigWire mirrors contracts.MenuItemModifier field for field —
// unlike Variant, Modifier has no undelivered contract field, so this exists
// only because backend/internal/menu.Modifier is its own domain struct
// (no schema_version), the same itemConfigWire/tableConfigWire reason.
type modifierConfigWire struct {
	ID              string `json:"id"`
	MenuItemID      string `json:"menu_item_id"`
	GroupName       string `json:"group_name"`
	OptionName      string `json:"option_name"`
	PriceDeltaPaise int64  `json:"price_delta_paise"`
	MinSelection    int    `json:"min_selection"`
	MaxSelection    int    `json:"max_selection"`
	ConfigVersion   int    `json:"config_version"`
	SchemaVersion   int    `json:"schema_version"`
}

// syncConfigResponse is packages/contracts/openapi/openapi.yaml's
// GET /sync/config 200 response, all nine required fields.
type syncConfigResponse struct {
	ConfigVersion int `json:"config_version"`
	// DayStartTime is the outlet's business-day boundary (ADR-018 §9.2),
	// local HH:MM. Read at the edge (business_date computation) and, until
	// this M4 T4 delivery fix, written by nothing: the write path existed as
	// a bare column with no route and no place in this bundle.
	DayStartTime    string                         `json:"day_start_time"`
	Users           []contracts.EdgeUserCacheEntry `json:"users"`
	Tables          []tableConfigWire              `json:"tables"`
	Categories      []categoryConfigWire           `json:"categories"`
	Items           []itemConfigWire               `json:"items"`
	Stations        []contracts.Station            `json:"stations"`
	ItemStations    []contracts.MenuItemStation    `json:"item_stations"`
	Printers        []contracts.Printer            `json:"printers"`
	StationPrinters []contracts.StationPrinter     `json:"station_printers"`
	// PrinterRoles closes a shipped gap (M4 T4 delivery-fix task): the
	// printer_role table has existed since 0.4.7 in both stores and in
	// Go/TS, but this bundle never carried it, so a cloud-synced outlet had
	// zero printer roles and print_invoice failed by name at every one.
	PrinterRoles        []contracts.PrinterRole          `json:"printer_roles"`
	ComplianceVersions  []contracts.ComplianceVersion    `json:"compliance_versions"`
	TaxProfiles         []contracts.TaxProfile           `json:"tax_profiles"`
	TaxRules            []contracts.TaxRule              `json:"tax_rules"`
	InvoiceSeries       []contracts.InvoiceSeries        `json:"invoice_series"`
	DiscountDefinitions []contracts.DiscountDefinition   `json:"discount_definitions"`
	FiscalProfile       *contracts.OutletFiscalProfile   `json:"fiscal_profile"`
	DeviceCredentials   []contracts.EdgeDeviceCredential `json:"device_credentials"`
	// Milestone 4 (ADR-018, T4): inventory items, their unit conversions,
	// recipes, recipe ingredients and modifier ingredient deltas.
	InventoryItems           []contracts.InventoryItem           `json:"inventory_items"`
	ItemUnitConversions      []contracts.ItemUnitConversion      `json:"item_unit_conversions"`
	Recipes                  []contracts.Recipe                  `json:"recipes"`
	RecipeIngredients        []contracts.RecipeIngredient        `json:"recipe_ingredients"`
	ModifierIngredientDeltas []contracts.ModifierIngredientDelta `json:"modifier_ingredient_deltas"`
	// MenuItemVariants/MenuItemModifiers: M4 T4 delivery-fix follow-up, the
	// most load-bearing instance of the class this task's guard exists to
	// catch. recipe.menu_item_variant_id is NOT NULL and recipes now sync
	// (T4), so an outlet that never received its own variants had every
	// recipe pointing at a row it did not have — every order line failed to
	// stamp a variant, and every sale gapped NO_VARIANT.
	MenuItemVariants  []variantConfigWire  `json:"menu_item_variants"`
	MenuItemModifiers []modifierConfigWire `json:"menu_item_modifiers"`

	// Milestone 5 (ADR-019, T1): suppliers, their price lists, purchase orders
	// and their lines. All CLOUD_TO_EDGE config, all read-only at the edge.
	//
	// PurchaseOrders carry NO RECEIPT STATE and never will. Receipt progress is
	// derived on both sides and the two derivations legitimately differ (the
	// edge sees its own grn_line rows, the cloud sees every outlet's), so there
	// is deliberately no progress field on this bundle for an edge to trust.
	Suppliers          []contracts.Supplier          `json:"suppliers"`
	SupplierItems      []contracts.SupplierItem      `json:"supplier_items"`
	PurchaseOrders     []contracts.PurchaseOrder     `json:"purchase_orders"`
	PurchaseOrderLines []contracts.PurchaseOrderLine `json:"purchase_order_lines"`
}

// syncConfigHandler assembles the composite bundle. It never runs SQL
// itself — every field comes from an owning context's already-exported,
// already-tenant/outlet-scoped Service method.
type syncConfigHandler struct {
	outlets           outletConfigProvider
	menu              menuConfigProvider
	tables            tablesConfigProvider
	kitchen           kitchenConfigProvider
	compliance        complianceConfigProvider
	inventory         inventoryConfigProvider
	procurement       procurementConfigProvider
	users             edgeUserCacheProvider
	deviceCredentials edgeDeviceCredentialProvider
}

func newSyncConfigHandler(outlets outletConfigProvider, menuSvc menuConfigProvider, tablesSvc tablesConfigProvider, kitchenSvc kitchenConfigProvider, complianceSvc complianceConfigProvider, inventorySvc inventoryConfigProvider, procurementSvc procurementConfigProvider, usersSvc edgeUserCacheProvider, deviceCredentialsSvc edgeDeviceCredentialProvider) *syncConfigHandler {
	return &syncConfigHandler{
		outlets: outlets, menu: menuSvc, tables: tablesSvc, kitchen: kitchenSvc, compliance: complianceSvc,
		inventory: inventorySvc, procurement: procurementSvc, users: usersSvc, deviceCredentials: deviceCredentialsSvc,
	}
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
	variants, err := h.menu.ListVariantsSince(r.Context(), outletID, sinceVersion)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	modifiers, err := h.menu.ListModifiersSince(r.Context(), outletID, sinceVersion)
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
	deviceCredentials, err := h.deviceCredentials.ListEdgeDeviceCredentials(r.Context(), tenantID, outletID, sinceVersion)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	inventoryBundle, err := h.inventory.SyncConfigBundle(r.Context(), tenantID, outletID, sinceVersion)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	procurementBundle, err := h.procurement.SyncConfigBundle(r.Context(), tenantID, outletID, sinceVersion)
	if err != nil {
		httpx.Error(w, err)
		return
	}

	resp := syncConfigResponse{
		ConfigVersion:       o.ConfigVersion,
		DayStartTime:        o.DayStartTime,
		Users:               users,
		Tables:              filterTables(tbls, sinceVersion),
		Categories:          filterCategories(cats, sinceVersion),
		Items:               filterItems(items, sinceVersion),
		Stations:            bundle.Stations,
		ItemStations:        bundle.ItemStations,
		Printers:            bundle.Printers,
		StationPrinters:     bundle.StationPrinters,
		PrinterRoles:        bundle.PrinterRoles,
		ComplianceVersions:  complianceBundle.ComplianceVersions,
		TaxProfiles:         complianceBundle.TaxProfiles,
		TaxRules:            complianceBundle.TaxRules,
		InvoiceSeries:       complianceBundle.InvoiceSeries,
		DiscountDefinitions: complianceBundle.DiscountDefinitions,
		FiscalProfile:       complianceBundle.FiscalProfile,
		DeviceCredentials:   deviceCredentials,

		InventoryItems:           inventoryBundle.InventoryItems,
		ItemUnitConversions:      inventoryBundle.ItemUnitConversions,
		Recipes:                  inventoryBundle.Recipes,
		RecipeIngredients:        inventoryBundle.RecipeIngredients,
		ModifierIngredientDeltas: inventoryBundle.ModifierIngredientDeltas,

		MenuItemVariants:  variantsToWire(variants),
		MenuItemModifiers: modifiersToWire(modifiers),

		Suppliers:          procurementBundle.Suppliers,
		SupplierItems:      procurementBundle.SupplierItems,
		PurchaseOrders:     procurementBundle.PurchaseOrders,
		PurchaseOrderLines: procurementBundle.PurchaseOrderLines,
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
	if resp.PrinterRoles == nil {
		resp.PrinterRoles = []contracts.PrinterRole{}
	}
	if resp.DeviceCredentials == nil {
		resp.DeviceCredentials = []contracts.EdgeDeviceCredential{}
	}
	if resp.InventoryItems == nil {
		resp.InventoryItems = []contracts.InventoryItem{}
	}
	if resp.ItemUnitConversions == nil {
		resp.ItemUnitConversions = []contracts.ItemUnitConversion{}
	}
	if resp.Recipes == nil {
		resp.Recipes = []contracts.Recipe{}
	}
	if resp.RecipeIngredients == nil {
		resp.RecipeIngredients = []contracts.RecipeIngredient{}
	}
	if resp.ModifierIngredientDeltas == nil {
		resp.ModifierIngredientDeltas = []contracts.ModifierIngredientDelta{}
	}
	if resp.Suppliers == nil {
		resp.Suppliers = []contracts.Supplier{}
	}
	if resp.SupplierItems == nil {
		resp.SupplierItems = []contracts.SupplierItem{}
	}
	if resp.PurchaseOrders == nil {
		resp.PurchaseOrders = []contracts.PurchaseOrder{}
	}
	if resp.PurchaseOrderLines == nil {
		resp.PurchaseOrderLines = []contracts.PurchaseOrderLine{}
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
			BasePricePaise: i.BasePricePaise, IsAvailable: i.IsAvailable,
			TaxProfileID: i.TaxProfileID, HSNSAC: i.HSNSAC, ConfigVersion: i.ConfigVersion,
		})
	}
	return out
}

// variantsToWire/modifiersToWire adapt backend/internal/menu's own domain
// structs to the wire shape, the itemConfigWire/tableConfigWire precedent.
// ListVariantsSince/ListModifiersSince are already since_version-filtered at
// the DB (the kitchen.StationPrintersSince shape), so unlike filterItems
// above there is no client-side filter to apply here.
func variantsToWire(in []menu.Variant) []variantConfigWire {
	out := make([]variantConfigWire, 0, len(in))
	for _, v := range in {
		out = append(out, variantConfigWire{
			ID: v.ID, MenuItemID: v.MenuItemID, Name: v.Name,
			PriceDeltaPaise: v.PriceDeltaPaise, IsDefault: v.IsDefault,
			ConfigVersion: v.ConfigVersion, SchemaVersion: 1,
		})
	}
	return out
}

func modifiersToWire(in []menu.Modifier) []modifierConfigWire {
	out := make([]modifierConfigWire, 0, len(in))
	for _, m := range in {
		out = append(out, modifierConfigWire{
			ID: m.ID, MenuItemID: m.MenuItemID, GroupName: m.GroupName, OptionName: m.OptionName,
			PriceDeltaPaise: m.PriceDeltaPaise, MinSelection: m.MinSelection, MaxSelection: m.MaxSelection,
			ConfigVersion: m.ConfigVersion, SchemaVersion: 1,
		})
	}
	return out
}
