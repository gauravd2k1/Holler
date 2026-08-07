// CanonicalOrder — the normalized shape every channel (POS, QR, aggregator,
// direct) maps into. See docs/spec/ordering.md §Canonical order model and
// HOLLER_MASTER_PROMPT.md §16.
//
// Frozen at Milestone 0.5 (ADR-008). Money fields are integer paise
// (CLAUDE.md §Money) — never floating point.

import { z } from "zod";

export const OrderTypeSchema = z.enum([
  "DINE_IN",
  "TAKEAWAY",
  "DELIVERY",
  "AGGREGATOR",
  "QR",
  "ROOM_SERVICE",
  "CATERING",
]);
export type OrderType = z.infer<typeof OrderTypeSchema>;

export const OrderSourceSchema = z.enum([
  "POS",
  "QR",
  "AGGREGATOR_ZOMATO",
  "AGGREGATOR_SWIGGY",
  "DIRECT",
]);
export type OrderSource = z.infer<typeof OrderSourceSchema>;

// Order state machine — see docs/domain/ORDER_STATE_MACHINE.md. Do not add
// states here without updating that document and bumping schema_version.
export const OrderStatusSchema = z.enum([
  "DRAFT",
  "CONFIRMED",
  "SENT_TO_KITCHEN",
  "PREPARING",
  "READY",
  "SERVED",
  "BILLED",
  "PAID",
  "CLOSED",
  "CANCELLED",
]);
export type OrderStatus = z.infer<typeof OrderStatusSchema>;

export const OrderItemModifierSchema = z.object({
  modifier_id: z.string().uuid(),
  group_name: z.string(),
  option_name: z.string(),
  price_delta_paise: z.number().int(),
});
export type OrderItemModifier = z.infer<typeof OrderItemModifierSchema>;

export const OrderItemSchema = z.object({
  id: z.string().uuid(),
  menu_item_id: z.string().uuid(),
  variant_id: z.string().uuid().nullable(),
  quantity: z.number().int().positive(),
  unit_price_paise: z.number().int(), // snapshot at order time — never recomputed from live menu
  line_total_paise: z.number().int(),
  modifiers: z.array(OrderItemModifierSchema).default([]),
  notes: z.string().nullable(),
});
export type OrderItem = z.infer<typeof OrderItemSchema>;

export const CanonicalOrderSchema = z.object({
  holler_order_id: z.string().uuid(),
  external_order_id: z.string().nullable(), // aggregator's own order id; null for POS/QR/Direct origin
  source: OrderSourceSchema,
  outlet_id: z.string().uuid(),

  order_type: OrderTypeSchema,
  status: OrderStatusSchema,
  table_id: z.string().uuid().nullable(),

  customer: z
    .object({
      name: z.string().nullable(),
      phone: z.string().nullable(),
    })
    .nullable(),
  delivery_address: z.string().nullable(),

  items: z.array(OrderItemSchema),

  subtotal_paise: z.number().int(),
  discount_paise: z.number().int().default(0),
  packaging_paise: z.number().int().default(0),
  delivery_charge_paise: z.number().int().default(0),
  taxes_paise: z.number().int().default(0),
  aggregator_discount_paise: z.number().int().default(0),
  merchant_discount_paise: z.number().int().default(0),
  total_paise: z.number().int(),

  payment_status: z.enum(["UNPAID", "PARTIALLY_PAID", "PAID", "REFUNDED"]),
  payment_source: z.string().nullable(),

  preparation_time_minutes: z.number().int().nullable(),
  rider: z
    .object({ name: z.string(), phone: z.string(), status: z.string() })
    .nullable(),

  timestamps: z.object({
    created_at: z.string().datetime(),
    confirmed_at: z.string().datetime().nullable(),
    updated_at: z.string().datetime(),
  }),
  source_payload: z.record(z.unknown()).nullable(), // raw external payload, audit only — never parsed as core data

  schema_version: z.literal(1),
});
export type CanonicalOrder = z.infer<typeof CanonicalOrderSchema>;

// OrderCommand set — the only sanctioned way to mutate an order; enforces
// the state machine at the command layer (docs/domain/ORDER_STATE_MACHINE.md).
export const OrderCommandSchema = z.discriminatedUnion("type", [
  z.object({ type: z.literal("CONFIRM_ORDER"), order_id: z.string().uuid() }),
  z.object({ type: z.literal("SEND_TO_KITCHEN"), order_id: z.string().uuid() }),
  z.object({ type: z.literal("MARK_READY"), order_id: z.string().uuid() }),
  z.object({ type: z.literal("MARK_SERVED"), order_id: z.string().uuid() }),
  z.object({ type: z.literal("BILL_ORDER"), order_id: z.string().uuid() }),
  z.object({
    type: z.literal("CANCEL_ORDER"),
    order_id: z.string().uuid(),
    reason: z.string(),
  }),
]);
export type OrderCommand = z.infer<typeof OrderCommandSchema>;
