// Business event payloads for the transactional outbox (ADR-007). Only the
// M0–M2 slice events are defined here (§81 MILESTONE 0.5 scope); the full
// event list in docs/spec/sync.md §Event model is added incrementally as
// each milestone's owning context is built.
//
// Frozen at Milestone 0.5 (ADR-008).

import { z } from "zod";
import { CanonicalOrderSchema } from "./order";
import { KotSchema, KotStatusSchema } from "./kot";
import { TableSessionSchema } from "./table";
import { InvoiceSchema } from "./invoice";
import { CashShiftSchema, PaymentSchema } from "./payment";

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

// Added at 0.4.1 (ADR-016 addendum). Quantity control landed at Milestone 3
// with no event to report a change, so a quantity edited AFTER ItemAdded had
// already synced left the cloud holding a permanently wrong line_total_paise —
// with no later event carrying enough to correct it. §53 forbids financial
// records being silently overwritten, and M3 builds tax and invoicing directly
// on these fields, so an operational-staleness workaround does not suffice.
//
// The payload carries the FULL corrected line, not a quantity delta. A
// delta-only event was considered and REJECTED on §50.1 grounds: it would
// require the cloud to recompute line_total_paise from its own copy of the
// unit price and modifier deltas, making the cloud a second computer of money
// the edge is authoritative for. The edge computes; the cloud stores what it
// is told. Same reasoning that keeps invoice numbering edge-local.
export const ItemQuantityChangedEventSchema = EventEnvelope(
  "ItemQuantityChanged",
  z.object({
    order_id: z.string().uuid(),
    // Self-describing for exactly the reason ItemRemoved is: the cloud must be
    // able to reconcile without replaying every prior event in order.
    item: CanonicalOrderSchema.shape.items.element,
    previous_quantity: z.number().int().positive(),
  }),
);
export type ItemQuantityChangedEvent = z.infer<typeof ItemQuantityChangedEventSchema>;

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

// Added at 0.3.0 (ADR-014). Milestone 2 gives the KOT a lifecycle — NEW →
// ACKNOWLEDGED → PREPARING → READY → SERVED — driven from KDS screens on the
// LAN. Before this, KOTCreated was the only KOT event frozen, so every
// transition after creation was invisible to the cloud: reporting could see
// that a ticket existed and never that the kitchen worked it.
//
// The status is carried with the moment the EDGE recorded it, not the moment
// the cloud received it (§50.1). Kitchen timing analytics are the whole point
// of the event, and an outlet that syncs once an hour would otherwise report
// every ticket as having been prepared in the same instant.
export const KotStatusChangedEventSchema = EventEnvelope(
  // Spelled KOT-, matching its sibling KOTCreated. The identifiers around it
  // read Kot- because that is Go and TypeScript naming; only the wire literal
  // is shouted, and it stays consistent with the event already frozen.
  "KOTStatusChanged",
  z.object({
    kot_id: z.string().uuid(),
    order_id: z.string().uuid(),
    status: KotStatusSchema,
    changed_at: z.string().datetime(),
    changed_by_device_id: z.string().uuid(),
  }),
);
export type KotStatusChangedEvent = z.infer<typeof KotStatusChangedEventSchema>;

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

// Milestone 3 billing events (ADR-016). §53 names InvoiceCreated,
// PaymentReceived and PaymentRefunded as immutable business events; the two
// shift events complete the cash-drawer trail §39 requires.
export const InvoiceCreatedEventSchema = EventEnvelope(
  "InvoiceCreated",
  z.object({ invoice: InvoiceSchema }),
);
export type InvoiceCreatedEvent = z.infer<typeof InvoiceCreatedEventSchema>;

export const PaymentReceivedEventSchema = EventEnvelope(
  "PaymentReceived",
  z.object({ payment: PaymentSchema }),
);
export type PaymentReceivedEvent = z.infer<typeof PaymentReceivedEventSchema>;

// A refund is an appended reversal, never a mutation of the original payment
// (docs/spec/payments.md §Conflict policy). The event carries the reversal row
// and the id of what it reverses.
export const PaymentRefundedEventSchema = EventEnvelope(
  "PaymentRefunded",
  z.object({ payment: PaymentSchema, reverses_payment_id: z.string().uuid() }),
);
export type PaymentRefundedEvent = z.infer<typeof PaymentRefundedEventSchema>;

export const CashShiftOpenedEventSchema = EventEnvelope(
  "CashShiftOpened",
  z.object({ shift: CashShiftSchema }),
);
export type CashShiftOpenedEvent = z.infer<typeof CashShiftOpenedEventSchema>;

export const CashShiftClosedEventSchema = EventEnvelope(
  "CashShiftClosed",
  z.object({ shift: CashShiftSchema }),
);
export type CashShiftClosedEvent = z.infer<typeof CashShiftClosedEventSchema>;

export const OutboxEventSchema = z.discriminatedUnion("event_type", [
  OrderCreatedEventSchema,
  ItemAddedEventSchema,
  ItemRemovedEventSchema,
  ItemQuantityChangedEventSchema,
  OrderConfirmedEventSchema,
  KotCreatedEventSchema,
  KotStatusChangedEventSchema,
  OrderReadyEventSchema,
  SentToKitchenEventSchema,
  OrderCancelledEventSchema,
  TableSessionOpenedEventSchema,
  TableSessionUpdatedEventSchema,
  InvoiceCreatedEventSchema,
  PaymentReceivedEventSchema,
  PaymentRefundedEventSchema,
  CashShiftOpenedEventSchema,
  CashShiftClosedEventSchema,
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
  "ItemQuantityChanged",
  "OrderConfirmed",
  "KOTCreated",
  "KOTStatusChanged",
  "OrderReady",
  "SentToKitchen",
  "OrderCancelled",
  "TableSessionOpened",
  "TableSessionUpdated",
  "InvoiceCreated",
  "PaymentReceived",
  "PaymentRefunded",
  "CashShiftOpened",
  "CashShiftClosed",
] as const;
export type OutboxEventType = (typeof OUTBOX_EVENT_TYPES)[number];
