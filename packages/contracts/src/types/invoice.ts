// GST invoice and invoice numbering — added at 0.4.0 (ADR-016, Milestone 3).
// Mirrors sqlite/0006_m3_billing.sql and postgres/0007_m3_billing.sql.
//
// Invoice is EDGE-AUTHORITATIVE (§50.1): the outlet issues bills with the
// uplink down, and the cloud only ever replays them. No cloud handler mints an
// invoice number or transitions an invoice — the rule ADR-014 set for
// kot.status, applied to money.
//
// InvoiceSeries is CONFIG (cloud→edge): the series *definition*. The counter
// that produces the next number is edge-local (sqlite `invoice_sequence`),
// deliberately has no representation here, and never syncs. Splitting them is
// what keeps numbering concurrency-safe and offline-capable at once (§33).

import { z } from "zod";
import { RateBpsSchema } from "./tax";

export const InvoiceStatusSchema = z.enum(["ISSUED", "CANCELLED"]);
export type InvoiceStatus = z.infer<typeof InvoiceStatusSchema>;

export const SequenceResetPolicySchema = z.enum(["NEVER", "FY", "MONTH", "DAY"]);
export type SequenceResetPolicy = z.infer<typeof SequenceResetPolicySchema>;

export const InvoiceSeriesSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  code: z.string(), // 'SALES', 'CREDIT_NOTE'
  // Tokens: {FY} {YYYY} {MM} {DD} {OUTLET}. 'FY{FY}/{OUTLET}/' with
  // padding_width 6 renders FY26/PNQ/001423 — the short human-facing format
  // CLAUDE.md §Money/time/identifiers requires.
  prefix_template: z.string(),
  reset_policy: SequenceResetPolicySchema,
  padding_width: z.number().int().min(1).max(12),
  is_active: z.boolean(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type InvoiceSeries = z.infer<typeof InvoiceSeriesSchema>;

// Who bears the GST liability (§32). Modelled at issue time because direct and
// ECO supplies must never be combined in compliance reporting, and that is
// only possible if the classification was captured when the bill was raised.
// Milestone 3 EXCLUDES the reporting outputs, not these fields.
export const TaxLiabilityPartySchema = z.enum(["RESTAURANT", "ECO"]);
export type TaxLiabilityParty = z.infer<typeof TaxLiabilityPartySchema>;

export const InvoiceLineSchema = z.object({
  id: z.string().uuid(),
  invoice_id: z.string().uuid(),
  // The order line this bills. This is what makes the split-bill conservation
  // property checkable: across a split group, every order line must appear
  // exactly once in total quantity — no loss, no duplication, no double-tax.
  order_item_id: z.string().uuid(),
  line_no: z.number().int().positive(),
  description: z.string(), // snapshot at issue time — never re-read from live menu
  hsn_sac: z.string().nullable(),
  quantity: z.number().int().positive(),
  unit_price_paise: z.number().int(),
  gross_paise: z.number().int(),
  discount_paise: z.number().int().default(0),
  taxable_value_paise: z.number().int(),
  tax_profile_id: z.string().uuid(),
  cgst_rate_bps: RateBpsSchema.default(0),
  cgst_paise: z.number().int().default(0),
  sgst_rate_bps: RateBpsSchema.default(0),
  sgst_paise: z.number().int().default(0),
  igst_rate_bps: RateBpsSchema.default(0),
  igst_paise: z.number().int().default(0),
  cess_rate_bps: RateBpsSchema.default(0),
  cess_paise: z.number().int().default(0),
  total_paise: z.number().int(),
  schema_version: z.literal(1),
});
export type InvoiceLine = z.infer<typeof InvoiceLineSchema>;

export const InvoiceSchema = z
  .object({
    id: z.string().uuid(),
    outlet_id: z.string().uuid(),
    order_id: z.string().uuid(),

    // Split bills are N invoices over one order sharing a split_group_id. Each
    // part is independently numbered and independently payable, because that
    // is what the customer physically receives. There is no bill_split entity.
    split_group_id: z.string().uuid().nullable(),
    split_index: z.number().int().positive().default(1),
    split_count: z.number().int().positive().default(1),

    series_id: z.string().uuid(),
    invoice_number: z.string(),
    invoice_date: z.string().datetime(), // UTC storage (CLAUDE.md §Time)
    business_date: z.string(), // outlet-local YYYY-MM-DD; may cross midnight

    status: InvoiceStatusSchema,
    cancelled_reason: z.string().nullable(),
    cancelled_at: z.string().datetime().nullable(),

    customer_name: z.string().nullable(),
    customer_phone: z.string().nullable(),
    customer_gstin: z.string().nullable(),
    place_of_supply_state_code: z.string(),

    lines: z.array(InvoiceLineSchema),

    // Money — integer paise throughout (CLAUDE.md §Money).
    subtotal_paise: z.number().int(),
    discount_paise: z.number().int().default(0),
    taxable_value_paise: z.number().int(),
    cgst_paise: z.number().int().default(0),
    sgst_paise: z.number().int().default(0),
    igst_paise: z.number().int().default(0),
    cess_paise: z.number().int().default(0),
    round_off_paise: z.number().int().default(0),
    grand_total_paise: z.number().int(),

    // Reproducibility (§31). The resolved rules AND the seller identity as
    // they stood at issue time, so a reprint after a rate or GSTIN change
    // produces the original document rather than a recomputed one.
    compliance_version_id: z.string().uuid(),
    tax_snapshot: z.record(z.unknown()),
    fiscal_profile: z.record(z.unknown()),

    // ECO (§32) — modelled now, reported later.
    channel: z.string(),
    tax_liability_party: TaxLiabilityPartySchema,
    eco_operator_name: z.string().nullable(),
    eco_operator_gstin: z.string().nullable(),
    supply_classification: z.string().nullable(),

    created_by_user_id: z.string().uuid(),
    created_at: z.string().datetime(),
    updated_at: z.string().datetime(),
    version: z.number().int(),
    schema_version: z.literal(1),
  })
  // The ADR-016 rounding policy, stated a third time. It is already a CHECK in
  // sqlite/0006 and postgres/0007; repeating it here means a malformed bill
  // cannot even be constructed in TypeScript, let alone stored or replayed.
  // Tax is summed per component across the invoice and rounded half-up once;
  // the grand total is then rounded to the nearest rupee, with the delta in
  // round_off_paise.
  .refine(
    (inv) =>
      inv.grand_total_paise ===
      inv.taxable_value_paise +
        inv.cgst_paise +
        inv.sgst_paise +
        inv.igst_paise +
        inv.cess_paise +
        inv.round_off_paise,
    { message: "grand_total_paise must equal taxable value + tax components + round_off (ADR-016)" },
  )
  // Rounding to the nearest rupee can never move a total by more than half a
  // rupee. A larger value means round-off is absorbing an arithmetic error.
  .refine((inv) => Math.abs(inv.round_off_paise) <= 50, {
    message: "round_off_paise cannot exceed half a rupee (ADR-016)",
  })
  .refine((inv) => inv.grand_total_paise % 100 === 0, {
    message: "grand_total_paise must settle in whole rupees (ADR-016)",
  })
  .refine((inv) => inv.split_index <= inv.split_count, {
    message: "split_index cannot exceed split_count",
  });
export type Invoice = z.infer<typeof InvoiceSchema>;
