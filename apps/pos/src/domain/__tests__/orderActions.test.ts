import { describe, expect, it } from "vitest";
import type { AuthenticatedPrincipal, OrderStatus } from "@holler/contracts";
import { canOfferConfirm, confirmErrorMessage } from "../orderActions";

function principal(permissions: AuthenticatedPrincipal["permissions"]): AuthenticatedPrincipal {
  return {
    user_id: "00000000-0000-7000-8000-000000000001",
    tenant_id: "00000000-0000-7000-8000-000000000002",
    outlet_id: "00000000-0000-7000-8000-000000000003",
    full_name: "Test Cashier",
    permissions,
    authenticated_offline: true,
    schema_version: 1,
  };
}

const NON_DRAFT_STATUSES: OrderStatus[] = [
  "CONFIRMED",
  "SENT_TO_KITCHEN",
  "PREPARING",
  "READY",
  "SERVED",
  "BILLED",
  "PAID",
  "CLOSED",
  "CANCELLED",
];

describe("canOfferConfirm", () => {
  it("offers confirm for a DRAFT order when the principal has order.modify", () => {
    expect(canOfferConfirm("DRAFT", principal(["order.modify"]))).toBe(true);
  });

  it("does not offer confirm for a DRAFT order without order.modify", () => {
    expect(canOfferConfirm("DRAFT", principal(["order.create"]))).toBe(false);
  });

  it("does not offer confirm with no principal at all", () => {
    expect(canOfferConfirm("DRAFT", null)).toBe(false);
  });

  it.each(NON_DRAFT_STATUSES)("does not offer confirm for status %s even with permission", (status) => {
    expect(canOfferConfirm(status, principal(["order.modify"]))).toBe(false);
  });
});

describe("confirmErrorMessage", () => {
  it("gives a plain-language message for ORDER_NOT_CONFIRMABLE", () => {
    const message = confirmErrorMessage({
      code: "ORDER_NOT_CONFIRMABLE",
      message: "order abc-123 is not in DRAFT status and cannot be confirmed: internal detail",
    });
    expect(message).not.toContain("abc-123");
    expect(message).not.toContain("internal detail");
    expect(message.toLowerCase()).toContain("confirm");
  });

  it("falls back to a generic message for any other error", () => {
    const message = confirmErrorMessage({ code: "STORAGE_ERROR", message: "sqlite: disk I/O error" });
    expect(message).not.toContain("sqlite");
    expect(message.toLowerCase()).toContain("try again");
  });

  it("falls back to a generic message for a non-TauriCommandError value", () => {
    expect(confirmErrorMessage(new Error("boom"))).toContain("try again");
  });
});
