import { describe, expect, it } from "vitest";
import {
  DECLARABLE_DIMENSIONS,
  GRN_GAP_REASONS,
  PURCHASE_RETURN_REASONS,
  canManageProcurement,
  echoHasDimensionDisagreement,
  entryIntentEcho,
  entryIntentRate,
  formatEnteredQuantity,
  grnGapDetailText,
  grnGapReasonCopy,
  procurementErrorMessage,
} from "../procurement";
import type { AuthenticatedPrincipal } from "@holler/contracts";
import type { GrnEntryIntentEcho, GrnGap } from "../../lib/tauri";

function echo(overrides: Partial<GrnEntryIntentEcho> = {}): GrnEntryIntentEcho {
  return {
    inventory_item_id: "item-1",
    inventory_item_name: "Basmati Rice",
    entered_purchase_unit: "SACK",
    entered_quantity_micro: 4_000_000,
    quantity_dimension: "MASS",
    pack_size_micro_applied: 50_000_000_000,
    base_quantity_micro: 200_000_000_000,
    item_dimension: "MASS",
    unit_cost_paise: 6,
    line_total_paise: 1_200_000,
    gap_reasons: [],
    schema_version: 1,
    ...overrides,
  };
}

function gap(overrides: Partial<GrnGap> = {}): GrnGap {
  return {
    id: "gap-1",
    outlet_id: "outlet-1",
    grn_id: "grn-1",
    grn_line_id: null,
    inventory_item_id: null,
    reason: "NO_PURCHASE_ORDER",
    detail: null,
    occurred_at: "2026-08-29T10:00:00.000Z",
    business_date: "2026-08-29",
    schema_version: 1,
    ...overrides,
  };
}

function principal(permissions: string[]): AuthenticatedPrincipal {
  return {
    user_id: "user-1",
    tenant_id: "tenant-1",
    outlet_id: "outlet-1",
    full_name: "Receiver",
    permissions,
    authenticated_offline: true,
    schema_version: 1,
  } as AuthenticatedPrincipal;
}

describe("formatEnteredQuantity", () => {
  it("prints whole purchase units with the supplier's own unit label", () => {
    expect(formatEnteredQuantity(4_000_000, "SACK")).toBe("4 SACK");
  });

  it("prints a decimal delivery-note quantity exactly, with no rounding", () => {
    expect(formatEnteredQuantity(12_500_000, "kg")).toBe("12.5 kg");
    expect(formatEnteredQuantity(750_000, "CRATE")).toBe("0.75 CRATE");
  });

  it("scales by the plain 1e6 purchase-unit scale, NOT the item's dimension", () => {
    // The trap this separate formatter exists to avoid: `formatMicroQuantity`
    // divides VOLUME by 1_000 (micro-litres to millilitres). A purchase unit
    // has no dimension, so running an entered quantity through that formatter
    // would report 4 drums as 4000 of something.
    expect(formatEnteredQuantity(4_000_000, "DRUM")).toBe("4 DRUM");
  });

  it("refuses a non-integer micro-quantity rather than printing a rounded one", () => {
    expect(() => formatEnteredQuantity(1.5, "SACK")).toThrow();
  });
});

describe("entryIntentEcho — M5 acceptance criterion 4", () => {
  it("states BOTH sides of the conversion and names the item", () => {
    // The line an operator reads at the door with a driver waiting. Typed
    // figure on the left, what actually reaches stock on the right.
    //
    // The base side prints in the dimension's own DISPLAY unit — grams for
    // MASS — because that is what `formatMicroQuantity` has printed on every
    // inventory screen since M4, and one product must not name the same
    // quantity two ways. 200 kg therefore reads "200000g".
    expect(entryIntentEcho(echo())).toBe("4 SACK → 200000g of Basmati Rice");
  });

  it("spells out the rate that was actually applied", () => {
    expect(entryIntentRate(echo())).toBe("1 SACK = 50000g");
  });

  it("formats the base side in the ITEM's dimension, not the purchase unit's scale", () => {
    const volume = echo({
      inventory_item_name: "Sunflower Oil",
      entered_purchase_unit: "TIN",
      entered_quantity_micro: 2_000_000,
      quantity_dimension: "VOLUME",
      item_dimension: "VOLUME",
      // 15 litres per tin, in micro-litres; 30 litres received.
      pack_size_micro_applied: 15_000_000,
      base_quantity_micro: 30_000_000,
    });
    // 30_000_000 micro-litres formatted against VOLUME is 30000ml = 30 l.
    expect(entryIntentEcho(volume)).toBe("2 TIN → 30000ml of Sunflower Oil");
  });

  it("carries a 1000x entry error straight into the echo, visibly", () => {
    // The failure this criterion exists for: someone types 4000 instead of 4.
    // The echo must restate it in base units so the magnitude is obvious
    // BEFORE the commit, not after a variance report months later.
    const fatFingered = echo({
      entered_quantity_micro: 4_000_000_000,
      base_quantity_micro: 200_000_000_000_000,
    });
    expect(entryIntentEcho(fatFingered)).toBe("4000 SACK → 200000000g of Basmati Rice");
  });
});

describe("echoHasDimensionDisagreement", () => {
  it("is false when the operator's declaration matches the item", () => {
    expect(echoHasDimensionDisagreement(echo())).toBe(false);
  });

  it("is true when they disagree — the condition the edge gaps as DIMENSION_MISMATCH", () => {
    expect(
      echoHasDimensionDisagreement(echo({ quantity_dimension: "COUNT", item_dimension: "MASS" })),
    ).toBe(true);
  });

  it("only reports; the declared dimension is never replaced by the item's", () => {
    // The x == x trap: if any layer copied item_dimension into
    // quantity_dimension, this could never return true. Asserted on a real
    // disagreement so the guard is provably able to fire.
    const mismatched = echo({ quantity_dimension: "VOLUME", item_dimension: "MASS" });
    expect(mismatched.quantity_dimension).toBe("VOLUME");
    expect(echoHasDimensionDisagreement(mismatched)).toBe(true);
  });
});

describe("grnGapReasonCopy — M5 acceptance criterion 3", () => {
  it("gives EVERY one of the eight contract reasons its own distinct title", () => {
    // The filed M4 defect this must not repeat: one blanket heading over
    // every reason, so a DIMENSION_MISMATCH reads as a missing recipe.
    // Distinctness is the property, so it is asserted as such.
    const titles = GRN_GAP_REASONS.map((r) => grnGapReasonCopy(r).title);
    expect(GRN_GAP_REASONS).toHaveLength(8);
    expect(new Set(titles).size).toBe(8);
  });

  it("gives every reason a next step, because a gap nobody can act on is noise", () => {
    for (const reason of GRN_GAP_REASONS) {
      expect(grnGapReasonCopy(reason).nextStep.length).toBeGreaterThan(0);
    }
  });

  it("never titles a gap with the raw enum code for a known reason", () => {
    for (const reason of GRN_GAP_REASONS) {
      expect(grnGapReasonCopy(reason).title).not.toBe(reason);
    }
  });

  it("falls back to the raw code for a reason this build has not heard of", () => {
    expect(grnGapReasonCopy("SOMETHING_NEW").title).toBe("SOMETHING_NEW");
  });
});

describe("grnGapDetailText", () => {
  it("renders the edge's own prose, which is what a person reads", () => {
    expect(grnGapDetailText(gap({ detail: "PO ref 'PO-9912' is not known at this outlet." }))).toBe(
      "PO ref 'PO-9912' is not known at this outlet.",
    );
  });

  it("never renders a blank row when the edge recorded no detail", () => {
    expect(grnGapDetailText(gap({ detail: null }))).toBe(
      grnGapReasonCopy("NO_PURCHASE_ORDER").nextStep,
    );
  });
});

describe("closed sets come from the contract enums, not a re-typed list", () => {
  it("offers the three declarable dimensions", () => {
    expect([...DECLARABLE_DIMENSIONS]).toEqual(["MASS", "VOLUME", "COUNT"]);
  });

  it("offers the six purchase-return reasons", () => {
    expect(PURCHASE_RETURN_REASONS).toHaveLength(6);
    expect(PURCHASE_RETURN_REASONS).toContain("DAMAGED");
    expect(PURCHASE_RETURN_REASONS).toContain("OVER_DELIVERY");
  });
});

describe("canManageProcurement", () => {
  it("requires procurement.manage", () => {
    expect(canManageProcurement(principal(["procurement.manage"]))).toBe(true);
    expect(canManageProcurement(principal(["inventory.manage"]))).toBe(false);
    expect(canManageProcurement(null)).toBe(false);
  });

  it("does not accept procurement.approve as a substitute", () => {
    // Two independent gates (ADR-019 §5), and approval is an admin-side
    // decision the edge must not be able to make at all.
    expect(canManageProcurement(principal(["procurement.approve"]))).toBe(false);
  });
});

describe("procurementErrorMessage", () => {
  it("shows the quantity rejection verbatim — it already names the bad text", () => {
    expect(
      procurementErrorMessage({
        code: "INVALID_RECEIPT_QUANTITY",
        message: '"4kg" is not a quantity.',
      }),
    ).toBe('"4kg" is not a quantity.');
  });

  it("says plainly that nothing was saved, so the driver is not let go too early", () => {
    expect(procurementErrorMessage(new Error("boom"))).toContain("has NOT been saved");
  });

  it("has no message for a missing purchase order, because that is not an error", () => {
    // Guarding the absence deliberately: a message here would be the first
    // step back toward refusing a delivery over a missing row.
    const generic = procurementErrorMessage(new Error("x"));
    for (const reason of GRN_GAP_REASONS) {
      expect(generic).not.toContain(reason);
    }
  });
});
