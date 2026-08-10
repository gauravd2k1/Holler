// Kitchen station contracts — added at 0.3.0 (ADR-014, Milestone 2).
//
// CONFIG aggregates under §50.1: the cloud owns them, they sync cloud→edge
// versioned by config_version, and the edge replaces them wholesale rather
// than merging. A station is a production destination (docs/spec/kitchen.md
// §Stations); it is never edge-authoritative, because which stations a kitchen
// has is a management decision, not a shop-floor transaction.
//
// The live ticket at a station is a `kot`, which IS edge-authoritative. That
// split is the same one ADR-011 drew between restaurant_table (config) and
// table_session (operational): no row is half-config, half-transaction.
//
// Field names and types match sqlite/0005_m2_kitchen_stations_printers.sql and
// postgres/0006_m2_kitchen_stations_printers.sql exactly.

import { z } from "zod";

export const StationSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  // Stable machine code (MAIN_KITCHEN, TANDOOR, BAR, ...). Unique per outlet,
  // never globally — two outlets both having a TANDOOR is the normal case.
  // kot.station stores this code, so renaming `name` never orphans a ticket.
  code: z.string().min(1),
  name: z.string().min(1),
  sort_order: z.number().int(),
  is_active: z.boolean(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type Station = z.infer<typeof StationSchema>;

// An item may route to more than one station (docs/spec/kitchen.md §Stations) —
// a thali hits MAIN_KITCHEN and TANDOOR, and both tickets must print. So this
// is a join row, not a `station_id` column on menu_item.
export const MenuItemStationSchema = z.object({
  menu_item_id: z.string().uuid(),
  station_id: z.string().uuid(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type MenuItemStation = z.infer<typeof MenuItemStationSchema>;
