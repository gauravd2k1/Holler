import { describe, expect, it } from "vitest";
import {
  addToCart,
  cartToOrderItems,
  cartTotalPaise,
  defaultVariant,
  freeModifierGroups,
  freeModifiers,
  groupRequiresSelection,
  modifierSelectionSatisfied,
  setLineQuantity,
  cartUnitCount,
  type CartLine,
} from "./cart";
import type { CaptainMenuItem } from "./api";

// Mirrors the demo seed's SPICE_GROUP exactly: ("Spice", [("Mild", 0), ("Med",
// 0), ("Hot", 0)]) — three free, optional options in one group.
const item: CaptainMenuItem = {
  id: "0191a000-0000-7000-8000-000000000101",
  category_id: "0191a000-0000-7000-8000-000000000201",
  name: "Butter Chicken",
  base_price_paise: 32000,
  is_available: true,
  variants: [
    { id: "0191a000-0000-7000-8000-000000000301", name: "Half", price_delta_paise: -10000, is_default: false },
    { id: "0191a000-0000-7000-8000-000000000302", name: "Full", price_delta_paise: 0, is_default: true },
  ],
  modifiers: [
    {
      id: "0191a000-0000-7000-8000-000000000401",
      group_name: "Spice",
      option_name: "Mild",
      price_delta_paise: 0,
      min_selection: 0,
      max_selection: 1,
    },
    {
      id: "0191a000-0000-7000-8000-000000000403",
      group_name: "Spice",
      option_name: "Med",
      price_delta_paise: 0,
      min_selection: 0,
      max_selection: 1,
    },
    {
      id: "0191a000-0000-7000-8000-000000000404",
      group_name: "Spice",
      option_name: "Hot",
      price_delta_paise: 0,
      min_selection: 0,
      max_selection: 1,
    },
    {
      id: "0191a000-0000-7000-8000-000000000402",
      group_name: "Add-on",
      option_name: "Extra gravy",
      price_delta_paise: 5000,
      min_selection: 0,
      max_selection: 1,
    },
  ],
};

// A distinct item with a REQUIRED free group — min_selection >= 1 — used to
// pin the "must be resolved before the line can be added" rule separately
// from Spice's optional one.
const itemWithRequiredGroup: CaptainMenuItem = {
  ...item,
  id: "0191a000-0000-7000-8000-000000000102",
  name: "Steak",
  modifiers: [
    {
      id: "0191a000-0000-7000-8000-000000000501",
      group_name: "Doneness",
      option_name: "Medium",
      price_delta_paise: 0,
      min_selection: 1,
      max_selection: 1,
    },
    {
      id: "0191a000-0000-7000-8000-000000000502",
      group_name: "Doneness",
      option_name: "Well done",
      price_delta_paise: 0,
      min_selection: 1,
      max_selection: 1,
    },
  ],
};

describe("defaultVariant", () => {
  it("picks the is_default variant", () => {
    expect(defaultVariant(item)?.name).toBe("Full");
  });

  it("returns null for an item with no variant at all — a seed defect, not a null to send", () => {
    expect(defaultVariant({ ...item, variants: [] })).toBeNull();
  });
});

describe("freeModifiers", () => {
  it("keeps only price_delta_paise === 0 (reduced scope, docs/captain-api.md)", () => {
    const free = freeModifiers(item);
    expect(free.map((m) => m.option_name)).toEqual(["Mild", "Med", "Hot"]);
  });
});

describe("freeModifierGroups", () => {
  it("groups free options by group_name and excludes the all-paid Add-on group", () => {
    const groups = freeModifierGroups(item);
    expect(groups).toHaveLength(1);
    expect(groups[0].groupName).toBe("Spice");
    expect(groups[0].options.map((o) => o.option_name)).toEqual(["Mild", "Med", "Hot"]);
  });

  it("returns no groups for an item with no free options at all", () => {
    const paidOnly: CaptainMenuItem = {
      ...item,
      modifiers: item.modifiers.filter((m) => m.group_name === "Add-on"),
    };
    expect(freeModifierGroups(paidOnly)).toHaveLength(0);
  });

  it("returns no groups for an item with no modifiers configured", () => {
    expect(freeModifierGroups({ ...item, modifiers: [] })).toHaveLength(0);
  });
});

describe("groupRequiresSelection / modifierSelectionSatisfied", () => {
  it("an optional group (min_selection 0) never blocks confirm", () => {
    const groups = freeModifierGroups(item);
    expect(groupRequiresSelection(groups[0])).toBe(false);
    expect(modifierSelectionSatisfied(groups, new Map())).toBe(true);
  });

  it("a required group (min_selection >= 1) blocks confirm until resolved", () => {
    const groups = freeModifierGroups(itemWithRequiredGroup);
    expect(groupRequiresSelection(groups[0])).toBe(true);
    expect(modifierSelectionSatisfied(groups, new Map())).toBe(false);
    const resolved = new Map([["Doneness", [groups[0].options[0]]]]);
    expect(modifierSelectionSatisfied(groups, resolved)).toBe(true);
  });
});

describe("addToCart", () => {
  it("adds a new line with the variant's price applied", () => {
    const variant = defaultVariant(item)!;
    const lines = addToCart([], item, variant, []);
    expect(lines).toHaveLength(1);
    expect(lines[0].unitPricePaise).toBe(32000); // Full: base + 0 delta
    expect(lines[0].quantity).toBe(1);
    // A variant is mandatory on every line — never null (CLAUDE.md M4 lesson).
    expect(lines[0].variantId).toBe(variant.id);
  });

  it("increments quantity on a repeat tap of the same item+variant+modifiers, rather than duplicating the line", () => {
    const variant = defaultVariant(item)!;
    let lines = addToCart([], item, variant, []);
    lines = addToCart(lines, item, variant, []);
    expect(lines).toHaveLength(1);
    expect(lines[0].quantity).toBe(2);
  });

  it("treats a different variant as a distinct line", () => {
    const full = item.variants[1];
    const half = item.variants[0];
    let lines = addToCart([], item, full, []);
    lines = addToCart(lines, item, half, []);
    expect(lines).toHaveLength(2);
  });
});

describe("cartTotalPaise", () => {
  it("sums unit price times quantity across lines, in integer paise", () => {
    const variant = defaultVariant(item)!;
    let lines = addToCart([], item, variant, []);
    lines = addToCart(lines, item, variant, []);
    lines = addToCart(lines, item, variant, []);
    expect(cartTotalPaise(lines)).toBe(96000); // 32000 * 3
  });
});

describe("cartToOrderItems", () => {
  it("produces a variant_id on every line, never null", () => {
    const variant = defaultVariant(item)!;
    const lines = addToCart([], item, variant, []);
    const orderItems = cartToOrderItems(lines);
    expect(orderItems[0].variant_id).toBe(variant.id);
    expect(orderItems[0].quantity).toBe(1);
    expect(orderItems[0].unit_price_paise).toBe(32000);
  });

  it("carries a selected free modifier through to the wire shape docs/captain-api.md specifies", () => {
    const variant = defaultVariant(item)!;
    const hot = freeModifierGroups(item)[0].options.find((o) => o.option_name === "Hot")!;
    const lines = addToCart([], item, variant, [hot]);
    const orderItems = cartToOrderItems(lines);
    expect(orderItems[0].modifiers).toEqual([
      {
        modifier_id: hot.id,
        group_name: "Spice",
        option_name: "Hot",
        price_delta_paise: 0,
      },
    ]);
  });

  it("treats the same item+variant with a different modifier selection as a distinct line", () => {
    const variant = defaultVariant(item)!;
    const groupOptions = freeModifierGroups(item)[0].options;
    const mild = groupOptions.find((o) => o.option_name === "Mild")!;
    const hot = groupOptions.find((o) => o.option_name === "Hot")!;
    let lines = addToCart([], item, variant, [mild]);
    lines = addToCart(lines, item, variant, [hot]);
    expect(lines).toHaveLength(2);
  });
});

describe("setLineQuantity", () => {
  const line = (key: string, quantity: number): CartLine => ({
    key,
    menuItemId: "01a09500-0000-7000-8000-000000000001",
    itemName: `item-${key}`,
    variantId: "01a09500-0000-7000-8000-000000000002",
    variantName: "Regular",
    unitPricePaise: 10000,
    modifiers: [],
    quantity,
    notes: null,
  });

  it("changes one line and leaves its neighbours alone", () => {
    const before = [line("a", 1), line("b", 2)];
    const after = setLineQuantity(before, "b", 5);
    expect(after.map((l) => [l.key, l.quantity])).toEqual([
      ["a", 1],
      ["b", 5],
    ]);
  });

  it("removes the line at zero, rather than keeping a zero-quantity line", () => {
    // A zero-quantity line would serialise into the order as quantity 0, which
    // OrderItemSchema refuses (positive int) — so the cart must not be able to
    // hold one at all.
    const after = setLineQuantity([line("a", 1), line("b", 1)], "a", 0);
    expect(after.map((l) => l.key)).toEqual(["b"]);
  });

  it("removes at a negative quantity too, rather than storing it", () => {
    expect(setLineQuantity([line("a", 1)], "a", -3)).toEqual([]);
  });

  it("is a no-op for a key that is not in the cart", () => {
    const before = [line("a", 1)];
    expect(setLineQuantity(before, "missing", 9)).toEqual(before);
  });

  it("counts units, not lines", () => {
    expect(cartUnitCount([line("a", 2), line("b", 3)])).toBe(5);
  });
});
