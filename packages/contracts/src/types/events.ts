// Business event payloads for the transactional outbox (ADR-007). Only the
// M0–M2 slice events are defined here (§81 MILESTONE 0.5 scope); the full
// event list in docs/spec/sync.md §Event model is added incrementally as
// each milestone's owning context is built.
//
// Frozen at Milestone 0.5 (ADR-008).

import { z } from "zod";
import { CanonicalOrderSchema } from "./order";
import { KotSchema } from "./kot";

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

export const KotCreatedEventSchema = EventEnvelope("KOTCreated", z.object({ kot: KotSchema }));
export type KotCreatedEvent = z.infer<typeof KotCreatedEventSchema>;

export const OrderReadyEventSchema = EventEnvelope(
  "OrderReady",
  z.object({ order_id: z.string().uuid() }),
);
export type OrderReadyEvent = z.infer<typeof OrderReadyEventSchema>;

export const OutboxEventSchema = z.discriminatedUnion("event_type", [
  OrderCreatedEventSchema,
  ItemAddedEventSchema,
  KotCreatedEventSchema,
  OrderReadyEventSchema,
]);
export type OutboxEvent = z.infer<typeof OutboxEventSchema>;
