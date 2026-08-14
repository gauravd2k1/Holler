import { describe, expect, it } from "vitest";
import type { AuthenticatedPrincipal, CashShift, Invoice, Payment } from "@holler/contracts";
import invoiceFixture from "@holler/contracts/fixtures/invoice.json";
import cashShiftFixture from "@holler/contracts/fixtures/cash_shift.json";
import {
  amountDuePaise,
  billingErrorMessage,
  canOfferBilling,
  canOfferReversal,
  isFullySettled,
  isVarianceReasonRequired,
  pendingTenderTotalPaise,
  projectedVariancePaise,
  remainingAfterPendingPaise,
  totalSettledPaise,
} from "../billing";

const invoice = invoiceFixture as Invoice; // grand_total_paise: 105000

function payment(overrides: Partial<Payment>): Payment {
  return {
    id: "018e5a2e-a001-7c3d-9f4e-1234567890ab",
    outlet_id: "018e5a2e-1a09-7c3d-9f4e-1234567890ab",
    order_id: "018e5a2e-2b10-7c3d-9f4e-1234567890ab",
    cash_shift_id: null,
    method: "CASH",
    status: "CAPTURED",
    amount_paise: 105000,
    tendered_paise: 105000,
    change_paise: 0,
    reference: null,
    external_id: null,
    reverses_payment_id: null,
    captured_at: "2026-08-12T14:32:00Z",
    allocations: [],
    created_by_user_id: "018e5a2e-3d12-7c3d-9f4e-1234567890ab",
    created_at: "2026-08-12T14:32:00Z",
    updated_at: "2026-08-12T14:32:00Z",
    version: 1,
    schema_version: 1,
    ...overrides,
  };
}

function principal(permissions: AuthenticatedPrincipal["permissions"]): AuthenticatedPrincipal {
  return {
    user_id: "018e5a2e-3d12-7c3d-9f4e-1234567890ab",
    tenant_id: "018e5a2e-0000-7c3d-9f4e-1234567890ab",
    outlet_id: "018e5a2e-1a09-7c3d-9f4e-1234567890ab",
    full_name: "Test Cashier",
    permissions,
    authenticated_offline: true,
    schema_version: 1,
  };
}

describe("totalSettledPaise", () => {
  it("sums only CAPTURED tenders", () => {
    const payments = [
      payment({ id: "p1", amount_paise: 50000 }),
      payment({ id: "p2", amount_paise: 40000, status: "PENDING" }),
    ];
    expect(totalSettledPaise(payments)).toBe(50000);
  });

  it("nets a reversal (negative amount) against its forward tender", () => {
    const payments = [
      payment({ id: "p1", amount_paise: 105000 }),
      payment({ id: "p2", amount_paise: -40000, reverses_payment_id: "p1" }),
    ];
    expect(totalSettledPaise(payments)).toBe(65000);
  });
});

describe("amountDuePaise / isFullySettled", () => {
  it("is the full grand total with no payments", () => {
    expect(amountDuePaise(invoice, [])).toBe(105000);
    expect(isFullySettled(invoice, [])).toBe(false);
  });

  it("is zero once fully settled", () => {
    const payments = [payment({ amount_paise: 105000 })];
    expect(amountDuePaise(invoice, payments)).toBe(0);
    expect(isFullySettled(invoice, payments)).toBe(true);
  });

  it("is exact for a split-payment worked example: ₹500 cash + ₹1,000 UPI + ₹500 card = ₹2,000 (§35)", () => {
    const splitInvoice: Invoice = { ...invoice, grand_total_paise: 200000 };
    const payments = [
      payment({ id: "p1", method: "CASH", amount_paise: 50000 }),
      payment({ id: "p2", method: "UPI", amount_paise: 100000, tendered_paise: null, change_paise: null }),
      payment({
        id: "p3",
        method: "CREDIT_CARD",
        amount_paise: 50000,
        tendered_paise: null,
        change_paise: null,
      }),
    ];
    expect(amountDuePaise(splitInvoice, payments)).toBe(0);
    expect(isFullySettled(splitInvoice, payments)).toBe(true);
  });
});

describe("pendingTenderTotalPaise / remainingAfterPendingPaise", () => {
  it("sums entries the cashier has typed but not yet submitted", () => {
    expect(
      pendingTenderTotalPaise([{ amountPaise: 50000 }, { amountPaise: 30000 }]),
    ).toBe(80000);
  });

  it("shows what remains after the entered-but-unsubmitted tenders are applied", () => {
    const remaining = remainingAfterPendingPaise(invoice, [], [
      { amountPaise: 50000 },
      { amountPaise: 30000 },
    ]);
    expect(remaining).toBe(25000); // 105000 - 80000
  });

  it("goes negative once entered tenders exceed what is due (change owed)", () => {
    const remaining = remainingAfterPendingPaise(invoice, [], [{ amountPaise: 200000 }]);
    expect(remaining).toBe(-95000);
  });

  it("never produces a non-integer paise value across many small tenders", () => {
    const entries = Array.from({ length: 7 }, () => ({ amountPaise: 1501 }));
    const total = pendingTenderTotalPaise(entries);
    expect(Number.isInteger(total)).toBe(true);
    expect(total).toBe(10507);
  });
});

describe("permissions", () => {
  it("gates the forward billing path on order.modify", () => {
    expect(canOfferBilling(null)).toBe(false);
    expect(canOfferBilling(principal(["order.create"]))).toBe(false);
    expect(canOfferBilling(principal(["order.modify"]))).toBe(true);
  });

  it("gates a reversal on order.void, not order.modify", () => {
    expect(canOfferReversal(principal(["order.modify"]))).toBe(false);
    expect(canOfferReversal(principal(["order.void"]))).toBe(true);
  });
});

describe("billingErrorMessage", () => {
  it("surfaces the edge's own variance message for a required reason (§39)", () => {
    const err = {
      code: "CASH_VARIANCE_REASON_REQUIRED",
      message: "cash shift shift-1 close rejected: counted cash differs from expected by -500 paise",
    };
    expect(billingErrorMessage(err)).toBe(err.message);
    expect(isVarianceReasonRequired(err)).toBe(true);
  });

  it("never renders a raw generic fallback as blank or silent", () => {
    expect(billingErrorMessage({ code: "SOMETHING_UNKNOWN", message: "x" })).not.toBe("");
    expect(billingErrorMessage(new Error("boom"))).not.toBe("");
  });

  it("gives an actionable message for a rejected forward tender amount", () => {
    expect(
      billingErrorMessage({ code: "FORWARD_PAYMENT_AMOUNT_NOT_POSITIVE", message: "x" }),
    ).toMatch(/greater than zero/);
  });
});

describe("projectedVariancePaise", () => {
  it("sums the shift's own movements — the same figure the edge will derive", () => {
    const shift = cashShiftFixture as unknown as CashShift;
    // OPENING_FLOAT 200000 + CASH_SALE 105000 = 305000 expected
    expect(projectedVariancePaise(shift, 304500)).toBe(-500);
    expect(projectedVariancePaise(shift, 305000)).toBe(0);
  });
});
