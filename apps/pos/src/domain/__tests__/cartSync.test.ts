import { describe, expect, it } from "vitest";
import type { CanonicalOrder, MenuItem } from "@holler/contracts";
import { isRecoverableDraft, menuItemNameResolver, orderToCartLines } from "../cartSync";

function menuItem(overrides: Partial<MenuItem> = {}): MenuItem {
  return {
    id: "item-1",
    outlet_id: "outlet-1",
    category_id: "cat-1",
    name: "Paneer Tikka",
    base_price_paise: 25000,
    is_available: true,
    tax_profile_id: null,
    hsn_sac: null,
    config_version: 1,
    schema_version: 1,
    ...overrides,
  };
}

function order(items: CanonicalOrder["items"]): Pick<CanonicalOrder, "items"> {
  return { items };
}

describe("orderToCartLines", () => {
  it("maps every persisted order item to a cart line with the same quantity and price", () => {
    const resolve = menuItemNameResolver([menuItem()]);
    const lines = orderToCartLines(
      order([
        {
          id: "oi-1",
          menu_item_id: "item-1",
          variant_id: null,
          quantity: 3,
          unit_price_paise: 25000,
          line_total_paise: 75000,
          modifiers: [],
          notes: null,
        },
        {
          id: "oi-2",
          menu_item_id: "item-1",
          variant_id: null,
          quantity: 1,
          unit_price_paise: 25000,
          line_total_paise: 25000,
          modifiers: [],
          notes: "no onions",
        },
      ]),
      resolve,
    );

    expect(lines).toHaveLength(2);
    expect(lines[0]).toEqual({
      lineId: "oi-1",
      menuItemId: "item-1",
      menuItemName: "Paneer Tikka",
      variantId: null,
      unitPricePaise: 25000,
      quantity: 3,
      notes: null,
      modifiers: [],
      lineTotalPaise: 75000,
    });
    expect(lines[1]?.notes).toBe("no onions");
    expect(lines[1]?.quantity).toBe(1);
  });

  it("carries a line's real modifiers and the edge-computed line total through unchanged", () => {
    const resolve = menuItemNameResolver([menuItem()]);
    const lines = orderToCartLines(
      order([
        {
          id: "oi-1",
          menu_item_id: "item-1",
          variant_id: null,
          quantity: 2,
          unit_price_paise: 25000,
          line_total_paise: 56000, // (25000 + 3000) * 2 — the edge's own computation
          modifiers: [
            {
              modifier_id: "00000000-0000-7000-8000-000000000099",
              group_name: "Extras",
              option_name: "Extra cheese",
              price_delta_paise: 3000,
            },
          ],
          notes: null,
        },
      ]),
      resolve,
    );

    expect(lines[0]?.lineTotalPaise).toBe(56000);
    expect(lines[0]?.modifiers).toEqual([
      {
        modifierId: "00000000-0000-7000-8000-000000000099",
        groupName: "Extras",
        optionName: "Extra cheese",
        priceDeltaPaise: 3000,
      },
    ]);
  });

  it("falls back to the raw menu item id when no matching menu item is loaded", () => {
    const resolve = menuItemNameResolver([]);
    const lines = orderToCartLines(
      order([
        {
          id: "oi-1",
          menu_item_id: "item-missing",
          variant_id: null,
          quantity: 1,
          unit_price_paise: 10000,
          line_total_paise: 10000,
          modifiers: [],
          notes: null,
        },
      ]),
      resolve,
    );
    expect(lines[0]?.menuItemName).toBe("item-missing");
  });

  it("returns an empty cart for an order with no items", () => {
    expect(orderToCartLines(order([]), menuItemNameResolver([]))).toEqual([]);
  });
});

describe("isRecoverableDraft", () => {
  it("is true only for DRAFT status", () => {
    expect(isRecoverableDraft({ status: "DRAFT" })).toBe(true);
    expect(isRecoverableDraft({ status: "CONFIRMED" })).toBe(false);
    expect(isRecoverableDraft({ status: "SENT_TO_KITCHEN" })).toBe(false);
  });
});
