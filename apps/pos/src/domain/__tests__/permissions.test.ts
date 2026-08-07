import { describe, expect, it } from "vitest";
import type { AuthenticatedPrincipal } from "@holler/contracts";
import { hasPermission, requirePermission } from "../permissions";

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

describe("hasPermission", () => {
  it("is false with no principal", () => {
    expect(hasPermission(null, "order.create")).toBe(false);
  });

  it("is false when the permission is missing", () => {
    expect(hasPermission(principal(["order.modify"]), "order.create")).toBe(false);
  });

  it("is true when the permission is present", () => {
    expect(hasPermission(principal(["order.create"]), "order.create")).toBe(true);
  });
});

describe("requirePermission", () => {
  it("throws instead of allowing the action to be issued", () => {
    expect(() => requirePermission(principal([]), "order.cancel")).toThrow();
  });

  it("does not throw when permitted", () => {
    expect(() => requirePermission(principal(["order.cancel"]), "order.cancel")).not.toThrow();
  });
});
