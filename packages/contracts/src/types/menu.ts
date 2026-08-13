// Menu catalog contracts — added at 0.2.2. Mirrors go/menu.go.
//
// These are CONFIG aggregates under §50.1: the cloud owns them, they sync
// cloud→edge versioned by config_version, and the edge replaces them wholesale
// rather than merging. They already carry their authority direction in
// AGGREGATE_AUTHORITY (menu_item since 0.1.0); this file exists so the POS
// frontend and the Tauri layer share one shape instead of each hand-rolling
// the SQLite column names.
//
// Field names and types match packages/contracts/sqlite/0001_init.sql and
// postgres/0001_init.sql exactly. Money is integer paise, never float.

import { z } from "zod";

export const MenuCategorySchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  name: z.string(),
  sort_order: z.number().int(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type MenuCategory = z.infer<typeof MenuCategorySchema>;

export const MenuItemSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  category_id: z.string().uuid(),
  name: z.string(),
  base_price_paise: z.number().int(), // integer paise (CLAUDE.md); never float
  is_available: z.boolean(), // item-snooze flag, §19
  // Added at 0.4.2 (ADR-016 addendum). Which TaxProfile prices this item.
  //
  // NULL is meaningful: "use the outlet's default profile". That keeps a
  // single-rate restaurant configuration-free rather than making every item
  // name the same profile.
  //
  // This is an INPUT to resolution, never a substitute for the snapshot.
  // Resolution happens at billing time and invoice_line stores what was
  // applied — its own tax_profile_id plus per-component rate_bps and paise.
  // Re-pointing an item at a different profile tomorrow must never alter what
  // a bill issued today says it charged (§31 reproducibility).
  tax_profile_id: z.string().uuid().nullable(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type MenuItem = z.infer<typeof MenuItemSchema>;

export const MenuItemVariantSchema = z.object({
  id: z.string().uuid(),
  menu_item_id: z.string().uuid(),
  name: z.string(),
  price_delta_paise: z.number().int(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type MenuItemVariant = z.infer<typeof MenuItemVariantSchema>;

export const MenuItemModifierSchema = z.object({
  id: z.string().uuid(),
  menu_item_id: z.string().uuid(),
  group_name: z.string(),
  option_name: z.string(),
  price_delta_paise: z.number().int(),
  min_selection: z.number().int(),
  max_selection: z.number().int(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type MenuItemModifier = z.infer<typeof MenuItemModifierSchema>;
