import { describe, expect, it } from "vitest";
import { formatPaiseAsPlainDecimal, formatPaiseAsRupees, parseRupeesToPaise } from "./money";

// Admin must match the till's formatting exactly (apps/pos/src/domain/money.ts)
// — two apps, one product, one money format. formatPaiseAsRupees is what the
// operator reads; formatPaiseAsPlainDecimal is what an editable price input is
// seeded with, so it round-trips through parseRupeesToPaise without a ₹ prefix
// breaking the parse.

describe("formatPaiseAsRupees", () => {
  it("formats a whole rupee amount", () => {
    expect(formatPaiseAsRupees(180000)).toBe("₹1800.00");
  });

  it("formats a sub-rupee remainder", () => {
    expect(formatPaiseAsRupees(12505)).toBe("₹125.05");
  });

  it("formats a large total without float drift", () => {
    expect(formatPaiseAsRupees(99999999)).toBe("₹999999.99");
  });

  it("formats zero", () => {
    expect(formatPaiseAsRupees(0)).toBe("₹0.00");
  });

  it("formats negative amounts", () => {
    expect(formatPaiseAsRupees(-12550)).toBe("-₹125.50");
  });
});

describe("formatPaiseAsPlainDecimal", () => {
  it("has no currency symbol, so it round-trips through parseRupeesToPaise", () => {
    const plain = formatPaiseAsPlainDecimal(12550);
    expect(plain).toBe("125.50");
    expect(parseRupeesToPaise(plain)).toBe(12550);
  });
});
