// Payments, payment allocation and the cash shift — added at 0.4.0 (ADR-016,
// Milestone 3). Mirrors sqlite/0006_m3_billing.sql and postgres/0007.
//
// All EDGE-AUTHORITATIVE (§50.1). `payment` has been in AGGREGATE_AUTHORITY as
// EDGE_TO_CLOUD since Milestone 0.5 with no payload behind it; 0.4.0 fills the
// shape in. That is a fill-in, not a new authority claim — the direction was
// decided when the map was written (ADR-016 §Payment).
//
// §34: never `order.payment_method = "UPI"`. A ₹2,000 bill settled as ₹500
// cash + ₹1,000 UPI + ₹500 card is three Payments, not one field.
//
// APPEND-ONLY (docs/spec/payments.md §Conflict policy, §53). Nothing mutates a
// captured payment: a void or refund appends a reversal row pointing at the
// original. Financial records are never last-write-wins.

import { z } from "zod";

export const PaymentMethodSchema = z.enum([
  "CASH",
  "UPI",
  "CREDIT_CARD",
  "DEBIT_CARD",
  "WALLET",
  "GIFT_CARD",
  "LOYALTY_POINTS",
  "BANK_TRANSFER",
  "AGGREGATOR_PAID",
  "HOUSE_ACCOUNT",
  "CREDIT",
]);
export type PaymentMethod = z.infer<typeof PaymentMethodSchema>;

// The lifecycle of ONE TENDER's capture attempt. Deliberately not
// CanonicalOrder.payment_status, which is the order's overall standing
// (UNPAID / PARTIALLY_PAID / PAID / REFUNDED). A ₹2,000 order can sit at
// PARTIALLY_PAID while one of its three tenders is CAPTURED, another FAILED and
// a third PENDING — collapsing the two would lose exactly that distinction,
// which is the point of §34's separate Payment entity.
//
// Milestone 3 delivers CASH and split tenders only; gateway capture lands in
// Milestone 7 (§81 EXCLUDES online payment gateways). The states are modelled
// now so a Razorpay attempt has somewhere to go without a contract change.
export const PaymentCaptureStatusSchema = z.enum([
  "PENDING",
  "CAPTURED",
  "FAILED",
  "VOIDED",
  "REFUNDED",
]);
export type PaymentCaptureStatus = z.infer<typeof PaymentCaptureStatusSchema>;

// How one tender settles against one or more invoices. This is what lets split
// payment and split bill compose: one card swipe can settle two parts of a
// split group, and one part can be settled by three tenders.
export const PaymentAllocationSchema = z.object({
  id: z.string().uuid(),
  payment_id: z.string().uuid(),
  invoice_id: z.string().uuid(),
  amount_paise: z.number().int(),
  schema_version: z.literal(1),
});
export type PaymentAllocation = z.infer<typeof PaymentAllocationSchema>;

export const PaymentSchema = z
  .object({
    id: z.string().uuid(),
    outlet_id: z.string().uuid(),
    order_id: z.string().uuid(),
    cash_shift_id: z.string().uuid().nullable(),
    method: PaymentMethodSchema,
    status: PaymentCaptureStatusSchema,
    amount_paise: z.number().int(), // negative on a reversal row
    tendered_paise: z.number().int().nullable(), // cash only: what the customer handed over
    change_paise: z.number().int().nullable(), // cash only
    reference: z.string().nullable(), // UTR / auth code / manual card slip number
    external_id: z.string().nullable(), // gateway id; Milestone 7
    reverses_payment_id: z.string().uuid().nullable(),
    captured_at: z.string().datetime().nullable(),
    allocations: z.array(PaymentAllocationSchema).default([]),
    created_by_user_id: z.string().uuid(),
    created_at: z.string().datetime(),
    updated_at: z.string().datetime(),
    version: z.number().int(),
    schema_version: z.literal(1),
  })
  // Mirrors the CHECKs in sqlite/0006 and postgres/0007. Cash-drawer fields on
  // a card or UPI tender would corrupt the expected-cash derivation for the
  // whole shift, so they are unrepresentable rather than merely discouraged.
  .refine((p) => p.tendered_paise === null || p.method === "CASH", {
    message: "tendered_paise is meaningful only on a CASH tender",
  })
  .refine((p) => p.reverses_payment_id === null || p.amount_paise <= 0, {
    message: "a reversal row carries a non-positive amount",
  });
export type Payment = z.infer<typeof PaymentSchema>;

export const CashShiftStatusSchema = z.enum(["OPEN", "CLOSED"]);
export type CashShiftStatus = z.infer<typeof CashShiftStatusSchema>;

export const CashMovementKindSchema = z.enum([
  "OPENING_FLOAT",
  "CASH_SALE",
  "CASH_REFUND",
  "PAID_IN",
  "PAID_OUT",
]);
export type CashMovementKind = z.infer<typeof CashMovementKindSchema>;

// Every movement of physical cash through the drawer (§39). Child row inside
// the shift's payload. Append-only: a correction is another movement.
export const CashMovementSchema = z
  .object({
    id: z.string().uuid(),
    cash_shift_id: z.string().uuid(),
    kind: CashMovementKindSchema,
    amount_paise: z.number().int(), // signed: PAID_OUT and CASH_REFUND negative
    reason: z.string().nullable(),
    payment_id: z.string().uuid().nullable(),
    created_by_user_id: z.string().uuid(),
    created_at: z.string().datetime(),
    schema_version: z.literal(1),
  })
  .refine((m) => !["PAID_IN", "PAID_OUT"].includes(m.kind) || m.reason !== null, {
    message: "a paid-in or paid-out movement requires a reason (§39)",
  });
export type CashMovement = z.infer<typeof CashMovementSchema>;

// Cashier-specific register (§39). Expected cash is derived from movements;
// actual is counted by a human; variance is the difference and needs a reason.
export const CashShiftSchema = z
  .object({
    id: z.string().uuid(),
    outlet_id: z.string().uuid(),
    device_id: z.string().uuid(),
    cashier_user_id: z.string().uuid(),
    status: CashShiftStatusSchema,
    opened_at: z.string().datetime(),
    opening_cash_paise: z.number().int().min(0),
    closed_at: z.string().datetime().nullable(),
    expected_cash_paise: z.number().int().nullable(),
    actual_cash_paise: z.number().int().nullable(),
    variance_paise: z.number().int().nullable(),
    variance_reason: z.string().nullable(),
    business_date: z.string(), // outlet-local YYYY-MM-DD
    movements: z.array(CashMovementSchema).default([]),
    created_at: z.string().datetime(),
    updated_at: z.string().datetime(),
    version: z.number().int(),
    schema_version: z.literal(1),
  })
  // A closed shift is fully accounted for. A register closed without its count
  // can never be reconciled afterwards, so the state is unrepresentable.
  .refine(
    (s) =>
      s.status === "OPEN" ||
      (s.closed_at !== null &&
        s.expected_cash_paise !== null &&
        s.actual_cash_paise !== null &&
        s.variance_paise !== null),
    { message: "a CLOSED shift must carry closed_at, expected, actual and variance" },
  )
  .refine((s) => s.variance_paise === null || s.variance_paise === 0 || s.variance_reason !== null, {
    message: "a non-zero cash variance requires a reason (§39)",
  });
export type CashShift = z.infer<typeof CashShiftSchema>;
