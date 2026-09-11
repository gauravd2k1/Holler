import { describe, expect, it } from "vitest";
import { formatPaise } from "./money";

describe("formatPaise", () => {
  it("formats whole rupees with two decimal places", () => {
    expect(formatPaise(32000)).toBe("₹320.00");
  });

  it("formats a non-round amount without floating point drift", () => {
    expect(formatPaise(12550)).toBe("₹125.50");
  });

  it("formats zero", () => {
    expect(formatPaise(0)).toBe("₹0.00");
  });

  it("formats a negative amount", () => {
    expect(formatPaise(-500)).toBe("-₹5.00");
  });
});
