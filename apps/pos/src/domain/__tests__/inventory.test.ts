import { describe, expect, it } from "vitest";
import {
  formatGapQuantity,
  formatMicroQuantity,
  formatVarianceBps,
  isLowStock,
  lowStockLines,
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
    expect(formatMicroQuantity(1_500_000, "VOLUME")).toBe("1.5ml");
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
