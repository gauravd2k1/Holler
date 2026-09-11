import { describe, expect, it } from "vitest";
import { addToCart, cartToOrderItems, cartTotalPaise, defaultVariant, freeModifiers } from "./cart";
import type { CaptainMenuItem } from "./api";

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
      option_name: "Extra spicy",
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
    expect(free).toHaveLength(1);
    expect(free[0].option_name).toBe("Extra spicy");
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
});
