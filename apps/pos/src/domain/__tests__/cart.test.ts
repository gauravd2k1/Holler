import { describe, expect, it } from "vitest";
import { cartSubtotalPaise, canSendOrder, requiresTable, type CartLine } from "../cart";

function line(overrides: Partial<CartLine> = {}): CartLine {
  return {
    lineId: "l1",
    menuItemId: "m1",
    menuItemName: "Paneer Tikka",
    variantId: null,
    unitPricePaise: 25000,
    quantity: 2,
    notes: null,
    ...overrides,
  };
}

describe("cartSubtotalPaise", () => {
  it("sums line totals in integer paise", () => {
    const lines = [line({ unitPricePaise: 25000, quantity: 2 }), line({ lineId: "l2", unitPricePaise: 9900, quantity: 3 })];
    // 25000*2 + 9900*3 = 50000 + 29700 = 79700
    expect(cartSubtotalPaise(lines)).toBe(79700);
  });

  it("is zero for an empty cart", () => {
    expect(cartSubtotalPaise([])).toBe(0);
  });
});

describe("requiresTable", () => {
  it("requires a table only for DINE_IN", () => {
    expect(requiresTable("DINE_IN")).toBe(true);
    expect(requiresTable("TAKEAWAY")).toBe(false);
    expect(requiresTable("DELIVERY")).toBe(false);
  });
});

describe("canSendOrder", () => {
  it("refuses an empty cart", () => {
    expect(canSendOrder("TAKEAWAY", null, [])).toBe(false);
  });

  it("refuses DINE_IN without a table", () => {
    expect(canSendOrder("DINE_IN", null, [line()])).toBe(false);
  });

  it("allows DINE_IN with a table and items", () => {
    expect(canSendOrder("DINE_IN", "table-1", [line()])).toBe(true);
  });

  it("allows TAKEAWAY without a table", () => {
    expect(canSendOrder("TAKEAWAY", null, [line()])).toBe(true);
  });
});
