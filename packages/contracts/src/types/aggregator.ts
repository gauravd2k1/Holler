import { z } from "zod";

/**
 * M6 Phase C, contracts 0.8.0 (ADR-022 ACCEPTED 2026-09-08).
 *
 * TWO AGGREGATES, NOT ONE. `aggregator_order` is the inbound DOCUMENT from an
 * external platform: CLOUD-AUTHORITATIVE, syncing DOWN, replaced wholesale at a
 * newer `document_version` and never field-merged. The local `order` created
 * from it stays EDGE-AUTHORITATIVE and syncs up exactly as every other order
 * does, linked by `external_order_id`.
 *
 * The guarantee that falls out of the split, and it is published: A NEW
 * AGGREGATOR ORDER CANNOT ARRIVE WHILE THE UPLINK IS DOWN; ONE THAT HAS ALREADY
 * ARRIVED IS FULLY OPERABLE OFFLINE.
 *
 * NOTHING IN THIS FILE NAMES A PLATFORM. `platform` is a string carrying data,
 * not a union carrying code — a new platform must not require a contract
 * change, and the drift check keeps platform vocabulary inside its own adapter
 * module.
 */

export const AggregatorOrderLineSchema = z.object({
  id: z.string().uuid(),
  aggregator_order_id: z.string().uuid(),
  line_number: z.number().int().positive(),

  // What the platform called it, kept whatever else happens. When the mapping
  // fails this is the only description of what the customer actually ordered.
  external_item_id: z.string(),
  external_item_name: z.string(),

  // NULLABLE, AND THE NULL IS LOAD-BEARING. A line that cannot be mapped to a
  // menu item is RECORDED, NOT REFUSED (ADR-022 rule 4, the grn_gap
  // precedent): refusing a delivery order that is already cooking is the
  // outage, not the protection.
  menu_item_id: z.string().uuid().nullable(),

  quantity: z.number().int().positive(),
  // Integer paise, like every other money field. Nullable because not every
  // inbound shape prices its lines.
  stated_unit_price_paise: z.number().int().nonnegative().nullable(),
  schema_version: z.literal(1),
});
export type AggregatorOrderLine = z.infer<typeof AggregatorOrderLineSchema>;

export const AggregatorOrderSchema = z.object({
  id: z.string().uuid(),
  tenant_id: z.string().uuid(),
  outlet_id: z.string().uuid(),

  // Data, not code. See the file header.
  platform: z.string().min(1),
  // Tenant- and platform-scoped, never global: two platforms can and do issue
  // the same id.
  external_order_id: z.string().min(1),

  // The platform's own state string, verbatim and unmapped. A platform status
  // NEVER writes `order.status` — one writer, as ADR-014 requires for
  // `kot.status`. Kept as the platform's own string so a status we have never
  // seen is recorded rather than coerced into the nearest local one.
  platform_status: z.string().min(1),

  // Replace-not-merge compares on this.
  document_version: z.number().int().positive(),

  // The raw inbound payload, kept whole: the record of what an external system
  // actually asked for. Storing only our parse of it makes a mapping bug
  // unfalsifiable after the fact.
  raw_payload: z.unknown(),

  stated_total_paise: z.number().int().nonnegative().nullable(),

  received_at: z.string(),
  business_date: z.string(),

  // NULL means "arrived, not yet accepted" — a visible operational state, not a
  // missing value. Creation of the local order is OPERATOR-CONFIRMED, never
  // automatic (ADR-022 addendum §2).
  accepted_at: z.string().nullable(),
  local_order_id: z.string().uuid().nullable(),

  lines: z.array(AggregatorOrderLineSchema),
  schema_version: z.literal(1),
});
export type AggregatorOrder = z.infer<typeof AggregatorOrderSchema>;
