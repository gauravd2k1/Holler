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

// Channel of origin. Mirrors the CHECK on `order.source` in both stores; the
// two must agree member for member and `scripts/check-order-source-drift.mjs`
// fails the build when they do not.
//
// AGGREGATOR_ZOMATO and AGGREGATOR_SWIGGY are DEPRECATED (ADR-026): they name a
// platform in a schema that is otherwise platform-agnostic. Neither has ever
// been written. They stay because removing a CHECK member is breaking; the
// removal trigger is the next breaking contracts bump.
//
// AGGREGATOR and TABLE_TAB are the 0.8.1 additions and NOTHING WRITES EITHER
// YET — the aggregator accept path still writes DIRECT, and the table device
// (ADR-025) has no code at all.
export const OrderSourceSchema = z.enum([
  "POS",
  "QR",
  /** @deprecated ADR-026 — use "AGGREGATOR"; removal at the next breaking bump. */
  "AGGREGATOR_ZOMATO",
  /** @deprecated ADR-026 — use "AGGREGATOR"; removal at the next breaking bump. */
  "AGGREGATOR_SWIGGY",
  "DIRECT",
  "AGGREGATOR",
  "TABLE_TAB",
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

// THE STATUSES IN WHICH AN ORDER'S LINES MAY STILL BE AMENDED. Added at
// 0.8.3 (ADR-028).
//
// Adding a line to an order that has already gone to the kitchen is the
// normal way a restaurant works — a table orders another round, a waiter
// sends a starter and a main separately — and the edge has allowed it since
// `#132-A` (docs/spec/kitchen.md). The cloud allowed it only in DRAFT and
// refused everything else with a permanent 409, which under an at-least-once
// outbox means the order's whole queue wedges behind the refusal.
//
// **THIS SET IS DECLARED ONCE AND CONSUMED, NEVER RESTATED.** It had three
// independent declarations — a Rust match arm at the edge, a Go equality test
// in the cloud, and one word of English in openapi.yaml — and they disagreed
// for months without anything going red, because the defect that hid it
// (orders stuck in DRAFT cloud-side, fixed the same day) meant the cloud rule
// was never reached. `scripts/check-order-amendable-drift.mjs` now fails the
// build if any surface disagrees with this array.
//
// The edge is the authority for order transactions (§50.1), so this is the
// EDGE's rule written down, not a negotiated middle: the cloud replays what
// the outlet did and does not get an amendable set of its own.
//
// READY, SERVED, BILLED, PAID, CLOSED and CANCELLED are deliberately absent.
// By then the correction belongs to a new order or an explicit reopen, not a
// silent edit of a bill already on its way to the guest.
export const ORDER_ITEM_AMENDABLE_STATUSES = [
  "DRAFT",
  "CONFIRMED",
  "SENT_TO_KITCHEN",
  "PREPARING",
] as const satisfies readonly OrderStatus[];

/** True when `status` still permits a line to be added, removed or resized. */
export function isOrderItemAmendable(status: OrderStatus): boolean {
  return (ORDER_ITEM_AMENDABLE_STATUSES as readonly OrderStatus[]).includes(status);
}

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

  // Short human-facing number ('#A184'), minted edge-side alongside the order.
  // Added at 0.4.0 (ADR-016) to close the M2 finding that a printed KOT
  // carried the raw UUID — a cook cannot read one aloud across a kitchen.
  // CLAUDE.md §Money/time/identifiers requires human-facing numbers be short
  // and forbids exposing sequential PKs as security identifiers, so this is a
  // display string, never a key.
  //
  // Nullable ONLY for rows written before 0.4.0: SQLite cannot add a NOT NULL
  // column to a populated table without rebuilding "order", and a rebuild of
  // a table that order_item, kot and invoice all reference is the worse risk.
  // Every create path populates it; readers fall back to the id for legacy
  // rows alone.
  display_number: z.string().nullable(),

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

  // Milestone 3 additions (ADR-016).
  //
  // SET_ORDER_ITEM_QUANTITY is a single command by design. Quantity must NOT
  // be implemented as remove-then-add: that is two durable writes with a crash
  // window between them, which is precisely the loss the durable-cart work
  // eliminated (docs/backlog-m2.md, docs/retro.md 2026-08-10). One command,
  // one write.
  z.object({
    type: z.literal("SET_ORDER_ITEM_QUANTITY"),
    order_id: z.string().uuid(),
    order_item_id: z.string().uuid(),
    quantity: z.number().int().positive(),
  }),
  z.object({
    type: z.literal("APPLY_DISCOUNT"),
    order_id: z.string().uuid(),
    discount_definition_id: z.string().uuid(),
    // LINE scope targets one line; BILL scope leaves this null.
    order_item_id: z.string().uuid().nullable(),
    // Required when the discount definition sets requires_reason.
    reason: z.string().nullable(),
  }),
  z.object({
    type: z.literal("PAY_ORDER"),
    order_id: z.string().uuid(),
  }),
  z.object({
    type: z.literal("CLOSE_ORDER"),
    order_id: z.string().uuid(),
  }),
]);
export type OrderCommand = z.infer<typeof OrderCommandSchema>;
