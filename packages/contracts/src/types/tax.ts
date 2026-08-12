// Tax engine, fiscal identity and discount config — added at 0.4.0 (ADR-016,
// Milestone 3). Mirrors sqlite/0006_m3_billing.sql and postgres/0007.
//
// CONFIG aggregates under §50.1: cloud-owned, synced cloud→edge versioned by
// config_version, replaced wholesale at the edge. The invoice that *uses*
// these rules is edge-authoritative — the same config/operational split
// ADR-011 drew between RestaurantTable and TableSession, and ADR-014 between
// Station and Kot.
//
// §31: "Do NOT scatter tax percentages throughout the application." Every rate
// in the product resolves through a TaxProfile. No other module holds one.

import { z } from "zod";

// Rates are integer basis points, never floats and never percentage strings:
// 2.5% = 250, 18% = 1800. CLAUDE.md forbids floating point for money, and a
// rate that multiplies money inherits the rule — 0.025 * 12550 is exactly the
// kind of arithmetic that produces 313.74999999999994.
export const RateBpsSchema = z.number().int().min(0).max(10000);

export const TaxComponentSchema = z.enum(["CGST", "SGST", "IGST", "CESS"]);
export type TaxComponent = z.infer<typeof TaxComponentSchema>;

export const PricingModeSchema = z.enum(["INCLUSIVE", "EXCLUSIVE"]);
export type PricingMode = z.infer<typeof PricingModeSchema>;

// The versioned ruleset an invoice pins itself to. §31 requires historical
// bills stay reproducible after rules change, which is only possible if the
// bill records *which* ruleset produced it.
export const ComplianceVersionSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  label: z.string(),
  effective_from: z.string().datetime(),
  notes: z.string().nullable(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type ComplianceVersion = z.infer<typeof ComplianceVersionSchema>;

// A component rate inside a profile, effective-dated. Child row travelling in
// its parent's config bundle — the menu_item_variant precedent, not an
// aggregate of its own.
export const TaxRuleSchema = z.object({
  id: z.string().uuid(),
  tax_profile_id: z.string().uuid(),
  compliance_version_id: z.string().uuid(),
  component: TaxComponentSchema,
  rate_bps: RateBpsSchema,
  effective_from: z.string().datetime(),
  effective_to: z.string().datetime().nullable(), // null = open-ended
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type TaxRule = z.infer<typeof TaxRuleSchema>;

export const TaxProfileSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  code: z.string(), // 'GST_5_RESTAURANT' — stable machine code, unique per outlet
  name: z.string(),
  // Belongs to the profile, not the rule: a profile is inclusive or exclusive
  // as a whole, and mixing the two across one profile's components has no
  // coherent meaning.
  pricing_mode: PricingModeSchema,
  is_default: z.boolean(),
  is_active: z.boolean(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type TaxProfile = z.infer<typeof TaxProfileSchema>;

// The seller identity printed on a GST invoice (§33). Effective-dated because
// a GSTIN or trade name can change and a reprinted historical invoice must
// carry the identity current when it was issued, not today's.
export const OutletFiscalProfileSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  legal_name: z.string(),
  trade_name: z.string(),
  address_line1: z.string(),
  address_line2: z.string().nullable(),
  city: z.string(),
  state_code: z.string(), // GST state code: '27' = Maharashtra
  state_name: z.string(),
  pincode: z.string(),
  gstin: z.string(),
  fssai_number: z.string().nullable(),
  invoice_footer_text: z.string().nullable(),
  effective_from: z.string().datetime(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type OutletFiscalProfile = z.infer<typeof OutletFiscalProfileSchema>;

export const DiscountScopeSchema = z.enum(["LINE", "BILL"]);
export type DiscountScope = z.infer<typeof DiscountScopeSchema>;

export const DiscountMethodSchema = z.enum(["PERCENT", "AMOUNT"]);
export type DiscountMethod = z.infer<typeof DiscountMethodSchema>;

// A discount a cashier may apply. An ad-hoc discount is still governed by one
// of these rows: the row carries the permission and reason requirements
// (§28 bill.discount / bill.discount.override).
export const DiscountDefinitionSchema = z
  .object({
    id: z.string().uuid(),
    outlet_id: z.string().uuid(),
    code: z.string(),
    name: z.string(),
    scope: DiscountScopeSchema,
    method: DiscountMethodSchema,
    value_bps: RateBpsSchema.nullable(),
    value_paise: z.number().int().min(0).nullable(),
    max_discount_paise: z.number().int().nullable(), // cap for PERCENT; null = uncapped
    required_permission: z.string().nullable(),
    requires_reason: z.boolean(),
    is_active: z.boolean(),
    effective_from: z.string().datetime(),
    effective_to: z.string().datetime().nullable(),
    config_version: z.number().int(),
    schema_version: z.literal(1),
  })
  // Mirrors the CHECK in sqlite/0006 and postgres/0007. A half-populated row
  // reaching the tax engine poses a question with no defined answer — "20% or
  // ₹50?" — so it is unrepresentable at every layer rather than validated at
  // one of them.
  .refine(
    (d) =>
      d.method === "PERCENT"
        ? d.value_bps !== null && d.value_paise === null
        : d.value_paise !== null && d.value_bps === null,
    { message: "a PERCENT discount carries value_bps only; an AMOUNT discount carries value_paise only" },
  );
export type DiscountDefinition = z.infer<typeof DiscountDefinitionSchema>;
