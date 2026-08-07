// Sync envelope — wraps every record moving between edge and cloud so the
// sync worker (and drift tests) can enforce the §50.1 authority rule
// mechanically. See docs/domain/SYNC_PROTOCOL.md. Frozen at Milestone 0.5
// (ADR-008).

import { z } from "zod";

export const AggregateTypeSchema = z.enum([
  "order",
  "kot",
  "menu_item",
  "payment",
  // Milestone 1 additions (ADR-011).
  "table_session",
  "app_user",
  "role",
  "restaurant_table",
]);
export type AggregateType = z.infer<typeof AggregateTypeSchema>;

export const SyncDirectionSchema = z.enum(["EDGE_TO_CLOUD", "CLOUD_TO_EDGE"]);
export type SyncDirection = z.infer<typeof SyncDirectionSchema>;

export const SyncStatusSchema = z.enum(["PENDING", "SYNCED", "FAILED"]);
export type SyncStatus = z.infer<typeof SyncStatusSchema>;

// §50.1: operational-transaction aggregates only ever sync edge→cloud;
// catalog/config aggregates only ever sync cloud→edge. This map is the
// single place that rule is encoded for validation.
export const AGGREGATE_AUTHORITY: Record<AggregateType, SyncDirection> = {
  order: "EDGE_TO_CLOUD",
  kot: "EDGE_TO_CLOUD",
  payment: "EDGE_TO_CLOUD",
  table_session: "EDGE_TO_CLOUD", // a seating is an operational transaction
  menu_item: "CLOUD_TO_EDGE",
  app_user: "CLOUD_TO_EDGE",
  role: "CLOUD_TO_EDGE",
  restaurant_table: "CLOUD_TO_EDGE", // the table's definition; its live state is table_session
};

export const SyncEnvelopeSchema = z
  .object({
    record_id: z.string().uuid(), // the wrapped entity's own id (UUIDv7/ULID)
    tenant_id: z.string().uuid(),
    outlet_id: z.string().uuid(),
    device_id: z.string().uuid(),
    aggregate_type: AggregateTypeSchema,
    direction: SyncDirectionSchema,
    created_at: z.string().datetime(),
    updated_at: z.string().datetime(),
    version: z.number().int(), // optimistic concurrency / config version, per-aggregate policy (sync.md §51)
    sync_status: SyncStatusSchema,
    payload: z.unknown(), // typed at the call site as CanonicalOrder | Kot | ...
  })
  .refine((env) => AGGREGATE_AUTHORITY[env.aggregate_type] === env.direction, {
    message: "direction violates the §50.1 authority rule for this aggregate_type",
  });
export type SyncEnvelope = z.infer<typeof SyncEnvelopeSchema>;
