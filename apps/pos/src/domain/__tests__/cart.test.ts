import { describe, expect, it } from "vitest";
import { cartSubtotalPaise, canSendOrder, requiresTable, type CartLine } from "../cart";

function line(overrides: Partial<CartLine> = {}): CartLine {
  const unitPricePaise = overrides.unitPricePaise ?? 25000;
  const quantity = overrides.quantity ?? 2;
  return {
    lineId: "l1",
    menuItemId: "m1",
    menuItemName: "Paneer Tikka",
    variantId: null,
    unitPricePaise,
    quantity,
    notes: null,
    modifiers: [],
    // Mirrors what the edge actually computes — see domain/cart.ts's
    // comment on why `lineTotal` no longer recomputes this client-side.
    lineTotalPaise: unitPricePaise * quantity,
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

  it("includes a modifier's price delta in the line total, not just unit price * quantity", () => {
    // The exact fiction docs/m3-planning.md called out: "204/204 scenarios
    // without ever exercising a modifier price delta". `lineTotalPaise` is
    // the edge's own computed value, so a modifier delta baked into it here
    // must be reflected without this module recomputing anything.
    const withModifier = line({
      unitPricePaise: 25000,
      quantity: 2,
      modifiers: [
        { modifierId: "mod-1", groupName: "Extras", optionName: "Extra cheese", priceDeltaPaise: 3000 },
      ],
      lineTotalPaise: (25000 + 3000) * 2,
    });
    expect(cartSubtotalPaise([withModifier])).toBe(56000);
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
