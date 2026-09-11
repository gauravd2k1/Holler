package main

import (
	"context"
	"fmt"
	"time"

	"github.com/holler/backend/internal/platform/postgres"
)

// seedCatalogueFromFile writes every "Shared" table from seed/README.md into
// Postgres, EXCEPT the goods receipt and opening stock -- see
// seedGoodsReceiptAndOpeningStock below for why those wait until after
// seed() runs. Cloud-only rows (role, role_permission, app_user, user_role)
// are NOT written here -- they stay hand-written in seed() in main.go, which
// reads the tenantID/outletID constants this file validates the JSON against.
//
// Every statement is idempotent: ON CONFLICT (id) DO UPDATE where the table
// is mutable, ON CONFLICT (id) DO NOTHING where it is append-only or
// immutable and a second write would fire that table's own trigger.
func seedCatalogueFromFile(ctx context.Context, pool postgres.Pool, sf *seedFile) error {
	now := time.Now().UTC()

	if _, err := pool.Exec(ctx,
		`INSERT INTO tenant (id, name, created_at, updated_at)
		 VALUES ($1, $2, $3, $3)
		 ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, updated_at = EXCLUDED.updated_at`,
		sf.Tenant.ID, sf.Tenant.Name, now); err != nil {
		return fmt.Errorf("seeding tenant: %w", err)
	}

	if _, err := pool.Exec(ctx,
		`INSERT INTO brand (id, tenant_id, name, created_at, updated_at)
		 VALUES ($1, $2, $3, $4, $4)
		 ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, updated_at = EXCLUDED.updated_at`,
		sf.Brand.ID, sf.Brand.TenantID, sf.Brand.Name, now); err != nil {
		return fmt.Errorf("seeding brand: %w", err)
	}

	dayStart := sf.Outlet.DayStartTime
	if dayStart == "" {
		dayStart = "00:00"
	}
	if _, err := pool.Exec(ctx,
		`INSERT INTO outlet (id, brand_id, name, timezone, day_start_time, config_version, created_at, updated_at)
		 VALUES ($1, $2, $3, $4, $5, 1, $6, $6)
		 ON CONFLICT (id) DO UPDATE SET
		   name = EXCLUDED.name, timezone = EXCLUDED.timezone,
		   day_start_time = EXCLUDED.day_start_time, updated_at = EXCLUDED.updated_at`,
		sf.Outlet.ID, sf.Outlet.BrandID, sf.Outlet.Name, sf.Outlet.Timezone, dayStart, now); err != nil {
		return fmt.Errorf("seeding outlet: %w", err)
	}

	for _, tp := range sf.TaxProfiles {
		if _, err := pool.Exec(ctx,
			`INSERT INTO tax_profile (id, outlet_id, code, name, pricing_mode, is_default, is_active, config_version)
			 VALUES ($1, $2, $3, $4, $5, $6, $7, 1)
			 ON CONFLICT (id) DO UPDATE SET
			   name = EXCLUDED.name, pricing_mode = EXCLUDED.pricing_mode,
			   is_default = EXCLUDED.is_default, is_active = EXCLUDED.is_active`,
			tp.ID, tp.OutletID, tp.Code, tp.Name, tp.PricingMode, tp.IsDefault, tp.IsActive); err != nil {
			return fmt.Errorf("seeding tax_profile %s: %w", tp.Code, err)
		}
	}

	for _, cv := range sf.ComplianceVersions {
		effectiveFrom, err := parseTime(cv.EffectiveFrom)
		if err != nil {
			return fmt.Errorf("compliance_version %s: %w", cv.Label, err)
		}
		if _, err := pool.Exec(ctx,
			`INSERT INTO compliance_version (id, outlet_id, label, effective_from, notes, config_version)
			 VALUES ($1, $2, $3, $4, $5, 1)
			 ON CONFLICT (id) DO UPDATE SET
			   label = EXCLUDED.label, effective_from = EXCLUDED.effective_from, notes = EXCLUDED.notes`,
			cv.ID, cv.OutletID, cv.Label, effectiveFrom, cv.Notes); err != nil {
			return fmt.Errorf("seeding compliance_version %s: %w", cv.Label, err)
		}
	}

	for _, tr := range sf.TaxRules {
		effectiveFrom, err := parseTime(tr.EffectiveFrom)
		if err != nil {
			return fmt.Errorf("tax_rule %s: %w", tr.ID, err)
		}
		var effectiveTo *time.Time
		if tr.EffectiveTo != nil {
			t, err := parseTime(*tr.EffectiveTo)
			if err != nil {
				return fmt.Errorf("tax_rule %s effective_to: %w", tr.ID, err)
			}
			effectiveTo = &t
		}
		if _, err := pool.Exec(ctx,
			`INSERT INTO tax_rule (id, tax_profile_id, compliance_version_id, component, rate_bps, effective_from, effective_to, config_version)
			 VALUES ($1, $2, $3, $4, $5, $6, $7, 1)
			 ON CONFLICT (id) DO UPDATE SET
			   component = EXCLUDED.component, rate_bps = EXCLUDED.rate_bps,
			   effective_from = EXCLUDED.effective_from, effective_to = EXCLUDED.effective_to`,
			tr.ID, tr.TaxProfileID, tr.ComplianceVersionID, tr.Component, tr.RateBps, effectiveFrom, effectiveTo); err != nil {
			return fmt.Errorf("seeding tax_rule %s: %w", tr.ID, err)
		}
	}

	for _, mc := range sf.MenuCategories {
		if _, err := pool.Exec(ctx,
			`INSERT INTO menu_category (id, outlet_id, name, sort_order, config_version)
			 VALUES ($1, $2, $3, $4, 1)
			 ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, sort_order = EXCLUDED.sort_order`,
			mc.ID, mc.OutletID, mc.Name, mc.SortOrder); err != nil {
			return fmt.Errorf("seeding menu_category %s: %w", mc.Name, err)
		}
	}

	// station_code is read above by the JSON decoder and deliberately
	// discarded here: it feeds menu_item_station at the edge, and Postgres
	// has no such table (seed/README.md).
	for _, mi := range sf.MenuItems {
		if _, err := pool.Exec(ctx,
			`INSERT INTO menu_item (id, outlet_id, category_id, name, base_price_paise, is_available, tax_profile_id, hsn_sac, config_version)
			 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 1)
			 ON CONFLICT (id) DO UPDATE SET
			   name = EXCLUDED.name, base_price_paise = EXCLUDED.base_price_paise,
			   is_available = EXCLUDED.is_available, tax_profile_id = EXCLUDED.tax_profile_id,
			   hsn_sac = EXCLUDED.hsn_sac, category_id = EXCLUDED.category_id`,
			mi.ID, mi.OutletID, mi.CategoryID, mi.Name, mi.BasePricePaise, mi.IsAvailable, mi.TaxProfileID, mi.HsnSac); err != nil {
			return fmt.Errorf("seeding menu_item %s: %w", mi.Name, err)
		}
	}

	for _, mv := range sf.MenuItemVariants {
		if _, err := pool.Exec(ctx,
			`INSERT INTO menu_item_variant (id, menu_item_id, name, price_delta_paise, is_default, config_version)
			 VALUES ($1, $2, $3, $4, $5, 1)
			 ON CONFLICT (id) DO UPDATE SET
			   name = EXCLUDED.name, price_delta_paise = EXCLUDED.price_delta_paise, is_default = EXCLUDED.is_default`,
			mv.ID, mv.MenuItemID, mv.Name, mv.PriceDeltaPaise, mv.IsDefault); err != nil {
			return fmt.Errorf("seeding menu_item_variant %s: %w", mv.Name, err)
		}
	}

	for _, mm := range sf.MenuItemModifiers {
		if _, err := pool.Exec(ctx,
			`INSERT INTO menu_item_modifier (id, menu_item_id, group_name, option_name, price_delta_paise, min_selection, max_selection, config_version)
			 VALUES ($1, $2, $3, $4, $5, $6, $7, 1)
			 ON CONFLICT (id) DO UPDATE SET
			   group_name = EXCLUDED.group_name, option_name = EXCLUDED.option_name,
			   price_delta_paise = EXCLUDED.price_delta_paise,
			   min_selection = EXCLUDED.min_selection, max_selection = EXCLUDED.max_selection`,
			mm.ID, mm.MenuItemID, mm.GroupName, mm.OptionName, mm.PriceDeltaPaise, mm.MinSelection, mm.MaxSelection); err != nil {
			return fmt.Errorf("seeding menu_item_modifier %s/%s: %w", mm.GroupName, mm.OptionName, err)
		}
	}

	for _, ii := range sf.InventoryItems {
		if _, err := pool.Exec(ctx,
			`INSERT INTO inventory_item (id, outlet_id, sku, name, category, dimension, reorder_level_micro, is_active, config_version)
			 VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE, 1)
			 ON CONFLICT (id) DO UPDATE SET
			   name = EXCLUDED.name, category = EXCLUDED.category,
			   dimension = EXCLUDED.dimension, reorder_level_micro = EXCLUDED.reorder_level_micro`,
			ii.ID, ii.OutletID, ii.Sku, ii.Name, ii.Category, ii.Dimension, ii.ReorderLevelMicro); err != nil {
			return fmt.Errorf("seeding inventory_item %s: %w", ii.Sku, err)
		}
	}

	for _, uc := range sf.ItemUnitConversions {
		if _, err := pool.Exec(ctx,
			`INSERT INTO item_unit_conversion (id, inventory_item_id, pack_unit_label, source_dimension, numerator, denominator, config_version)
			 VALUES ($1, $2, $3, $4, $5, $6, 1)
			 ON CONFLICT (id) DO UPDATE SET
			   pack_unit_label = EXCLUDED.pack_unit_label, source_dimension = EXCLUDED.source_dimension,
			   numerator = EXCLUDED.numerator, denominator = EXCLUDED.denominator`,
			uc.ID, uc.InventoryItemID, uc.PackUnitLabel, uc.SourceDimension, uc.Numerator, uc.Denominator); err != nil {
			return fmt.Errorf("seeding item_unit_conversion %s: %w", uc.PackUnitLabel, err)
		}
	}

	// Recipes must be written before recipe_ingredients that reference one
	// another as sub-recipes, and the input slice order from the emitter is
	// trusted to already respect that (the emitter walks the same graph it
	// authored). A forward reference here is a foreign-key error, loud and
	// immediate, never a silent drop.
	for _, r := range sf.Recipes {
		if _, err := pool.Exec(ctx,
			`INSERT INTO recipe (id, menu_item_variant_id, name, output_dimension, output_quantity_micro, config_version)
			 VALUES ($1, $2, $3, $4, $5, 1)
			 ON CONFLICT (id) DO UPDATE SET
			   name = EXCLUDED.name, output_dimension = EXCLUDED.output_dimension,
			   output_quantity_micro = EXCLUDED.output_quantity_micro`,
			r.ID, r.MenuItemVariantID, r.Name, r.OutputDimension, r.OutputQuantityMicro); err != nil {
			return fmt.Errorf("seeding recipe %s: %w", r.Name, err)
		}
	}

	for _, ri := range sf.RecipeIngredients {
		componentKind := "ITEM"
		if ri.SubRecipeID != nil && *ri.SubRecipeID != "" {
			componentKind = "SUB_RECIPE"
		}
		if _, err := pool.Exec(ctx,
			`INSERT INTO recipe_ingredient (id, recipe_id, component_kind, inventory_item_id, sub_recipe_id, quantity_micro, quantity_dimension, config_version)
			 VALUES ($1, $2, $3, $4, $5, $6, $7, 1)
			 ON CONFLICT (id) DO UPDATE SET
			   quantity_micro = EXCLUDED.quantity_micro, quantity_dimension = EXCLUDED.quantity_dimension`,
			ri.ID, ri.RecipeID, componentKind, ri.InventoryItemID, ri.SubRecipeID, ri.QuantityMicro, ri.QuantityDimension); err != nil {
			return fmt.Errorf("seeding recipe_ingredient %s: %w", ri.ID, err)
		}
	}

	for _, md := range sf.ModifierIngredientDeltas {
		if _, err := pool.Exec(ctx,
			`INSERT INTO modifier_ingredient_delta (id, menu_item_modifier_id, inventory_item_id, quantity_micro, config_version)
			 VALUES ($1, $2, $3, $4, 1)
			 ON CONFLICT (id) DO UPDATE SET quantity_micro = EXCLUDED.quantity_micro`,
			md.ID, md.MenuItemModifierID, md.InventoryItemID, md.QuantityMicro); err != nil {
			return fmt.Errorf("seeding modifier_ingredient_delta %s: %w", md.ID, err)
		}
	}

	for _, s := range sf.Suppliers {
		if _, err := pool.Exec(ctx,
			`INSERT INTO supplier (id, outlet_id, code, name, gstin, phone, email, address, payment_terms_days, is_active, config_version, created_at, updated_at)
			 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 1, $11, $11)
			 ON CONFLICT (id) DO UPDATE SET
			   name = EXCLUDED.name, gstin = EXCLUDED.gstin, phone = EXCLUDED.phone,
			   email = EXCLUDED.email, address = EXCLUDED.address,
			   payment_terms_days = EXCLUDED.payment_terms_days, is_active = EXCLUDED.is_active,
			   updated_at = EXCLUDED.updated_at`,
			s.ID, s.OutletID, s.Code, s.Name, s.Gstin, s.Phone, s.Email, s.Address, s.PaymentTermsDays, s.IsActive, now); err != nil {
			return fmt.Errorf("seeding supplier %s: %w", s.Code, err)
		}
	}

	for _, si := range sf.SupplierItems {
		if _, err := pool.Exec(ctx,
			`INSERT INTO supplier_item (id, supplier_id, inventory_item_id, purchase_unit, pack_size_micro, quantity_dimension, last_price_paise, is_preferred)
			 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
			 ON CONFLICT (id) DO UPDATE SET
			   purchase_unit = EXCLUDED.purchase_unit, pack_size_micro = EXCLUDED.pack_size_micro,
			   quantity_dimension = EXCLUDED.quantity_dimension, last_price_paise = EXCLUDED.last_price_paise,
			   is_preferred = EXCLUDED.is_preferred`,
			si.ID, si.SupplierID, si.InventoryItemID, si.PurchaseUnit, si.PackSizeMicro, si.QuantityDimension, si.LastPricePaise, si.IsPreferred); err != nil {
			return fmt.Errorf("seeding supplier_item %s: %w", si.ID, err)
		}
	}

	return nil
}

// seedGoodsReceiptAndOpeningStock writes the goods receipt (and its ledger
// entries) plus any standalone opening_stock rows. Split out from
// seedCatalogueFromFile deliberately: goods_receipt_note.received_by_user_id
// and stock_ledger_entry.created_by_user_id reference app_user, and the
// cloud-only identity rows (role, app_user, user_role) are seeded by seed()
// in main.go, which itself depends on tenant/outlet already existing. The
// call order in main() is therefore: seedCatalogueFromFile, then seed(),
// then this function.
func seedGoodsReceiptAndOpeningStock(ctx context.Context, pool postgres.Pool, sf *seedFile) error {
	if sf.GoodsReceipt != nil {
		if err := seedGoodsReceiptFromFile(ctx, pool, sf.GoodsReceipt); err != nil {
			return err
		}
	}

	if len(sf.OpeningStock) > 0 {
		if err := seedStockLedgerEntries(ctx, pool, sf.Outlet.ID, sf.OpeningStock); err != nil {
			return fmt.Errorf("seeding opening_stock: %w", err)
		}
	}

	return nil
}

// seedGoodsReceiptFromFile writes goods_receipt_note and its grn_line rows,
// plus any ledger entries the emitter chose to carry alongside them. Both
// goods_receipt_note and stock_ledger_entry reject an UPDATE with a trigger
// (IMMUTABLE / append-only respectively), so re-running this against an
// already-seeded database uses ON CONFLICT (id) DO NOTHING rather than the
// DO UPDATE pattern used elsewhere in this file -- a DO UPDATE here would
// fire the trigger and fail the whole re-run.
func seedGoodsReceiptFromFile(ctx context.Context, pool postgres.Pool, gr *seedGoodsReceipt) error {
	receivedAt, err := parseTime(gr.ReceivedAt)
	if err != nil {
		return fmt.Errorf("goods_receipt.received_at: %w", err)
	}
	businessDate, err := parseDate(gr.BusinessDate)
	if err != nil {
		return fmt.Errorf("goods_receipt.business_date: %w", err)
	}

	if _, err := pool.Exec(ctx,
		`INSERT INTO goods_receipt_note (id, tenant_id, outlet_id, purchase_order_id, supplier_id, grn_number, delivery_note_ref, received_at, received_by_user_id, business_date, notes)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
		 ON CONFLICT (id) DO NOTHING`,
		gr.ID, tenantID, gr.OutletID, gr.PurchaseOrderID, gr.SupplierID, gr.GrnNumber, gr.DeliveryNoteRef, receivedAt, gr.ReceivedByUserID, businessDate, gr.Notes); err != nil {
		return fmt.Errorf("seeding goods_receipt_note %s: %w", gr.GrnNumber, err)
	}

	for _, l := range gr.Lines {
		if _, err := pool.Exec(ctx,
			`INSERT INTO grn_line (id, grn_id, inventory_item_id, line_number, purchase_order_line_id, entered_purchase_unit, entered_quantity_micro, quantity_dimension, base_quantity_micro, pack_size_micro_applied, unit_cost_paise, line_total_paise, batch_code, expiry_date)
			 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
			 ON CONFLICT (id) DO NOTHING`,
			l.ID, gr.ID, l.InventoryItemID, l.LineNumber, l.PurchaseOrderLineID, l.EnteredPurchaseUnit, l.EnteredQuantityMicro, l.QuantityDimension, l.BaseQuantityMicro, l.PackSizeMicroApplied, l.UnitCostPaise, l.LineTotalPaise, l.BatchCode, l.ExpiryDate); err != nil {
			return fmt.Errorf("seeding grn_line %d: %w", l.LineNumber, err)
		}
	}

	if len(gr.LedgerEntries) > 0 {
		if err := seedStockLedgerEntries(ctx, pool, gr.OutletID, gr.LedgerEntries); err != nil {
			return fmt.Errorf("seeding goods_receipt ledger entries: %w", err)
		}
	}

	return nil
}

// seedStockLedgerEntries writes rows into stock_ledger_entry. entry_seq is
// taken from the JSON when the emitter supplies one; otherwise this
// function assigns the next value from the outlet's current high-water
// mark, so a re-run against an already-seeded database and a fresh bootstrap
// both produce a row satisfying UNIQUE (outlet_id, entry_seq).
func seedStockLedgerEntries(ctx context.Context, pool postgres.Pool, defaultOutletID string, entries []seedStockLedgerEntry) error {
	var nextSeq int64
	needsCounter := false
	for _, e := range entries {
		if e.EntrySeq == nil {
			needsCounter = true
			break
		}
	}
	if needsCounter {
		row := pool.QueryRow(ctx,
			`SELECT COALESCE(MAX(entry_seq), 0) FROM stock_ledger_entry WHERE outlet_id = $1`, defaultOutletID)
		if err := row.Scan(&nextSeq); err != nil {
			return fmt.Errorf("reading current stock_ledger_entry high-water mark: %w", err)
		}
	}

	for _, e := range entries {
		outletID := defaultOutletID
		if e.OutletID != nil && *e.OutletID != "" {
			outletID = *e.OutletID
		}

		entrySeq := e.EntrySeq
		if entrySeq == nil {
			nextSeq++
			v := nextSeq
			entrySeq = &v
		}

		occurredAt, err := parseTime(e.OccurredAt)
		if err != nil {
			return fmt.Errorf("stock_ledger_entry %s occurred_at: %w", e.ID, err)
		}
		businessDate, err := parseDate(e.BusinessDate)
		if err != nil {
			return fmt.Errorf("stock_ledger_entry %s business_date: %w", e.ID, err)
		}

		if _, err := pool.Exec(ctx,
			`INSERT INTO stock_ledger_entry (
				id, outlet_id, entry_seq, inventory_item_id, inventory_item_name, dimension,
				entry_type, origin, quantity_applied_micro,
				recipe_id, recipe_version, recipe_name,
				reason_code, note, occurred_at, business_date, created_by_user_id,
				modifier_delta_id, modifier_name, modifier_delta_version,
				unit_cost_paise, line_total_paise,
				source_grn_id, source_purchase_return_id, source_stock_transfer_out_id, source_stock_count_id
			 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26)
			 ON CONFLICT (id) DO NOTHING`,
			e.ID, outletID, *entrySeq, e.InventoryItemID, e.InventoryItemName, e.Dimension,
			e.EntryType, e.Origin, e.QuantityMicro,
			e.RecipeID, e.RecipeVersion, e.RecipeName,
			e.ReasonCode, e.Note, occurredAt, businessDate, e.CreatedByUserID,
			e.ModifierDeltaID, e.ModifierName, e.ModifierDeltaVersion,
			e.UnitCostPaise, e.LineTotalPaise,
			e.SourceGrnID, e.SourcePurchaseReturnID, e.SourceStockTransferOutID, e.SourceStockCountID); err != nil {
			return fmt.Errorf("seeding stock_ledger_entry %s: %w", e.ID, err)
		}
	}

	return nil
}

func parseTime(s string) (time.Time, error) {
	t, err := time.Parse(time.RFC3339, s)
	if err != nil {
		return time.Time{}, fmt.Errorf("parsing timestamp %q: %w", s, err)
	}
	return t.UTC(), nil
}

func parseDate(s string) (time.Time, error) {
	t, err := time.Parse("2006-01-02", s)
	if err != nil {
		return time.Time{}, fmt.Errorf("parsing date %q: %w", s, err)
	}
	return t, nil
}
