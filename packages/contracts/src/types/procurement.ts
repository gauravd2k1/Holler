// Procurement — suppliers, purchase orders, goods receipt, returns and the
// outbound half of inter-outlet transfer. Contracts 0.6.0, ADR-019.
//
// Mirrored in go/procurement.go; the drift suites compare the two.
//
// ---------------------------------------------------------------------------
// A GRN NEVER BLOCKS ON A PURCHASE ORDER.
// ---------------------------------------------------------------------------
//
// purchase_order_id, supplier_id and purchase_order_line_id are all
// `.nullable()` here for the same reason they are nullable in both stores:
// goods arrive against a PO that never synced, against a PO amended after
// dispatch, and with no PO at all. Each case records a GrnGap and ACCEPTS THE
// RECEIPT. This is M4's "stock never blocks a sale" generalised to the inbound
// side, and it is the load-bearing rule of this version.
//
// Note for anyone tightening these types later: Zod `.nullable()` is not
// `.optional()`. A missing key fails `.parse` exactly like a wrong type, which
// is what caught the Tauri DTO drift on 2026-08-27. Keep the key, keep the null.
//
// ---------------------------------------------------------------------------
// QUANTITIES ARE INTEGER MICRO-UNITS; MONEY IS INTEGER PAISE.
// ---------------------------------------------------------------------------
//
// 0015's rule: canonical unit of the dimension x 10^6, scale carried in the
// field name. The binding range limit is JavaScript's 2^53, not i64, because
// these cross the wire as `number`. A 50 kg sack is 5e10 micro-grams.

import { z } from "zod";

// The three dimensions, identical to inventory's. Re-declared rather than
// imported so this file's CHECK constraints and its schema cannot drift apart
// silently — the same reason recipe_ingredient.quantity_dimension exists.
export const QuantityDimensionSchema = z.enum(["MASS", "VOLUME", "COUNT"]);
export type QuantityDimension = z.infer<typeof QuantityDimensionSchema>;

// ---------------------------------------------------------------------------
// supplier — AGGREGATE, cloud->edge
// ---------------------------------------------------------------------------
export const SupplierSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  code: z.string().min(1),
  name: z.string().min(1),
  // Nullable because unregistered suppliers are ordinary in this market. No
  // fallback and no synthesised placeholder, for the reason menu_item.hsn_sac
  // has none: a wrong code that looks configured is worse than a missing one.
  gstin: z.string().nullable(),
  phone: z.string().nullable(),
  email: z.string().nullable(),
  address: z.string().nullable(),
  payment_terms_days: z.number().int().nonnegative(),
  is_active: z.boolean(),
  config_version: z.number().int(),
  created_at: z.string().datetime(),
  updated_at: z.string().datetime(),
  schema_version: z.literal(1),
});
export type Supplier = z.infer<typeof SupplierSchema>;

// ---------------------------------------------------------------------------
// supplier_item — CHILD ROW of supplier. Not an aggregate.
// ---------------------------------------------------------------------------
export const SupplierItemSchema = z.object({
  id: z.string().uuid(),
  supplier_id: z.string().uuid(),
  inventory_item_id: z.string().uuid(),
  // The supplier's own unit, off their delivery note: 'SACK', 'CRATE', 'TIN'.
  // Free text on purpose — it is their label, not an enum this product owns.
  purchase_unit: z.string().min(1),
  // Base-dimension quantity in one purchase_unit. One 50 kg sack = 5e10.
  pack_size_micro: z.number().int().positive(),

  // THE UNIT THE AUTHOR CHOSE, NEVER DERIVED FROM THE REFERENT.
  //
  // Contracts 0.5.2's rule on the purchase side. Without it pack_size_micro is
  // dimensionless in storage, and reclassifying an inventory_item from MASS to
  // COUNT silently reinterprets every pack size against it.
  //
  // IF A WRITE PATH OR UI AUTO-FILLS THIS FROM inventory_item.dimension, THE
  // CLOUD'S COMPARISON BECOMES x == x AND THE GUARD CAN NEVER FIRE — and it
  // will look correct in review. That is this column's only real risk.
  quantity_dimension: QuantityDimensionSchema,

  // Advisory only: prefills a PO line, never the price a GRN posts. What was
  // invoiced is a fact; what was expected is a guess.
  last_price_paise: z.number().int().nonnegative().nullable(),
  is_preferred: z.boolean(),
  schema_version: z.literal(1),
});
export type SupplierItem = z.infer<typeof SupplierItemSchema>;

// ---------------------------------------------------------------------------
// purchase_order — AGGREGATE, cloud->edge. Read-only at the edge.
// ---------------------------------------------------------------------------
//
// NO RECEIPT STATE. PARTIALLY_RECEIVED and CLOSED-on-receipt are deliberately
// absent: receiving happens at the edge, this is a cloud-owned config row, and
// a receipt-driven status would make the outlet a second writer of a cloud
// aggregate (§50.1, ADR-011). Receipt progress is DERIVED on both sides, and
// THE TWO DERIVATIONS LEGITIMATELY DIFFER — the edge sees only its own GRN
// lines, the cloud sees every outlet's. Show both and label them; never
// reconcile them. Full reasoning in postgres/0028 and ADR-019.
export const PurchaseOrderStatusSchema = z.enum([
  "DRAFT",
  "PENDING_APPROVAL",
  "APPROVED",
  "SENT",
  "CANCELLED",
  "CLOSED",
]);
export type PurchaseOrderStatus = z.infer<typeof PurchaseOrderStatusSchema>;

export const PurchaseOrderLineSchema = z.object({
  id: z.string().uuid(),
  purchase_order_id: z.string().uuid(),
  inventory_item_id: z.string().uuid(),
  line_number: z.number().int().positive(),
  purchase_unit: z.string().min(1),
  ordered_quantity_micro: z.number().int().positive(),
  // 0.5.2's rule. Never auto-filled from the referent.
  quantity_dimension: QuantityDimensionSchema,
  unit_price_paise: z.number().int().nonnegative(),
  line_total_paise: z.number().int().nonnegative(),
});
export type PurchaseOrderLine = z.infer<typeof PurchaseOrderLineSchema>;

export const PurchaseOrderSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  supplier_id: z.string().uuid(),
  po_number: z.string().min(1),
  status: PurchaseOrderStatusSchema,
  expected_date: z.string().nullable(),
  notes: z.string().nullable(),
  total_paise: z.number().int().nonnegative(),
  // Both null or both set — an approval is whole or it did not happen, which
  // is what makes "who authorised this spend" answerable.
  approved_by_user_id: z.string().uuid().nullable(),
  approved_at: z.string().datetime().nullable(),
  created_at: z.string().datetime(),
  config_version: z.number().int(),
  lines: z.array(PurchaseOrderLineSchema),
  schema_version: z.literal(1),
});
export type PurchaseOrder = z.infer<typeof PurchaseOrderSchema>;

// ---------------------------------------------------------------------------
// goods_receipt_note — AGGREGATE, edge->cloud. IMMUTABLE.
// ---------------------------------------------------------------------------
export const GrnLineSchema = z.object({
  id: z.string().uuid(),
  grn_id: z.string().uuid(),
  inventory_item_id: z.string().uuid(),
  line_number: z.number().int().positive(),
  // NULLABLE: a line with no matching PO line is received and gapped.
  purchase_order_line_id: z.string().uuid().nullable(),

  // BOTH SIDES OF THE CONVERSION ARE STORED, and this is not redundancy.
  // entered_* is what the human typed; base_quantity_micro is what the ledger
  // receives. When a receipt turns out to be 1000x wrong, "what did they
  // actually type?" must be answerable from the row, not reconstructed from a
  // pack size that may since have been edited.
  entered_purchase_unit: z.string().min(1),
  entered_quantity_micro: z.number().int().positive(),
  quantity_dimension: QuantityDimensionSchema,
  base_quantity_micro: z.number().int().positive(),
  // The rate actually applied, snapshotted, so this receipt's arithmetic stays
  // reproducible after supplier_item is edited.
  pack_size_micro_applied: z.number().int().positive(),

  // Cost per BASE unit. The field that finally consumes
  // stock_ledger_entry.unit_cost_paise, which ADR-018 deferred to exactly here.
  unit_cost_paise: z.number().int().nonnegative(),
  line_total_paise: z.number().int().nonnegative(),

  // MODELLED NOW, ALERTED IN M6. Batch identity is captured at receipt or
  // never — you cannot retrofit which crate a chicken came out of. Both are
  // EXEMPT in scripts/check-contract-field-consumers.mjs with M6 named, and
  // BOTH EXEMPTIONS COME OUT when M6's expiry alerting lands.
  batch_code: z.string().nullable(),
  expiry_date: z.string().nullable(),
});
export type GrnLine = z.infer<typeof GrnLineSchema>;

export const GoodsReceiptNoteSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  // NULLABLE, AND THIS IS THE POINT. See the file header.
  purchase_order_id: z.string().uuid().nullable(),
  supplier_id: z.string().uuid().nullable(),
  grn_number: z.string().min(1),
  // The supplier's own reference off the delivery note. No format assumed and
  // no uniqueness enforced — it is their number, not ours.
  delivery_note_ref: z.string().nullable(),
  received_at: z.string().datetime(),
  received_by_user_id: z.string().uuid(),
  // Outlet-local business day from compute_business_date(), NOT the first ten
  // characters of a UTC instant. See docs/m5-planning.md §1.2 for what that
  // shortcut costs on the billing side.
  business_date: z.string(),
  notes: z.string().nullable(),
  lines: z.array(GrnLineSchema),
  schema_version: z.literal(1),
});
export type GoodsReceiptNote = z.infer<typeof GoodsReceiptNoteSchema>;

// ---------------------------------------------------------------------------
// grn_gap — AGGREGATE, edge->cloud. The other half of "never blocks".
// ---------------------------------------------------------------------------
//
// PLAIN ENVELOPE OUTBOX, deliberately not a ranged stream: no entry_seq, no
// counter, no cursor, no contiguity check. A grn_gap is a discrete event a
// buyer acts on, not a per-sale row arriving all day like
// stock_deduction_gap. Full reasoning in sqlite/0027.
export const GrnGapReasonSchema = z.enum([
  "NO_PURCHASE_ORDER", // received with no PO at all
  "PURCHASE_ORDER_NOT_FOUND", // PO referenced but never synced to this edge
  "PO_LINE_NOT_FOUND", // item received that the PO does not list
  "QUANTITY_EXCEEDS_ORDERED", // over-delivery; accepted, flagged
  "NO_SUPPLIER_ITEM", // no supplier_item row for this item + unit
  "NO_UNIT_CONVERSION", // purchase unit not convertible to base
  "DIMENSION_MISMATCH", // entered dimension != inventory_item.dimension
  "SUPPLIER_NOT_FOUND", // delivery from an unconfigured supplier
]);
export type GrnGapReason = z.infer<typeof GrnGapReasonSchema>;

export const GrnGapSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  grn_id: z.string().uuid(),
  grn_line_id: z.string().uuid().nullable(),
  inventory_item_id: z.string().uuid().nullable(),
  reason: GrnGapReasonSchema,
  // Human-readable, and it is read by a human: M5 acceptance criterion 3
  // requires the gap VISIBLE ON THE POS, not merely present in a table.
  detail: z.string().nullable(),
  occurred_at: z.string().datetime(),
  business_date: z.string(),
  schema_version: z.literal(1),
});
export type GrnGap = z.infer<typeof GrnGapSchema>;

// ---------------------------------------------------------------------------
// purchase_return — AGGREGATE, edge->cloud. IMMUTABLE. Posts RETURN_TO_VENDOR.
// ---------------------------------------------------------------------------
export const PurchaseReturnReasonSchema = z.enum([
  "DAMAGED",
  "EXPIRED",
  "WRONG_ITEM",
  "QUALITY",
  "OVER_DELIVERY",
  "OTHER",
]);
export type PurchaseReturnReason = z.infer<typeof PurchaseReturnReasonSchema>;

export const PurchaseReturnLineSchema = z.object({
  id: z.string().uuid(),
  purchase_return_id: z.string().uuid(),
  inventory_item_id: z.string().uuid(),
  grn_line_id: z.string().uuid().nullable(),
  line_number: z.number().int().positive(),
  entered_purchase_unit: z.string().min(1),
  entered_quantity_micro: z.number().int().positive(),
  quantity_dimension: QuantityDimensionSchema,
  base_quantity_micro: z.number().int().positive(),
  unit_cost_paise: z.number().int().nonnegative(),
});
export type PurchaseReturnLine = z.infer<typeof PurchaseReturnLineSchema>;

export const PurchaseReturnSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  supplier_id: z.string().uuid().nullable(),
  grn_id: z.string().uuid().nullable(),
  return_number: z.string().min(1),
  reason: PurchaseReturnReasonSchema,
  returned_at: z.string().datetime(),
  returned_by_user_id: z.string().uuid(),
  business_date: z.string(),
  notes: z.string().nullable(),
  lines: z.array(PurchaseReturnLineSchema),
  schema_version: z.literal(1),
});
export type PurchaseReturn = z.infer<typeof PurchaseReturnSchema>;

// ---------------------------------------------------------------------------
// stock_transfer_out — AGGREGATE, edge->cloud. OUTBOUND HALF ONLY (M5).
// ---------------------------------------------------------------------------
//
// Posts TRANSFER_OUT at the SOURCE outlet. The destination receipt
// (TRANSFER_IN) and goods-in-transit reconciliation are M8, with multi-outlet:
// a transfer spans two edge databases, which is not something to half-build
// here. destination_outlet_id is recorded now so M8 has its link and no
// migration has to find it later; the cloud reads it in M5 for the transfer
// list, so it is not an unconsumed field.
export const StockTransferLineSchema = z.object({
  id: z.string().uuid(),
  stock_transfer_out_id: z.string().uuid(),
  inventory_item_id: z.string().uuid(),
  line_number: z.number().int().positive(),
  base_quantity_micro: z.number().int().positive(),
  quantity_dimension: QuantityDimensionSchema,
  unit_cost_paise: z.number().int().nonnegative(),
});
export type StockTransferLine = z.infer<typeof StockTransferLineSchema>;

export const StockTransferOutSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  destination_outlet_id: z.string().uuid(),
  transfer_number: z.string().min(1),
  dispatched_at: z.string().datetime(),
  dispatched_by_user_id: z.string().uuid(),
  business_date: z.string(),
  notes: z.string().nullable(),
  lines: z.array(StockTransferLineSchema),
  schema_version: z.literal(1),
});
export type StockTransferOut = z.infer<typeof StockTransferOutSchema>;

// ---------------------------------------------------------------------------
// supplier_invoice / supplier_credit — CLOUD-ONLY, MODELLED NOT ACTED ON (M7)
// ---------------------------------------------------------------------------
//
// Deliberately NOT AggregateTypes (the refresh_token / device_credential
// precedent). No SQLite mirror: an outlet does not reconcile a supplier ledger
// with the uplink down, and an edge copy would be a second authority over
// money owed.
//
// M5 creates and lists them. NOTHING in M5 transitions
// SupplierInvoice.status beyond 'RECEIVED' — the settlement states exist so
// the shape does not change when M7 lands, the yield_factor_ppm precedent.
export const SupplierInvoiceStatusSchema = z.enum([
  "RECEIVED",
  "APPROVED",
  "PART_PAID",
  "PAID",
  "DISPUTED",
  "CANCELLED",
]);
export type SupplierInvoiceStatus = z.infer<typeof SupplierInvoiceStatusSchema>;

export const SupplierInvoiceSchema = z.object({
  id: z.string().uuid(),
  tenant_id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  supplier_id: z.string().uuid(),
  grn_id: z.string().uuid().nullable(),
  supplier_invoice_no: z.string().min(1),
  invoice_date: z.string(),
  due_date: z.string().nullable(),
  subtotal_paise: z.number().int().nonnegative(),
  tax_paise: z.number().int().nonnegative(),
  total_paise: z.number().int().nonnegative(),
  status: SupplierInvoiceStatusSchema,
  created_at: z.string().datetime(),
  updated_at: z.string().datetime(),
  schema_version: z.literal(1),
});
export type SupplierInvoice = z.infer<typeof SupplierInvoiceSchema>;

export const SupplierCreditSchema = z.object({
  id: z.string().uuid(),
  tenant_id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  supplier_id: z.string().uuid(),
  purchase_return_id: z.string().uuid().nullable(),
  credit_note_no: z.string().min(1),
  credit_date: z.string(),
  amount_paise: z.number().int().nonnegative(),
  created_at: z.string().datetime(),
  updated_at: z.string().datetime(),
  schema_version: z.literal(1),
});
export type SupplierCredit = z.infer<typeof SupplierCreditSchema>;
