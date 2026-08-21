// Menu catalog contracts — added at 0.2.2. Mirrors src/types/menu.ts.
//
// CONFIG aggregates under §50.1: cloud-owned, synced cloud→edge versioned by
// config_version, replaced wholesale at the edge rather than merged.
// Field names match sqlite/0001_init.sql and postgres/0001_init.sql exactly.
// Money is integer paise, never float.
package contracts

type MenuCategory struct {
	ID            string `json:"id"`
	OutletID      string `json:"outlet_id"`
	Name          string `json:"name"`
	SortOrder     int    `json:"sort_order"`
	ConfigVersion int    `json:"config_version"`
	SchemaVersion int    `json:"schema_version"`
}

type MenuItem struct {
	ID             string `json:"id"`
	OutletID       string `json:"outlet_id"`
	CategoryID     string `json:"category_id"`
	Name           string `json:"name"`
	BasePricePaise int64  `json:"base_price_paise"`
	IsAvailable    bool   `json:"is_available"`
	// TaxProfileID added at 0.4.2 (ADR-016 addendum). Which TaxProfile prices
	// this item; nil means "use the outlet's default profile".
	//
	// An INPUT to resolution, never a substitute for the snapshot: resolution
	// happens at billing time and InvoiceLine stores what was applied (its own
	// TaxProfileID plus per-component rate_bps and paise), so re-pointing an
	// item tomorrow never rewrites a bill issued today (§31).
	TaxProfileID *string `json:"tax_profile_id"`
	// HSNSAC added at 0.4.5 (ADR-016 addendum). The HSN (goods) or SAC
	// (services) code a GST tax invoice must print for this item's lines.
	//
	// On MenuItem rather than TaxProfile because HSN/SAC classifies what is
	// sold while a profile classifies how it is rated — prepared food
	// (SAC 9963) and packaged water (HSN 2201) routinely share one 5%
	// profile, so hanging the code off the profile would force them to
	// share a code.
	//
	// Nil-able, and not a licence to print a blank code: a default of
	// "9963" for everything would stamp a plausible, wrong, legally
	// meaningful code on packaged goods, and a wrong HSN is worse than a
	// missing one because it looks configured. The completeness rule lives
	// at invoice-issue time, which must reject a NULL line and name the
	// item.
	//
	// An INPUT to resolution: InvoiceLine.HSNSAC stores what was applied,
	// so a catalogue correction never rewrites an issued bill (§31).
	HSNSAC        *string `json:"hsn_sac"`
	ConfigVersion int     `json:"config_version"`
	SchemaVersion int     `json:"schema_version"`
}

type MenuItemVariant struct {
	ID              string `json:"id"`
	MenuItemID      string `json:"menu_item_id"`
	Name            string `json:"name"`
	PriceDeltaPaise int64  `json:"price_delta_paise"`
	// 0.5.7. The column landed at 0.5.0 (sqlite/0014, ADR-018 §2.1) and the
	// wire types never got it -- the additive-consumer-list rule's failure
	// mode, predating the rule. Without it a default variant cannot sync, and
	// order lines at a cloud-synced outlet cannot stamp one.
	IsDefault     bool `json:"is_default"`
	ConfigVersion int  `json:"config_version"`
	SchemaVersion int  `json:"schema_version"`
}

type MenuItemModifier struct {
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
