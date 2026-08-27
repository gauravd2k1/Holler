import { describe, expect, it } from "vitest";
import {
  formatGapQuantity,
  formatMicroQuantity,
  formatVarianceBps,
  isLowStock,
  lowStockLines,
  isNegativeStock,
  negativeStockLines,
  stockAttentionLines,
} from "../inventory";
import type { CurrentStockLine } from "../../lib/tauri";

function line(overrides: Partial<CurrentStockLine> = {}): CurrentStockLine {
  return {
    inventory_item_id: "item-1",
    inventory_item_name: "Chicken",
    dimension: "MASS",
    current_quantity_micro: 5_000_000,
    reorder_level_micro: 2_000_000,
    par_level_micro: null,
    schema_version: 1,
    ...overrides,
  };
}

describe("formatMicroQuantity", () => {
  it("formats a whole gram amount with a MASS unit", () => {
    expect(formatMicroQuantity(5_000_000, "MASS")).toBe("5g");
  });

  it("formats a value with a fractional part, trimmed of trailing zeros", () => {
    // Was `expect(...1_500_000, "VOLUME").toBe("1.5ml")` until 2026-08-27 —
    // an assertion that ENCODED the 1000x VOLUME display bug and is why it
    // survived. 1_500_000 micro-litres is 1500ml. The fractional-trim
    // behaviour this case exists for is checked on MASS below and on VOLUME
    // at sub-millilitre scale in the dimension-scale suite.
    expect(formatMicroQuantity(1_500_000, "VOLUME")).toBe("1500ml");
    expect(formatMicroQuantity(1_500_000, "MASS")).toBe("1.5g");
    expect(formatMicroQuantity(333_333, "COUNT")).toBe("0.333333pcs");
  });

  it("formats negative quantities without clamping to zero — negative stock is legal", () => {
    expect(formatMicroQuantity(-250_000, "MASS")).toBe("-0.25g");
    expect(formatMicroQuantity(-5_000_000, "MASS")).toBe("-5g");
  });

  it("formats zero", () => {
    expect(formatMicroQuantity(0, "COUNT")).toBe("0pcs");
  });

  it("rejects a non-integer micro-quantity", () => {
    expect(() => formatMicroQuantity(1.5, "MASS")).toThrow();
  });
});

describe("formatGapQuantity", () => {
  it("is the plain integer count, never divided by a million", () => {
    // A gap of "3" sellable units must render as "3", not "0.000003" — this
    // is the distinction the task calls out explicitly: StockDeductionGap
    // .quantity is NOT a micro-quantity.
    expect(formatGapQuantity(3)).toBe("3");
  });

  it("rejects a non-integer count", () => {
    expect(() => formatGapQuantity(1.5)).toThrow();
  });

  it("would materially disagree with formatMicroQuantity on the same raw number", () => {
    // Same raw value 3, run through the wrong formatter, would read "0g" —
    // this pins the two formatters as genuinely different functions, not an
    // accidental alias of one another.
    expect(formatGapQuantity(3)).not.toBe(formatMicroQuantity(3, "MASS"));
  });
});

describe("isLowStock / lowStockLines", () => {
  it("is low when current <= reorder level", () => {
    expect(isLowStock(line({ current_quantity_micro: 2_000_000, reorder_level_micro: 2_000_000 }))).toBe(true);
    expect(isLowStock(line({ current_quantity_micro: 1_000_000, reorder_level_micro: 2_000_000 }))).toBe(true);
  });

  it("is not low when current is above reorder level", () => {
    expect(isLowStock(line({ current_quantity_micro: 3_000_000, reorder_level_micro: 2_000_000 }))).toBe(false);
  });

  it("is never low when reorder_level_micro is null — unconfigured, not zero", () => {
    expect(isLowStock(line({ current_quantity_micro: -5_000_000, reorder_level_micro: null }))).toBe(false);
  });

  it("treats negative current stock as low when a threshold is configured", () => {
    expect(isLowStock(line({ current_quantity_micro: -1_000_000, reorder_level_micro: 2_000_000 }))).toBe(true);
  });

  it("filters a mixed list down to only the low lines", () => {
    const lines = [
      line({ inventory_item_id: "a", current_quantity_micro: 1_000_000, reorder_level_micro: 2_000_000 }),
      line({ inventory_item_id: "b", current_quantity_micro: 5_000_000, reorder_level_micro: 2_000_000 }),
      line({ inventory_item_id: "c", current_quantity_micro: -1_000_000, reorder_level_micro: null }),
    ];
    expect(lowStockLines(lines).map((l) => l.inventory_item_id)).toEqual(["a"]);
  });
});

describe("formatVarianceBps", () => {
  it("formats a positive variance", () => {
    expect(formatVarianceBps(250)).toBe("2.50%");
  });

  it("formats a negative variance without clamping", () => {
    expect(formatVarianceBps(-50)).toBe("-0.50%");
  });

  it("formats zero", () => {
    expect(formatVarianceBps(0)).toBe("0.00%");
  });

  it("rejects a non-integer bps value", () => {
    expect(() => formatVarianceBps(2.5)).toThrow();
  });
});

// The defect this locks out, found by hand on 2026-08-27: Red Chilli Powder
// at -1.6 g was flagged LOW while Salt at -1.2 g was flagged NOTHING, purely
// because nobody had configured a reorder level for salt. A real failure with
// an absent signal, in the feature built to prevent absent signals.
describe("isNegativeStock", () => {
  it("is true below zero with NO reorder level configured — the Salt case", () => {
    expect(isNegativeStock(line({ current_quantity_micro: -1_200_000, reorder_level_micro: null })))
      .toBe(true);
  });

  it("is true below zero with a reorder level configured — the Red Chilli case", () => {
    expect(
      isNegativeStock(line({ current_quantity_micro: -1_600_000, reorder_level_micro: 2_000_000 })),
    ).toBe(true);
  });

  it("is false at exactly zero — nothing left is not the same as books wrong", () => {
    expect(isNegativeStock(line({ current_quantity_micro: 0, reorder_level_micro: null }))).toBe(
      false,
    );
  });

  it("never consults the reorder level", () => {
    // Same quantity, every possible threshold: the answer must not move.
    for (const reorder of [null, 0, 5_000_000, -5_000_000]) {
      expect(
        isNegativeStock(line({ current_quantity_micro: -1, reorder_level_micro: reorder })),
      ).toBe(true);
    }
  });
});

describe("stockAttentionLines", () => {
  it("reports a negative line once, under negative, never also as low", () => {
    const salt = line({
      inventory_item_id: "salt",
      current_quantity_micro: -1_200_000,
      reorder_level_micro: null,
    });
    const chilli = line({
      inventory_item_id: "chilli",
      current_quantity_micro: -1_600_000,
      reorder_level_micro: 2_000_000,
    });
    const paneer = line({
      inventory_item_id: "paneer",
      current_quantity_micro: 1_000_000,
      reorder_level_micro: 2_000_000,
    });

    const { negative, low } = stockAttentionLines([salt, chilli, paneer]);
    expect(negative.map((l) => l.inventory_item_id).sort()).toEqual(["chilli", "salt"]);
    // chilli is under its reorder level too, and must NOT appear twice.
    expect(low.map((l) => l.inventory_item_id)).toEqual(["paneer"]);
  });

  it("surfaces a negative item that no low-stock rule would ever catch", () => {
    const salt = line({ current_quantity_micro: -1, reorder_level_micro: null });
    expect(lowStockLines([salt])).toEqual([]);
    expect(negativeStockLines([salt])).toHaveLength(1);
  });
});

// VOLUME is stored in micro-LITRES, not micro-millilitres: edge units.rs has
// litres(n) = n*1e6 and millilitres(n) = n*1e3. Dividing every dimension by
// 1e6 and printing "ml" understated every volume 1000-fold, which is how Soda
// Water's litres(5) reorder level rendered as "5ml". Storage and recipe
// authoring were correct throughout; only the formatter was wrong.
describe("formatMicroQuantity — VOLUME is micro-litres", () => {
  it("renders a 5 litre reorder level as 5000ml, not 5ml — the Soda Water case", () => {
    expect(formatMicroQuantity(5_000_000, "VOLUME")).toBe("5000ml");
  });

  it("renders recipe-scale volumes correctly", () => {
    expect(formatMicroQuantity(20_000, "VOLUME")).toBe("20ml"); // 20ml cream
    expect(formatMicroQuantity(15_000, "VOLUME")).toBe("15ml"); // 15ml oil
  });

  it("keeps sub-millilitre precision without padding to the mass scale", () => {
    expect(formatMicroQuantity(250, "VOLUME")).toBe("0.25ml");
    expect(formatMicroQuantity(-250, "VOLUME")).toBe("-0.25ml");
  });

  it("leaves MASS and COUNT on the 1e6 scale", () => {
    expect(formatMicroQuantity(1_500_000, "MASS")).toBe("1.5g");
    expect(formatMicroQuantity(10_000_000_000, "MASS")).toBe("10000g");
    expect(formatMicroQuantity(0, "COUNT")).toBe("0pcs");
  });
});
