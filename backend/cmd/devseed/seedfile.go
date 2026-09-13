// seed/demo-outlet.json is the single shared seed source (seed/README.md).
// This file decodes it into typed Go structs -- deliberately NOT
// map[string]any, so that a field the frozen schema requires but the JSON
// omits is a decode failure, never a silent zero. The emitter side lives in
// edge/database/src/bin/devseed.rs; both readers consume the same committed
// bytes.
//
// Only the "Shared" tables from seed/README.md are decoded here. Cloud-only
// rows (role, role_permission, app_user, user_role) stay hand-written in
// main.go and are never read from this file.
package main

import (
	"encoding/json"
	"fmt"
	"os"
)

// seedFile mirrors the top-level object documented in seed/README.md
// "File format". Field order matches the README for ease of comparison.
type seedFile struct {
	SchemaVersion int    `json:"schema_version"`
	GeneratedBy   string `json:"generated_by"`

	// SHA-256 of the seed/outlet.toml the emitter read, lowercase hex. Which
	// restaurant's identity file produced this catalogue -- so a run can be
	// traced back to the file that configured it, and so a mismatch is
	// detectable rather than a wrong name nobody notices.
	OutletSourceSHA256 string `json:"outlet_source_sha256"`

	Tenant seedTenant `json:"tenant"`
	Brand  seedBrand  `json:"brand"`
	Outlet seedOutlet `json:"outlet"`

	// Everything a GST invoice prints that is not already on the outlet row,
	// from seed/outlet.toml (schema_version 2).
	//
	// THE CLOUD DECODES THIS AND WRITES NONE OF IT TODAY, deliberately:
	// outlet_fiscal_profile is edge-seeded (seed/README.md "Scope"), so this
	// block travels for parity of DESCRIPTION, not of rows -- exactly the
	// menu_item.station_code precedent, which the edge consumes to build
	// menu_item_station and the cloud discards because Postgres has no such
	// table.
	//
	// It is declared rather than omitted because the decoder runs with
	// DisallowUnknownFields: an undeclared field here is a loud failure, and
	// "decode it and choose not to write it" is a recorded decision, while
	// "never hear about it" is the contracts 0.5.9 defect.
	//
	// Tier 2 (docs/pilot-readiness.md) puts these fields behind an admin
	// "Outlet settings" screen writing outlet + outlet_fiscal_profile HERE,
	// in the cloud, delivered to the edge by the config pull. This struct is
	// where that writer will read from.
	OutletIdentity seedOutletIdentity `json:"outlet_identity"`

	TaxProfiles        []seedTaxProfile        `json:"tax_profiles"`
	ComplianceVersions []seedComplianceVersion `json:"compliance_versions"`
	TaxRules           []seedTaxRule           `json:"tax_rules"`

	MenuCategories    []seedMenuCategory     `json:"menu_categories"`
	MenuItems         []seedMenuItem         `json:"menu_items"`
	MenuItemVariants  []seedMenuItemVariant  `json:"menu_item_variants"`
	MenuItemModifiers []seedMenuItemModifier `json:"menu_item_modifiers"`

	InventoryItems      []seedInventoryItem      `json:"inventory_items"`
	ItemUnitConversions []seedItemUnitConversion `json:"item_unit_conversions"`
	Recipes             []seedRecipe             `json:"recipes"`
	RecipeIngredients   []seedRecipeIngredient   `json:"recipe_ingredients"`

	ModifierIngredientDeltas []seedModifierIngredientDelta `json:"modifier_ingredient_deltas"`

	Suppliers     []seedSupplier     `json:"suppliers"`
	SupplierItems []seedSupplierItem `json:"supplier_items"`

	// Deliberate exception (seed/README.md, "The goods receipt is a
	// deliberate exception"). Nullable: a seed file with no receipt yet is
	// legal while the demo content is still being authored.
	GoodsReceipt *seedGoodsReceipt      `json:"goods_receipt"`
	OpeningStock []seedStockLedgerEntry `json:"opening_stock"`
}

type seedTenant struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

type seedBrand struct {
	ID       string `json:"id"`
	TenantID string `json:"tenant_id"`
	Name     string `json:"name"`
}

// seedOutletIdentity mirrors the `outlet_identity` block, which mirrors
// seed/outlet.toml one key for one field. Every value is a validated string
// by the time it reaches this file: the emitter refuses a GSTIN whose leading
// digits disagree with state_code, a non-six-digit pincode, an invoice_prefix
// that is not [A-Z]{1,4}/, and any non-ASCII character in a printed string.
// See edge/database/src/bin/devseed/outlet_identity.rs.
type seedOutletIdentity struct {
	RestaurantName    string  `json:"restaurant_name"`
	LegalName         string  `json:"legal_name"`
	OutletName        string  `json:"outlet_name"`
	AddressLine1      string  `json:"address_line1"`
	AddressLine2      *string `json:"address_line2"`
	City              string  `json:"city"`
	StateCode         string  `json:"state_code"`
	StateName         string  `json:"state_name"`
	Pincode           string  `json:"pincode"`
	GSTIN             string  `json:"gstin"`
	FSSAI             *string `json:"fssai"`
	InvoicePrefix     string  `json:"invoice_prefix"`
	InvoiceFooterText string  `json:"invoice_footer_text"`
}

type seedOutlet struct {
	ID           string `json:"id"`
	BrandID      string `json:"brand_id"`
	Name         string `json:"name"`
	Timezone     string `json:"timezone"`
	DayStartTime string `json:"day_start_time"`
}

type seedTaxProfile struct {
	ID          string `json:"id"`
	OutletID    string `json:"outlet_id"`
	Code        string `json:"code"`
	Name        string `json:"name"`
	PricingMode string `json:"pricing_mode"`
	IsDefault   bool   `json:"is_default"`
	IsActive    bool   `json:"is_active"`
}

type seedComplianceVersion struct {
	ID            string  `json:"id"`
	OutletID      string  `json:"outlet_id"`
	Label         string  `json:"label"`
	EffectiveFrom string  `json:"effective_from"`
	Notes         *string `json:"notes"`
}

type seedTaxRule struct {
	ID                  string  `json:"id"`
	TaxProfileID        string  `json:"tax_profile_id"`
	ComplianceVersionID string  `json:"compliance_version_id"`
	Component           string  `json:"component"`
	RateBps             int64   `json:"rate_bps"`
	EffectiveFrom       string  `json:"effective_from"`
	EffectiveTo         *string `json:"effective_to"`
}

type seedMenuCategory struct {
	ID        string `json:"id"`
	OutletID  string `json:"outlet_id"`
	Name      string `json:"name"`
	SortOrder int    `json:"sort_order"`
}

type seedMenuItem struct {
	ID             string  `json:"id"`
	OutletID       string  `json:"outlet_id"`
	CategoryID     string  `json:"category_id"`
	Name           string  `json:"name"`
	BasePricePaise int64   `json:"base_price_paise"`
	IsAvailable    bool    `json:"is_available"`
	TaxProfileID   *string `json:"tax_profile_id"`
	HsnSac         string  `json:"hsn_sac"`
	// StationCode travels for the edge only, to build menu_item_station --
	// there is no such table in Postgres. Read and deliberately discarded.
	StationCode string `json:"station_code"`
}

type seedMenuItemVariant struct {
	ID              string `json:"id"`
	MenuItemID      string `json:"menu_item_id"`
	Name            string `json:"name"`
	PriceDeltaPaise int64  `json:"price_delta_paise"`
	IsDefault       bool   `json:"is_default"`
}

type seedMenuItemModifier struct {
	ID              string `json:"id"`
	MenuItemID      string `json:"menu_item_id"`
	GroupName       string `json:"group_name"`
	OptionName      string `json:"option_name"`
	PriceDeltaPaise int64  `json:"price_delta_paise"`
	MinSelection    int    `json:"min_selection"`
	MaxSelection    int    `json:"max_selection"`
}

type seedInventoryItem struct {
	ID                string  `json:"id"`
	OutletID          string  `json:"outlet_id"`
	Sku               string  `json:"sku"`
	Name              string  `json:"name"`
	Category          *string `json:"category"`
	Dimension         string  `json:"dimension"`
	ReorderLevelMicro *int64  `json:"reorder_level_micro"`
}

type seedItemUnitConversion struct {
	ID              string `json:"id"`
	InventoryItemID string `json:"inventory_item_id"`
	PackUnitLabel   string `json:"pack_unit_label"`
	SourceDimension string `json:"source_dimension"`
	Numerator       int64  `json:"numerator"`
	Denominator     int64  `json:"denominator"`
}

type seedRecipe struct {
	ID                  string `json:"id"`
	MenuItemVariantID   string `json:"menu_item_variant_id"`
	Name                string `json:"name"`
	OutputDimension     string `json:"output_dimension"`
	OutputQuantityMicro int64  `json:"output_quantity_micro"`
}

// seedRecipeIngredient. component_kind is deliberately NOT a JSON field: it
// is derived here from which of InventoryItemID/SubRecipeID is set, a purely
// structural fact rather than an authored value -- unlike QuantityDimension,
// which is the unit the author chose and is NEVER derived (contracts 0.5.2).
type seedRecipeIngredient struct {
	ID                string  `json:"id"`
	RecipeID          string  `json:"recipe_id"`
	InventoryItemID   *string `json:"inventory_item_id"`
	SubRecipeID       *string `json:"sub_recipe_id"`
	QuantityMicro     int64   `json:"quantity_micro"`
	QuantityDimension string  `json:"quantity_dimension"`
}

type seedModifierIngredientDelta struct {
	ID                 string `json:"id"`
	MenuItemModifierID string `json:"menu_item_modifier_id"`
	InventoryItemID    string `json:"inventory_item_id"`
	QuantityMicro      int64  `json:"quantity_micro"`
}

type seedSupplier struct {
	ID               string  `json:"id"`
	OutletID         string  `json:"outlet_id"`
	Code             string  `json:"code"`
	Name             string  `json:"name"`
	Gstin            *string `json:"gstin"`
	Phone            *string `json:"phone"`
	Email            *string `json:"email"`
	Address          *string `json:"address"`
	PaymentTermsDays int     `json:"payment_terms_days"`
	IsActive         bool    `json:"is_active"`
}

type seedSupplierItem struct {
	ID                string `json:"id"`
	SupplierID        string `json:"supplier_id"`
	InventoryItemID   string `json:"inventory_item_id"`
	PurchaseUnit      string `json:"purchase_unit"`
	PackSizeMicro     int64  `json:"pack_size_micro"`
	QuantityDimension string `json:"quantity_dimension"`
	LastPricePaise    *int64 `json:"last_price_paise"`
	IsPreferred       bool   `json:"is_preferred"`
}

// seedGoodsReceipt and seedGRNLine mirror goods_receipt_note/grn_line
// exactly (seed/README.md "deliberate exception"). Seeded into BOTH stores
// directly, never through the edge->cloud replay path A7 would otherwise
// provide -- and the cloud's copy stays a REPLICA (contracts 0.7.0)
// regardless of how it got there.
type seedGoodsReceipt struct {
	ID               string        `json:"id"`
	OutletID         string        `json:"outlet_id"`
	PurchaseOrderID  *string       `json:"purchase_order_id"`
	SupplierID       *string       `json:"supplier_id"`
	GrnNumber        string        `json:"grn_number"`
	DeliveryNoteRef  *string       `json:"delivery_note_ref"`
	ReceivedAt       string        `json:"received_at"`
	ReceivedByUserID string        `json:"received_by_user_id"`
	BusinessDate     string        `json:"business_date"`
	Notes            *string       `json:"notes"`
	Lines            []seedGRNLine `json:"lines"`
	// LedgerEntries are the stock movements the receipt produced, if the
	// emitter chooses to carry them here rather than in top-level
	// opening_stock. Optional: this file never invents a ledger row the JSON
	// did not supply.
	LedgerEntries []seedStockLedgerEntry `json:"ledger_entries"`
}

type seedGRNLine struct {
	ID                   string  `json:"id"`
	InventoryItemID      string  `json:"inventory_item_id"`
	LineNumber           int     `json:"line_number"`
	PurchaseOrderLineID  *string `json:"purchase_order_line_id"`
	EnteredPurchaseUnit  string  `json:"entered_purchase_unit"`
	EnteredQuantityMicro int64   `json:"entered_quantity_micro"`
	QuantityDimension    string  `json:"quantity_dimension"`
	BaseQuantityMicro    int64   `json:"base_quantity_micro"`
	PackSizeMicroApplied int64   `json:"pack_size_micro_applied"`
	UnitCostPaise        int64   `json:"unit_cost_paise"`
	LineTotalPaise       int64   `json:"line_total_paise"`
	BatchCode            *string `json:"batch_code"`
	ExpiryDate           *string `json:"expiry_date"`
}

// seedStockLedgerEntry mirrors stock_ledger_entry. entry_seq is optional in
// the JSON: if the emitter supplies one it is used verbatim, otherwise this
// seeder assigns the next value from a local, outlet-scoped counter. Either
// way the value actually written is never invented silently -- it is either
// the author's value or a value this file is responsible for and documents.
type seedStockLedgerEntry struct {
	ID                       string  `json:"id"`
	OutletID                 *string `json:"outlet_id"`
	EntrySeq                 *int64  `json:"entry_seq"`
	InventoryItemID          string  `json:"inventory_item_id"`
	InventoryItemName        string  `json:"inventory_item_name"`
	Dimension                string  `json:"dimension"`
	EntryType                string  `json:"entry_type"`
	Origin                   string  `json:"origin"`
	QuantityMicro            int64   `json:"quantity_micro"`
	RecipeID                 *string `json:"recipe_id"`
	RecipeVersion            *int    `json:"recipe_version"`
	RecipeName               *string `json:"recipe_name"`
	ReasonCode               *string `json:"reason_code"`
	Note                     *string `json:"note"`
	OccurredAt               string  `json:"occurred_at"`
	BusinessDate             string  `json:"business_date"`
	CreatedByUserID          *string `json:"created_by_user_id"`
	ModifierDeltaID          *string `json:"modifier_delta_id"`
	ModifierName             *string `json:"modifier_name"`
	ModifierDeltaVersion     *int    `json:"modifier_delta_version"`
	UnitCostPaise            *int64  `json:"unit_cost_paise"`
	LineTotalPaise           *int64  `json:"line_total_paise"`
	SourceGrnID              *string `json:"source_grn_id"`
	SourcePurchaseReturnID   *string `json:"source_purchase_return_id"`
	SourceStockTransferOutID *string `json:"source_stock_transfer_out_id"`
	SourceStockCountID       *string `json:"source_stock_count_id"`
}

// loadSeedFile decodes path with DisallowUnknownFields: a field the JSON
// carries that this struct set does not declare is a decode failure, never a
// silently discarded column (the exact shape of the contracts 0.5.9 defect,
// one hop earlier).
func loadSeedFile(path string) (*seedFile, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("opening seed file %s: %w", path, err)
	}
	defer f.Close()

	dec := json.NewDecoder(f)
	dec.DisallowUnknownFields()

	var sf seedFile
	if err := dec.Decode(&sf); err != nil {
		return nil, fmt.Errorf("decoding seed file %s: %w", path, err)
	}

	if err := sf.validate(); err != nil {
		return nil, fmt.Errorf("validating seed file %s: %w", path, err)
	}

	return &sf, nil
}

// validate enforces the rules seed/README.md and CLAUDE.md bind on every
// reader of this file: hsn_sac is never blank (contracts 0.4.5),
// quantity_dimension always travels with a recipe_ingredient row (contracts
// 0.5.2), and the required id fields are never empty.
func (sf *seedFile) validate() error {
	// Bumped 1 -> 2 when the outlet_identity block landed. Pinned to the
	// exact version rather than ">= 1": a reader that accepts an older file
	// accepts one with no identity block at all, and every field below would
	// read as an empty string.
	if sf.SchemaVersion != 2 {
		return fmt.Errorf("schema_version %d is not 2 -- re-emit seed/demo-outlet.json: cd edge/database && cargo run --bin devseed -- --emit-json ../../seed/demo-outlet.json", sf.SchemaVersion)
	}
	// The identity fields the cloud does not write are still REQUIRED to be
	// present and non-empty. A block that decodes to zero values would mean
	// the emitter stopped filling it, and the first symptom otherwise is a
	// blank name on a bill several steps downstream.
	if sf.OutletSourceSHA256 == "" {
		return fmt.Errorf("outlet_source_sha256 must not be empty -- the catalogue does not say which seed/outlet.toml produced it")
	}
	for field, value := range map[string]string{
		"restaurant_name":     sf.OutletIdentity.RestaurantName,
		"legal_name":          sf.OutletIdentity.LegalName,
		"outlet_name":         sf.OutletIdentity.OutletName,
		"address_line1":       sf.OutletIdentity.AddressLine1,
		"city":                sf.OutletIdentity.City,
		"state_code":          sf.OutletIdentity.StateCode,
		"state_name":          sf.OutletIdentity.StateName,
		"pincode":             sf.OutletIdentity.Pincode,
		"gstin":               sf.OutletIdentity.GSTIN,
		"invoice_prefix":      sf.OutletIdentity.InvoicePrefix,
		"invoice_footer_text": sf.OutletIdentity.InvoiceFooterText,
	} {
		if value == "" {
			return fmt.Errorf("outlet_identity.%s must not be empty", field)
		}
	}
	// Re-checked here rather than trusted from the emitter. This reader runs
	// against a COMMITTED file that a person can edit by hand, and a wrong
	// place-of-supply on every invoice is not a defect any screen shows.
	if len(sf.OutletIdentity.GSTIN) < 2 || sf.OutletIdentity.GSTIN[:2] != sf.OutletIdentity.StateCode {
		return fmt.Errorf("outlet_identity.state_code %q does not match the first two digits of gstin %q -- place-of-supply would be wrong on every invoice", sf.OutletIdentity.StateCode, sf.OutletIdentity.GSTIN)
	}
	// The names on the three rows this seeder writes come from the identity
	// file, so they must agree with it. A tenant named for one restaurant and
	// an invoice footer for another is internally consistent on every screen.
	if sf.Tenant.Name != sf.OutletIdentity.LegalName {
		return fmt.Errorf("tenant.name %q does not match outlet_identity.legal_name %q", sf.Tenant.Name, sf.OutletIdentity.LegalName)
	}
	if sf.Brand.Name != sf.OutletIdentity.RestaurantName {
		return fmt.Errorf("brand.name %q does not match outlet_identity.restaurant_name %q", sf.Brand.Name, sf.OutletIdentity.RestaurantName)
	}
	if sf.Outlet.Name != sf.OutletIdentity.OutletName {
		return fmt.Errorf("outlet.name %q does not match outlet_identity.outlet_name %q", sf.Outlet.Name, sf.OutletIdentity.OutletName)
	}
	if sf.Tenant.ID == "" || sf.Brand.ID == "" || sf.Outlet.ID == "" {
		return fmt.Errorf("tenant/brand/outlet id must not be empty")
	}
	// Fixed development ids: keep their exact current values (seed/README.md
	// "Identity"). A mismatch here means the emitted file and the hand-pinned
	// role/user rows in main.go now disagree about which outlet they refer
	// to, and every downstream FK would silently point at the wrong row.
	if sf.Tenant.ID != tenantID {
		return fmt.Errorf("tenant.id %s does not match the pinned devseed tenantID %s", sf.Tenant.ID, tenantID)
	}
	if sf.Outlet.ID != outletID {
		return fmt.Errorf("outlet.id %s does not match the pinned devseed outletID %s", sf.Outlet.ID, outletID)
	}

	for i, mi := range sf.MenuItems {
		if mi.ID == "" {
			return fmt.Errorf("menu_items[%d]: id must not be empty", i)
		}
		if mi.HsnSac == "" {
			return fmt.Errorf("menu_items[%d] (%s): hsn_sac must never be null or blank -- an invoice cannot issue without it (contracts 0.4.5)", i, mi.Name)
		}
	}

	for i, ri := range sf.RecipeIngredients {
		if ri.QuantityDimension == "" {
			return fmt.Errorf("recipe_ingredients[%d]: quantity_dimension must be the author's chosen unit, never blank (contracts 0.5.2)", i)
		}
		hasItem := ri.InventoryItemID != nil && *ri.InventoryItemID != ""
		hasSub := ri.SubRecipeID != nil && *ri.SubRecipeID != ""
		if hasItem == hasSub {
			return fmt.Errorf("recipe_ingredients[%d]: exactly one of inventory_item_id/sub_recipe_id must be set", i)
		}
	}

	for i, si := range sf.SupplierItems {
		if si.QuantityDimension == "" {
			return fmt.Errorf("supplier_items[%d]: quantity_dimension must be the author's chosen unit, never blank (contracts 0.5.2)", i)
		}
	}

	if sf.GoodsReceipt != nil {
		for i, l := range sf.GoodsReceipt.Lines {
			if l.QuantityDimension == "" {
				return fmt.Errorf("goods_receipt.lines[%d]: quantity_dimension must be the author's chosen unit, never blank (contracts 0.5.2)", i)
			}
		}
	}

	return nil
}
