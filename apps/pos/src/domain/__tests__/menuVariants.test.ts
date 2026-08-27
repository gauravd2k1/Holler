import { describe, expect, it } from "vitest";
import type { MenuItem, MenuItemVariant } from "@holler/contracts";

import { resolveVariantForTap, variantPricePaise, variantsForItem } from "../menu";

// Half at 18000 paise, Full at 32000. The gap between them is the whole point
// of these tests: a silent default here is a wrong BILL, not a wrong count.
const ITEM: MenuItem = {
  id: "0191e850-0000-7000-8000-0000000000a1",
  outlet_id: "0191a000-0000-7000-8000-00000000000a",
  category_id: "0191e850-0000-7000-8000-0000000000c1",
  name: "Palak Paneer",
  base_price_paise: 18000,
  is_available: true,
  tax_profile_id: null,
  hsn_sac: "9963",
  config_version: 1,
  schema_version: 1,
};

function variant(id: string, name: string, delta: number, isDefault: boolean): MenuItemVariant {
  return {
    id,
    menu_item_id: ITEM.id,
    name,
    price_delta_paise: delta,
    is_default: isDefault,
    config_version: 1,
    schema_version: 1,
  };
}

const HALF = variant("0191e850-0000-7000-8000-0000000000v1", "Half", 0, false);
const FULL = variant("0191e850-0000-7000-8000-0000000000v2", "Full", 14000, true);

describe("resolveVariantForTap", () => {
  it("resolves with a null variant when the item genuinely has none", () => {
    // T0b seeds 11 such items deliberately. Deduction records NO_VARIANT and
    // the sale completes -- the honest answer, not a defect.
    const r = resolveVariantForTap(ITEM, []);
    expect(r).toEqual({ kind: "RESOLVED", variantId: null, pricePaise: 18000 });
  });

  it("resolves silently when exactly one variant exists", () => {
    // Nothing to choose between, so a tap cannot be ambiguous. This is what
    // T0b's six "Regular" variants are for.
    const only = variant("0191e850-0000-7000-8000-0000000000v9", "Regular", 500, false);
    const r = resolveVariantForTap(ITEM, [only]);
    expect(r).toEqual({ kind: "RESOLVED", variantId: only.id, pricePaise: 18500 });
  });

  it("NEVER resolves a multi-variant item, even though one is is_default", () => {
    // The trap. A default fallback here sells Full at 32000 whenever nobody
    // chose, and prints a wrong bill. Regression-locked: if this ever returns
    // RESOLVED, the POS has started guessing at prices.
    const r = resolveVariantForTap(ITEM, [HALF, FULL]);
    expect(r.kind).toBe("MUST_CHOOSE");
  });

  it("lets is_default PRESELECT but not resolve", () => {
    const r = resolveVariantForTap(ITEM, [HALF, FULL]);
    if (r.kind !== "MUST_CHOOSE") throw new Error("expected MUST_CHOOSE");
    expect(r.preselectedId).toBe(FULL.id);
    expect(r.options.map((v) => v.name)).toEqual(["Half", "Full"]);
  });

  it("preselects nothing when two rows both claim the default", () => {
    // A config defect the contract does not forbid. Guessing at a price is
    // exactly what must not happen, so preselect neither.
    const bothDefault = [variant(HALF.id, "Half", 0, true), FULL];
    const r = resolveVariantForTap(ITEM, bothDefault);
    if (r.kind !== "MUST_CHOOSE") throw new Error("expected MUST_CHOOSE");
    expect(r.preselectedId).toBeNull();
  });

  it("ignores variants belonging to other items", () => {
    const other: MenuItemVariant = { ...HALF, menu_item_id: "0191e850-0000-7000-8000-0000000000zz" };
    const r = resolveVariantForTap(ITEM, [other]);
    expect(r).toEqual({ kind: "RESOLVED", variantId: null, pricePaise: 18000 });
  });
});

describe("variantPricePaise", () => {
  it("adds the delta to the item's base price, in integer paise", () => {
    expect(variantPricePaise(ITEM, HALF)).toBe(18000);
    expect(variantPricePaise(ITEM, FULL)).toBe(32000);
    expect(variantPricePaise(ITEM, null)).toBe(18000);
  });
});

describe("variantsForItem", () => {
  it("orders by price so the cheapest option reads first", () => {
    expect(variantsForItem(ITEM, [FULL, HALF]).map((v) => v.name)).toEqual(["Half", "Full"]);
  });
});
