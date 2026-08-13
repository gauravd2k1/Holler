import { describe, expect, it } from "vitest";
import { formatPaiseAsRupees, lineTotalPaise, parseRupeesToPaise, sumPaise } from "../money";

describe("formatPaiseAsRupees", () => {
  it("formats zero", () => {
    expect(formatPaiseAsRupees(0)).toBe("₹0.00");
  });

  it("formats a value under one rupee", () => {
    expect(formatPaiseAsRupees(5)).toBe("₹0.05");
    expect(formatPaiseAsRupees(99)).toBe("₹0.99");
  });

  it("formats a typical amount", () => {
    expect(formatPaiseAsRupees(12550)).toBe("₹125.50");
  });

  it("formats a large amount without float drift", () => {
    // 99999999 paise repeated across many float divisions would drift;
    // integer div/mod must not.
    expect(formatPaiseAsRupees(99999999)).toBe("₹999999.99");
    expect(formatPaiseAsRupees(100000000)).toBe("₹1000000.00");
  });

  it("formats negative amounts", () => {
    expect(formatPaiseAsRupees(-12550)).toBe("-₹125.50");
  });

  it("rejects non-integer input", () => {
    expect(() => formatPaiseAsRupees(12550.5)).toThrow();
  });
});

describe("sumPaise", () => {
  it("sums an empty array to zero", () => {
    expect(sumPaise([])).toBe(0);
  });

  it("sums many small amounts without drift", () => {
    const amounts = Array.from({ length: 1000 }, () => 33); // 0.33 * 1000 = 330.00
    expect(sumPaise(amounts)).toBe(33000);
  });
});

describe("lineTotalPaise", () => {
  it("multiplies unit price by quantity", () => {
    expect(lineTotalPaise(12550, 3)).toBe(37650);
  });

  it("rejects zero or negative quantity", () => {
    expect(() => lineTotalPaise(100, 0)).toThrow();
    expect(() => lineTotalPaise(100, -1)).toThrow();
  });
});

describe("parseRupeesToPaise", () => {
  it("parses a whole number of rupees", () => {
    expect(parseRupeesToPaise("25")).toBe(2500);
  });

  it("parses two decimal places exactly, without float drift", () => {
    expect(parseRupeesToPaise("12.50")).toBe(1250);
    // A value float multiplication is known to mis-round (0.1 + 0.2-class
    // drift): 0.29 * 100 in native float is 28.999999999999996.
    expect(parseRupeesToPaise("0.29")).toBe(29);
  });

  it("pads a single decimal place", () => {
    expect(parseRupeesToPaise("5.5")).toBe(550);
  });

  it("parses a negative amount", () => {
    expect(parseRupeesToPaise("-10")).toBe(-1000);
  });

  it("parses zero", () => {
    expect(parseRupeesToPaise("0")).toBe(0);
  });

  it("trims surrounding whitespace", () => {
    expect(parseRupeesToPaise("  25.00  ")).toBe(2500);
  });

  it("rejects more than two decimal places rather than truncating a typo", () => {
    expect(parseRupeesToPaise("12.505")).toBeNull();
  });

  it("rejects non-numeric input", () => {
    expect(parseRupeesToPaise("abc")).toBeNull();
    expect(parseRupeesToPaise("")).toBeNull();
    expect(parseRupeesToPaise("12.")).toBeNull();
  });
});
