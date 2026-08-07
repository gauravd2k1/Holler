// KOT (Kitchen Order Ticket) — one row per station ticket, not per order.
// See docs/spec/kitchen.md §12. Frozen at Milestone 0.5 (ADR-008).

import { z } from "zod";

export const KotStatusSchema = z.enum([
  "NEW",
  "ACKNOWLEDGED",
  "PREPARING",
  "READY",
  "SERVED",
  "CANCELLED",
]);
export type KotStatus = z.infer<typeof KotStatusSchema>;

export const KotTicketItemSchema = z.object({
  order_item_id: z.string().uuid(),
  name: z.string(),
  quantity: z.number().int().positive(),
  modifiers: z.array(z.string()).default([]),
  notes: z.string().nullable(),
});
export type KotTicketItem = z.infer<typeof KotTicketItemSchema>;

export const KotSchema = z.object({
  id: z.string().uuid(),
  order_id: z.string().uuid(),
  station: z.string(), // MAIN_KITCHEN | TANDOOR | BAR | DESSERT | ... (docs/spec/kitchen.md §Stations)
  sequence: z.number().int().positive(), // 1 for #132, 2 for #132-A, 3 for #132-C
  status: KotStatusSchema,
  items: z.array(KotTicketItemSchema),
  created_by_device_id: z.string().uuid(),
  created_at: z.string().datetime(),
  updated_at: z.string().datetime(),
  schema_version: z.literal(1),
});
export type Kot = z.infer<typeof KotSchema>;
