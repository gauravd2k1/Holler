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
  // Milestone 4 additions (ADR-018). item_unit_conversion, recipe_ingredient,
  // modifier_ingredient_delta and stock_count_line are deliberately absent —
  // child rows travelling inside their parent's payload or config bundle.
  // stock_balance_snapshot is absent for the invoice_sequence reason: it is an
  // edge-local derived projection and must never sync. The cloud may re-derive
  // its own stock view by summing the ledger; it may never mirror the edge's.
  "inventory_item",
  "recipe",
  "stock_ledger_entry",
  "stock_count",
  "stock_deduction_gap",
  // Milestone 5 additions (ADR-019). supplier_item, purchase_order_line,
  // grn_line, purchase_return_line and stock_transfer_line are deliberately
  // absent — child rows travelling inside their parent's payload or config
  // bundle, the menu_item_variant / station_printer / invoice_line precedent.
  //
  // grn_sequence is absent for the invoice_sequence reason: edge-local, and
  // mirroring a counter would make the cloud a second minter of a number the
  // outlet issues (§33).
  //
  // supplier_invoice and supplier_credit are absent for the refresh_token /
  // device_credential reason: cloud-only. An outlet does not reconcile a
  // supplier ledger with the uplink down, and an edge copy would be a second
  // authority over money owed.
  "supplier",
  "purchase_order",
  "goods_receipt_note",
  "grn_gap",
  "purchase_return",
  "stock_transfer_out",
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
  // Milestone 4 (ADR-018). Same cut as every milestone before it: a raw
  // material's definition and a recipe are management decisions, while
  // consuming, wasting and counting stock are shop-floor transactions the
  // outlet performs with the uplink down.
  inventory_item: "CLOUD_TO_EDGE",
  recipe: "CLOUD_TO_EDGE",
  stock_ledger_entry: "EDGE_TO_CLOUD",
  stock_count: "EDGE_TO_CLOUD",
  // A signal, not a correction — and cloud-visible because the person who can
  // see it and the person who can fix it are different people in different
  // places. Shares the ledger ingest route rather than taking its own.
  stock_deduction_gap: "EDGE_TO_CLOUD",

  // Milestone 5 (ADR-019). The same cut every milestone before it has made:
  // who we buy from and what we ordered are management decisions; what
  // physically arrived at the door, what went back, and what was dispatched
  // are shop-floor transactions the outlet performs with the uplink down.
  supplier: "CLOUD_TO_EDGE",
  // NO RECEIPT STATE on this aggregate — receipt progress is derived on both
  // sides and the two derivations legitimately differ (the edge sees only its
  // own GRN lines). Storing it would make the outlet a second writer of a
  // cloud row. See postgres/0028 and ADR-019.
  purchase_order: "CLOUD_TO_EDGE",
  // THE OUTLET RECEIVES GOODS WITH THE UPLINK DOWN and the cloud only
  // replays — the invoice / payment / cash_shift split exactly.
  goods_receipt_note: "EDGE_TO_CLOUD",
  // A signal, not a correction, and cloud-visible because the person who can
  // see it and the person who can fix it are different people in different
  // places — the stock_deduction_gap argument, inbound. Plain envelope
  // outbox, NOT a ranged stream: a gap is a discrete event a buyer acts on,
  // not a per-sale row arriving all day.
  grn_gap: "EDGE_TO_CLOUD",
  purchase_return: "EDGE_TO_CLOUD",
  // Outbound half only. TRANSFER_IN and goods-in-transit are M8: a transfer
  // spans two edge databases, which is multi-outlet machinery.
  stock_transfer_out: "EDGE_TO_CLOUD",
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
