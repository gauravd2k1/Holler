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
  // Milestone 2 additions (ADR-014). Only the two entities are aggregates:
  // menu_item_station and station_printer are routing rows that travel inside
  // their parent's config bundle, exactly as menu_item_variant and
  // menu_item_modifier do. print_job and kot_status_history are deliberately
  // absent — see printer.ts and sqlite/0005 for why.
  "station",
  "printer",
  // Milestone 3 additions (ADR-016). tax_rule, invoice_line,
  // payment_allocation, cash_movement and outlet_fiscal_profile are
  // deliberately absent: they are child rows travelling inside their parent's
  // payload or config bundle, exactly as menu_item_variant and station_printer
  // do. invoice_sequence is absent for the opposite reason — it is edge-local
  // and must never sync, the print_job precedent (see invoice.ts).
  "invoice",
  "cash_shift",
  "tax_profile",
  "compliance_version",
  "invoice_series",
  "discount_definition",
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
  station: "CLOUD_TO_EDGE", // the station's definition; its live ticket is a kot
  printer: "CLOUD_TO_EDGE", // the printer's definition; its live work is a print_job (edge-local)

  // Milestone 3 (ADR-016). The outlet issues bills and takes money with the
  // uplink down, so both are edge-authoritative and the cloud only replays.
  invoice: "EDGE_TO_CLOUD",
  cash_shift: "EDGE_TO_CLOUD",
  // Tax rules, fiscal identity, numbering format and discount policy are
  // management decisions, so they are cloud-owned config. The same cut as
  // station/kot: the series' definition is config, the number it issued lives
  // on an edge-authoritative invoice, and the counter between them never syncs.
  tax_profile: "CLOUD_TO_EDGE",
  compliance_version: "CLOUD_TO_EDGE",
  invoice_series: "CLOUD_TO_EDGE",
  discount_definition: "CLOUD_TO_EDGE",
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
