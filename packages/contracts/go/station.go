// Kitchen station contracts — added at 0.3.0 (ADR-014, Milestone 2).
// Mirrors src/types/station.ts.
//
// CONFIG aggregates under §50.1: cloud-owned, synced cloud→edge versioned by
// config_version, replaced wholesale at the edge rather than merged. The live
// ticket at a station is a Kot, which is edge-authoritative — the same
// config/operational split ADR-011 drew between RestaurantTable and
// TableSession.
//
// Field names match sqlite/0005_m2_kitchen_stations_printers.sql and
// postgres/0006_m2_kitchen_stations_printers.sql exactly.
package contracts

type Station struct {
	ID       string `json:"id"`
	OutletID string `json:"outlet_id"`
	// Stable machine code (MAIN_KITCHEN, TANDOOR, BAR, ...), unique per outlet
	// and never globally. Kot.Station stores this code, so renaming Name never
	// orphans a ticket.
	Code          string `json:"code"`
	Name          string `json:"name"`
	SortOrder     int    `json:"sort_order"`
	IsActive      bool   `json:"is_active"`
	ConfigVersion int    `json:"config_version"`
	SchemaVersion int    `json:"schema_version"`
}

// MenuItemStation is a join row, not a station_id column on MenuItem: an item
// may route to more than one station (docs/spec/kitchen.md §Stations), and both
// tickets must print.
type MenuItemStation struct {
	MenuItemID    string `json:"menu_item_id"`
	StationID     string `json:"station_id"`
	ConfigVersion int    `json:"config_version"`
	SchemaVersion int    `json:"schema_version"`
}
