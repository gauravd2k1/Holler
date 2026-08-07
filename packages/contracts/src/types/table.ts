// Table contracts — Milestone 1 (ADR-011). Mirrors go/table.go.
//
// Authority split (§50.1, no split-authority columns):
//   RestaurantTable  — pure config, cloud→edge, versioned, replaced wholesale.
//   TableSession     — operational, edge→cloud, append-only replay.

import { z } from "zod";

export const RestaurantTableSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  section: z.string(), // floor / zone, e.g. 'GROUND', 'TERRACE'
  label: z.string(), // 'T4', 'G12'
  seat_count: z.number().int().positive(),
  is_active: z.boolean(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type RestaurantTable = z.infer<typeof RestaurantTableSchema>;

// Stored state of an open seating. AVAILABLE is not a stored value — a table
// with no open session IS available (see TableDisplayState below).
export const TableSessionStateSchema = z.enum([
  "OCCUPIED",
  "ORDERED",
  "KOT_SENT",
  "FOOD_READY",
  "BILL_REQUESTED",
  "PAYMENT_PENDING",
  "PAID",
  "DIRTY",
  "CLOSED",
]);
export type TableSessionState = z.infer<typeof TableSessionStateSchema>;

// The floor-plan state docs/spec/tables.md renders: the open session's state,
// or AVAILABLE when there is none. RESERVED is defined by the spec but is not
// produced until reservations land (Milestone 9) — nothing in Milestone 1
// writes it.
export const TableDisplayStateSchema = z.enum([
  "AVAILABLE",
  "RESERVED",
  ...TableSessionStateSchema.options,
]);
export type TableDisplayState = z.infer<typeof TableDisplayStateSchema>;

export const TableSessionSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  table_id: z.string().uuid(),
  state: TableSessionStateSchema,
  current_order_id: z.string().uuid().nullable(),
  guest_count: z.number().int().positive(),
  opened_by_user_id: z.string().uuid().nullable(),
  opened_at: z.string().datetime(),
  closed_at: z.string().datetime().nullable(),
  version: z.number().int(), // optimistic concurrency, sync envelope field
  created_at: z.string().datetime(),
  updated_at: z.string().datetime(),
  schema_version: z.literal(1),
});
export type TableSession = z.infer<typeof TableSessionSchema>;
