package inventory

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

// Handler wires the inventory HTTP surface. It contains no business logic —
// every request is delegated to Service (CLAUDE.md §Coding rules).
type Handler struct {
	svc *Service
}

func NewHandler(svc *Service) *Handler {
	return &Handler{svc: svc}
}

// Mount registers this context's HUMAN-authenticated config write routes per
// packages/contracts/openapi/openapi.yaml: POST /inventory/items
// (inventory.manage) and POST /inventory/recipes (recipe.manage). Both are
// CONFIG, cloud→edge, ordinary unwrapped writes — envelopes are the
// edge→cloud replay pattern and appear on no config route (ADR-018 §10).
func (h *Handler) Mount(r chi.Router) {
	r.With(auth.RequirePermission(contracts.PermissionInventoryManage)).
		Post("/inventory/items", h.createInventoryItem)
	r.With(auth.RequirePermission(contracts.PermissionRecipeManage)).
		Post("/inventory/recipes", h.createRecipe)
}

// MountIngest registers POST /inventory/ledger-entries and
// POST /inventory/counts: edge→cloud replay by definition (§50.1) — the
// caller is always an enrolled device, never a browser — mirroring
// backend/internal/kitchen.MountIngest and backend/internal/ordering's
// identical split.
func (h *Handler) MountIngest(r chi.Router) {
	r.Post("/inventory/ledger-entries", h.ingestLedgerEntries)
	r.Post("/inventory/counts", h.ingestStockCount)
}

func deviceCaller(r *http.Request) (tenantID, outletID string, ok bool) {
	p, ok := outlet.DevicePrincipalFromContext(r.Context())
	if !ok {
		return "", "", false
	}
	return p.TenantID, p.OutletID, true
}

func requireEnvelopeOutletMatch(envOutletID, callerOutletID string) error {
	if envOutletID != "" && envOutletID != callerOutletID {
		return errors.New("envelope outlet_id does not match the authenticated device's outlet")
	}
	return nil
}

// --- envelope plumbing, mirroring backend/internal/kitchen/http.go ---------

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

// --- POST /inventory/ledger-entries ----------------------------------------

// ingestLedgerEntries is ADR-018 §10.1's route pinning a SET of aggregate
// types rather than one: stock_ledger_entry and stock_deduction_gap share
// this route because a gap belongs beside the movements it failed to
// produce. It switches on env.AggregateType across the route's declared
// set, calling the existing single-type pin for the matched arm; anything
// outside the set is 422 from the default arm — the mechanical correction
// ADR-018 §10.1 records: requireEnvelope pins exactly one type per call, so
// this handler is what pins the SET.
func (h *Handler) ingestLedgerEntries(w http.ResponseWriter, r *http.Request) {
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
	case contracts.AggregateTypeStockLedgerEntry:
		var entry contracts.StockLedgerEntry
		if err := json.Unmarshal(payload, &entry); err != nil {
			httpx.Error(w, httpx.ErrInvalidInput)
			return
		}
		stored, err := h.svc.IngestLedgerEntry(r.Context(), tenantID, env, entry)
		if err != nil {
			writeIngestError(w, err)
			return
		}
		httpx.JSON(w, http.StatusCreated, stored)

	case contracts.AggregateTypeStockDeductionGap:
		var gap contracts.StockDeductionGap
		if err := json.Unmarshal(payload, &gap); err != nil {
			httpx.Error(w, httpx.ErrInvalidInput)
			return
		}
		stored, err := h.svc.IngestDeductionGap(r.Context(), tenantID, env, gap)
		if err != nil {
			writeIngestError(w, err)
			return
		}
		httpx.JSON(w, http.StatusCreated, stored)

	default:
		writeEnvelopeRouteMismatch(w, ErrAuthorityViolation)
	}
}

// --- POST /inventory/counts -------------------------------------------------

type stockCountPayload struct {
	contracts.StockCount
	Lines []contracts.StockCountLine `json:"lines"`
}

func (h *Handler) ingestStockCount(w http.ResponseWriter, r *http.Request) {
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

	var body stockCountPayload
	if err := json.Unmarshal(payload, &body); err != nil {
		httpx.Error(w, httpx.ErrInvalidInput)
		return
	}
	if body.Lines == nil {
		body.Lines = []contracts.StockCountLine{}
	}

	stored, err := h.svc.IngestStockCount(r.Context(), tenantID, env, StockCountReplay{Count: body.StockCount, Lines: body.Lines})
	if err != nil {
		writeIngestError(w, err)
		return
	}
	httpx.JSON(w, http.StatusCreated, stockCountPayload{StockCount: stored.Count, Lines: stored.Lines})
}

// --- POST /inventory/items --------------------------------------------------

func (h *Handler) principalTenant(r *http.Request) (string, bool) {
	p, ok := auth.PrincipalFromContext(r.Context())
	if !ok {
		return "", false
	}
	return p.TenantID, true
}

type createItemUnitConversionRequest struct {
	ID              string `json:"id"`
	PackUnitLabel   string `json:"pack_unit_label"`
	SourceDimension string `json:"source_dimension"`
	Numerator       int64  `json:"numerator"`
	Denominator     int64  `json:"denominator"`
}

type createInventoryItemRequest struct {
	ID                string                            `json:"id"`
	OutletID          string                            `json:"outlet_id"`
	SKU               string                            `json:"sku"`
	Name              string                            `json:"name"`
	Category          *string                           `json:"category"`
	Dimension         string                            `json:"dimension"`
	ReorderLevelMicro *int64                            `json:"reorder_level_micro"`
	ParLevelMicro     *int64                            `json:"par_level_micro"`
	StorageLocation   *string                           `json:"storage_location"`
	IsActive          bool                              `json:"is_active"`
	Conversions       []createItemUnitConversionRequest `json:"conversions"`
}

func (h *Handler) createInventoryItem(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	var req createInventoryItemRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	conversions := make([]NewItemUnitConversionInput, 0, len(req.Conversions))
	for _, c := range req.Conversions {
		conversions = append(conversions, NewItemUnitConversionInput{
			ID: c.ID, PackUnitLabel: c.PackUnitLabel, SourceDimension: contracts.Dimension(c.SourceDimension),
			Numerator: c.Numerator, Denominator: c.Denominator,
		})
	}
	item, storedConversions, err := h.svc.CreateInventoryItem(r.Context(), tenantID, NewInventoryItemInput{
		ID: req.ID, OutletID: req.OutletID, SKU: req.SKU, Name: req.Name, Category: req.Category,
		Dimension: contracts.Dimension(req.Dimension), ReorderLevelMicro: req.ReorderLevelMicro,
		ParLevelMicro: req.ParLevelMicro, StorageLocation: req.StorageLocation, IsActive: req.IsActive,
		Conversions: conversions,
	})
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, struct {
		contracts.InventoryItem
		Conversions []contracts.ItemUnitConversion `json:"conversions"`
	}{InventoryItem: item, Conversions: storedConversions})
}

// --- POST /inventory/recipes ------------------------------------------------

type createRecipeIngredientRequest struct {
	ID                string  `json:"id"`
	ComponentKind     string  `json:"component_kind"`
	InventoryItemID   *string `json:"inventory_item_id"`
	SubRecipeID       *string `json:"sub_recipe_id"`
	QuantityMicro     int64   `json:"quantity_micro"`
	QuantityDimension string  `json:"quantity_dimension"`
	SortOrder         int     `json:"sort_order"`
}

type createRecipeRequestBody struct {
	ID                  string `json:"id"`
	MenuItemVariantID   string `json:"menu_item_variant_id"`
	Name                string `json:"name"`
	OutputDimension     string `json:"output_dimension"`
	OutputQuantityMicro int64  `json:"output_quantity_micro"`
}

type createRecipeRequest struct {
	Recipe      createRecipeRequestBody         `json:"recipe"`
	Ingredients []createRecipeIngredientRequest `json:"ingredients"`
}

func (h *Handler) createRecipe(w http.ResponseWriter, r *http.Request) {
	tenantID, ok := h.principalTenant(r)
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	var req createRecipeRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	ingredients := make([]NewRecipeIngredientInput, 0, len(req.Ingredients))
	for _, i := range req.Ingredients {
		ingredients = append(ingredients, NewRecipeIngredientInput{
			ID: i.ID, ComponentKind: contracts.RecipeComponentKind(i.ComponentKind),
			InventoryItemID: i.InventoryItemID, SubRecipeID: i.SubRecipeID,
			QuantityMicro: i.QuantityMicro, QuantityDimension: contracts.Dimension(i.QuantityDimension),
			SortOrder: i.SortOrder,
		})
	}
	recipe, storedIngredients, err := h.svc.CreateRecipe(r.Context(), tenantID, NewRecipeInput{
		ID: req.Recipe.ID, MenuItemVariantID: req.Recipe.MenuItemVariantID, Name: req.Recipe.Name,
		OutputDimension: contracts.Dimension(req.Recipe.OutputDimension), OutputQuantityMicro: req.Recipe.OutputQuantityMicro,
		Ingredients: ingredients,
	})
	if err != nil {
		// ErrRecipeCycle/ErrRecipeDepthExceeded wrap httpx.ErrConflict, which
		// httpx.Error already maps to 409 naming the offending path. But
		// ErrDimensionMismatch is a distinguished 422 per
		// packages/contracts/openapi/openapi.yaml POST /inventory/recipes —
		// distinct from the plain 400 httpx.ErrInvalidInput otherwise maps
		// to — so it is special-cased ahead of the shared error envelope,
		// the same shape writeEnvelopeRouteMismatch already uses for 422.
		if errors.Is(err, ErrDimensionMismatch) {
			httpx.JSON(w, http.StatusUnprocessableEntity, envelopeRouteMismatchBody{
				Code:    "dimension_mismatch",
				Message: err.Error(),
			})
			return
		}
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, struct {
		Recipe      contracts.Recipe             `json:"recipe"`
		Ingredients []contracts.RecipeIngredient `json:"ingredients"`
	}{Recipe: recipe, Ingredients: storedIngredients})
}
