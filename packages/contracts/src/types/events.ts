// Business event payloads for the transactional outbox (ADR-007). Only the
// M0–M2 slice events are defined here (§81 MILESTONE 0.5 scope); the full
// event list in docs/spec/sync.md §Event model is added incrementally as
// each milestone's owning context is built.
//
// Frozen at Milestone 0.5 (ADR-008).

import { z } from "zod";
import { CanonicalOrderSchema } from "./order";
import { KotSchema } from "./kot";
import { TableSessionSchema } from "./table";

const EventEnvelope = <T extends z.ZodTypeAny>(eventType: string, dataSchema: T) =>
  z.object({
    event_id: z.string().uuid(),
    event_type: z.literal(eventType),
    occurred_at: z.string().datetime(),
    outlet_id: z.string().uuid(),
    schema_version: z.literal(1),
    data: dataSchema,
  });

export const OrderCreatedEventSchema = EventEnvelope(
  "OrderCreated",
  z.object({ order: CanonicalOrderSchema }),
);
export type OrderCreatedEvent = z.infer<typeof OrderCreatedEventSchema>;

export const ItemAddedEventSchema = EventEnvelope(
  "ItemAdded",
  z.object({
    order_id: z.string().uuid(),
    item: CanonicalOrderSchema.shape.items.element,
  }),
);
export type ItemAddedEvent = z.infer<typeof ItemAddedEventSchema>;

// Added at 0.2.3. docs/spec/sync.md §Event model always listed ItemRemoved; it
// was simply never frozen here, which left the edge's line-removal path
// caller-described while the add path was hardened. The full item travels in
// the payload, not just an id: once the row is deleted the cloud has no way to
// look up what left the order, so the event must be self-describing.
export const ItemRemovedEventSchema = EventEnvelope(
  "ItemRemoved",
  z.object({
    order_id: z.string().uuid(),
    item: CanonicalOrderSchema.shape.items.element,
  }),
);
export type ItemRemovedEvent = z.infer<typeof ItemRemovedEventSchema>;

// Added at 0.2.5. The cashier confirming a draft — DRAFT→CONFIRMED in the
// order state machine. Deliberately NOT named OrderAccepted: docs/spec/sync.md
// lists that name, but in the aggregator context acceptance means a merchant
// accepting an inbound order, which is a different business event. When
// Milestone 6 lands, AcceptOrder on the AggregatorProvider interface gets its
// own event rather than sharing this one.
export const OrderConfirmedEventSchema = EventEnvelope(
  "OrderConfirmed",
  z.object({
    order_id: z.string().uuid(),
    // The edge is authoritative for order transactions (§50.1), so this is the
    // moment the edge recorded, not the moment the cloud received it.
    confirmed_at: z.string().datetime(),
  }),
);
export type OrderConfirmedEvent = z.infer<typeof OrderConfirmedEventSchema>;

export const KotCreatedEventSchema = EventEnvelope("KOTCreated", z.object({ kot: KotSchema }));
export type KotCreatedEvent = z.infer<typeof KotCreatedEventSchema>;

export const OrderReadyEventSchema = EventEnvelope(
  "OrderReady",
  z.object({ order_id: z.string().uuid() }),
);
export type OrderReadyEvent = z.infer<typeof OrderReadyEventSchema>;

// Added at 0.2.2. The edge sync worker needed these to replay send-to-kitchen,
// cancellation and table seatings, and coined the strings locally because the
// contract had none — a de-facto unfrozen contract. These schemas use those
// exact strings, so freezing them required no edge change.
export const SentToKitchenEventSchema = EventEnvelope(
  "SentToKitchen",
  z.object({ order_id: z.string().uuid() }),
);
export type SentToKitchenEvent = z.infer<typeof SentToKitchenEventSchema>;

export const OrderCancelledEventSchema = EventEnvelope(
  "OrderCancelled",
  z.object({ order_id: z.string().uuid(), reason: z.string() }),
);
export type OrderCancelledEvent = z.infer<typeof OrderCancelledEventSchema>;

export const TableSessionOpenedEventSchema = EventEnvelope(
  "TableSessionOpened",
  z.object({ session: TableSessionSchema }),
);
export type TableSessionOpenedEvent = z.infer<typeof TableSessionOpenedEventSchema>;

export const TableSessionUpdatedEventSchema = EventEnvelope(
  "TableSessionUpdated",
  z.object({ session: TableSessionSchema }),
);
export type TableSessionUpdatedEvent = z.infer<typeof TableSessionUpdatedEventSchema>;

export const OutboxEventSchema = z.discriminatedUnion("event_type", [
  OrderCreatedEventSchema,
  ItemAddedEventSchema,
  ItemRemovedEventSchema,
  OrderConfirmedEventSchema,
  KotCreatedEventSchema,
  OrderReadyEventSchema,
  SentToKitchenEventSchema,
  OrderCancelledEventSchema,
  TableSessionOpenedEventSchema,
  TableSessionUpdatedEventSchema,
]);
export type OutboxEvent = z.infer<typeof OutboxEventSchema>;

// The authoritative event_type string list. Go mirrors it as OutboxEventTypes
// and a drift test asserts the two are identical. The Rust edge crates cannot
// import this (no Rust binding yet — deferred until a fourth Rust consumer
// exists, ADR-011 addendum), so scripts/check-event-type-drift.mjs greps their
// literals against this list in both directions instead.
export const OUTBOX_EVENT_TYPES = [
  "OrderCreated",
  "ItemAdded",
  "ItemRemoved",
  "OrderConfirmed",
  "KOTCreated",
  "OrderReady",
  "SentToKitchen",
  "OrderCancelled",
  "TableSessionOpened",
  "TableSessionUpdated",
] as const;
export type OutboxEventType = (typeof OUTBOX_EVENT_TYPES)[number];
