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
	ConfigVersion  int    `json:"config_version"`
	SchemaVersion  int    `json:"schema_version"`
}

type MenuItemVariant struct {
	ID              string `json:"id"`
	MenuItemID      string `json:"menu_item_id"`
	Name            string `json:"name"`
	PriceDeltaPaise int64  `json:"price_delta_paise"`
	ConfigVersion   int    `json:"config_version"`
	SchemaVersion   int    `json:"schema_version"`
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
