import { describe, expect, it } from "vitest";
import type {
  AuthenticatedPrincipal,
  CashShift,
  DiscountDefinition,
  Invoice,
  Payment,
} from "@holler/contracts";
import invoiceFixture from "@holler/contracts/fixtures/invoice.json";
import cashShiftFixture from "@holler/contracts/fixtures/cash_shift.json";
import {
  amountDuePaise,
  billingErrorMessage,
  canApplyDiscount,
  canOfferBilling,
  canOfferReversal,
  discountRequiresReason,
  everySplitPartHasALine,
  isDiscountOfferable,
  isFullySettled,
  isVarianceReasonRequired,
  paymentsForInvoice,
  pendingTenderTotalPaise,
  previewLineDiscountPerUnitPaise,
  projectedVariancePaise,
  remainingAfterPendingPaise,
  splitPartToRequest,
  stagedDiscountsAreComplete,
  totalQuantityAssignedPreview,
  totalSettledPaise,
  type SplitPartDraft,
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

  it("surfaces the edge's own message verbatim for a double-settlement rejection (T9 retry)", () => {
    const err = {
      code: "FORWARD_PAYMENT_EXCEEDS_REMAINING_DUE",
      message: "payment of 100 paise exceeds invoice inv-1's remaining due of 0 paise",
    };
    expect(billingErrorMessage(err)).toBe(err.message);
  });
});

function discountDefinition(overrides: Partial<DiscountDefinition>): DiscountDefinition {
  return {
    id: "018e5a2e-9000-7c3d-9f4e-1234567890ab",
    outlet_id: "018e5a2e-1a09-7c3d-9f4e-1234567890ab",
    code: "STAFF10",
    name: "Staff 10%",
    scope: "LINE",
    method: "PERCENT",
    value_bps: 1000,
    value_paise: null,
    max_discount_paise: null,
    required_permission: null,
    requires_reason: false,
    is_active: true,
    effective_from: "2020-01-01T00:00:00Z",
    effective_to: null,
    config_version: 1,
    schema_version: 1,
    ...overrides,
  };
}

describe("isDiscountOfferable", () => {
  const now = "2026-08-15T12:00:00Z";

  it("offers an active, effective, LINE-scope discount", () => {
    expect(isDiscountOfferable(discountDefinition({}), now)).toBe(true);
  });

  it("never offers a BILL-scope discount — unimplemented, not silently narrowed", () => {
    expect(isDiscountOfferable(discountDefinition({ scope: "BILL" }), now)).toBe(false);
  });

  it("never offers an inactive discount", () => {
    expect(isDiscountOfferable(discountDefinition({ is_active: false }), now)).toBe(false);
  });

  it("never offers a discount that is not yet effective", () => {
    expect(
      isDiscountOfferable(discountDefinition({ effective_from: "2027-01-01T00:00:00Z" }), now),
    ).toBe(false);
  });

  it("never offers a discount past its effective_to", () => {
    expect(
      isDiscountOfferable(discountDefinition({ effective_to: "2026-01-01T00:00:00Z" }), now),
    ).toBe(false);
  });
});

describe("discountRequiresReason / canApplyDiscount", () => {
  it("reflects the definition's own requires_reason flag", () => {
    expect(discountRequiresReason(discountDefinition({ requires_reason: true }))).toBe(true);
    expect(discountRequiresReason(discountDefinition({ requires_reason: false }))).toBe(false);
  });

  it("permits any authenticated principal when no permission is named", () => {
    expect(canApplyDiscount(principal([]), discountDefinition({}))).toBe(true);
    expect(canApplyDiscount(null, discountDefinition({}))).toBe(false);
  });

  it("requires the named permission when one is set", () => {
    // No dedicated billing permission exists yet (ADR-016 0.4.4 addendum) —
    // a real discount_definition names one of the existing Permission enum
    // values, so this uses one rather than inventing a string outside it.
    const def = discountDefinition({ required_permission: "order.void" });
    expect(canApplyDiscount(principal(["order.modify"]), def)).toBe(false);
    expect(canApplyDiscount(principal(["order.void"]), def)).toBe(true);
  });
});

describe("previewLineDiscountPerUnitPaise", () => {
  it("computes a PERCENT discount by integer basis points, never a float multiply", () => {
    // 10% (1000 bps) of Rs.325.00 (32500 paise) = 3250 paise exactly.
    const def = discountDefinition({ method: "PERCENT", value_bps: 1000 });
    expect(previewLineDiscountPerUnitPaise(def, 32_500)).toBe(3_250);
  });

  it("rounds a PERCENT discount half-up", () => {
    // 15% of Rs.0.33 (33 paise): 33*1500 = 49500; /10000 = 4.95 -> 5.
    const def = discountDefinition({ method: "PERCENT", value_bps: 1500 });
    expect(previewLineDiscountPerUnitPaise(def, 33)).toBe(5);
  });

  it("caps a PERCENT discount at max_discount_paise", () => {
    // 50% of Rs.500.00 (50000 paise) = 25000, capped to 5000.
    const def = discountDefinition({ method: "PERCENT", value_bps: 5000, max_discount_paise: 5_000 });
    expect(previewLineDiscountPerUnitPaise(def, 50_000)).toBe(5_000);
  });

  it("uses the configured paise value verbatim for an AMOUNT discount", () => {
    const def = discountDefinition({ method: "AMOUNT", value_bps: null, value_paise: 5_000 });
    expect(previewLineDiscountPerUnitPaise(def, 32_500)).toBe(5_000);
  });
});

describe("stagedDiscountsAreComplete", () => {
  it("is complete when nothing requires a reason", () => {
    const def = discountDefinition({ id: "d1", requires_reason: false });
    const byId = new Map([["d1", def]]);
    expect(
      stagedDiscountsAreComplete(
        [{ orderItemId: "item-1", discountDefinitionId: "d1", reason: "" }],
        byId,
      ),
    ).toBe(true);
  });

  it("is incomplete when a required reason is blank, complete once typed", () => {
    const def = discountDefinition({ id: "d1", requires_reason: true });
    const byId = new Map([["d1", def]]);
    expect(
      stagedDiscountsAreComplete(
        [{ orderItemId: "item-1", discountDefinitionId: "d1", reason: "   " }],
        byId,
      ),
    ).toBe(false);
    expect(
      stagedDiscountsAreComplete(
        [{ orderItemId: "item-1", discountDefinitionId: "d1", reason: "manager approved" }],
        byId,
      ),
    ).toBe(true);
  });
});

describe("billingErrorMessage — discount codes (ADR-016 §28)", () => {
  it("surfaces the edge's own message for a missing reason", () => {
    const err = { code: "DISCOUNT_REASON_REQUIRED", message: "discount 'MGR_COMP' requires a reason" };
    expect(billingErrorMessage(err)).toBe(err.message);
  });

  it("surfaces the edge's own message for a denied permission", () => {
    const err = {
      code: "DISCOUNT_PERMISSION_DENIED",
      message: "applying discount 'OVERRIDE20' requires the 'bill.discount.override' permission",
    };
    expect(billingErrorMessage(err)).toBe(err.message);
  });

  it("names BILL scope as unavailable rather than a generic failure", () => {
    expect(billingErrorMessage({ code: "DISCOUNT_SCOPE_NOT_SUPPORTED", message: "x" })).toMatch(
      /not available/,
    );
  });

  it("surfaces the edge tax engine's own validity message verbatim", () => {
    const err = {
      code: "INVALID_INPUT",
      message: "discount_per_unit_paise must not exceed unit_price_paise",
    };
    expect(billingErrorMessage(err)).toBe(err.message);
  });
});

describe("paymentsForInvoice", () => {
  it("keeps only payments allocated against the named invoice", () => {
    const p1 = payment({
      id: "p1",
      allocations: [{ id: "a1", payment_id: "p1", invoice_id: "inv-1", amount_paise: 21000, schema_version: 1 }],
    });
    const p2 = payment({
      id: "p2",
      allocations: [{ id: "a2", payment_id: "p2", invoice_id: "inv-2", amount_paise: 21000, schema_version: 1 }],
    });
    expect(paymentsForInvoice([p1, p2], "inv-1")).toEqual([p1]);
    expect(paymentsForInvoice([p1, p2], "inv-2")).toEqual([p2]);
  });

  it("excludes a payment with no allocation matching the invoice at all", () => {
    const p1 = payment({ id: "p1", allocations: [] });
    expect(paymentsForInvoice([p1], "inv-1")).toEqual([]);
  });
});

describe("split bill drafting", () => {
  it("drops non-positive quantities when building the wire request", () => {
    const draft: SplitPartDraft = { quantities: { "item-1": 1, "item-2": 0, "item-3": -1 } };
    expect(splitPartToRequest(draft)).toEqual({ lines: [{ orderItemId: "item-1", quantity: 1 }] });
  });

  it("previews the total quantity assigned to one line across every part", () => {
    const parts: SplitPartDraft[] = [
      { quantities: { "item-1": 1 } },
      { quantities: { "item-1": 1 } },
    ];
    expect(totalQuantityAssignedPreview("item-1", parts)).toBe(2);
    expect(totalQuantityAssignedPreview("item-2", parts)).toBe(0);
  });

  it("requires every part to carry at least one positive-quantity line", () => {
    expect(everySplitPartHasALine([])).toBe(false);
    expect(
      everySplitPartHasALine([{ quantities: { "item-1": 1 } }, { quantities: {} }]),
    ).toBe(false);
    expect(
      everySplitPartHasALine([{ quantities: { "item-1": 1 } }, { quantities: { "item-2": 1 } }]),
    ).toBe(true);
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
