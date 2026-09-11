import { describe, expect, it } from "vitest";
import { buildUpiPaymentLink, readUpiDemoPayee } from "../upi";

describe("readUpiDemoPayee", () => {
  it("returns null when the VPA is unset", () => {
    expect(readUpiDemoPayee({})).toBeNull();
  });

  it("returns null when the VPA is blank", () => {
    expect(readUpiDemoPayee({ VITE_HOLLER_DEMO_UPI_VPA: "   " })).toBeNull();
  });

  it("falls back to the VPA as the display name when no name is configured", () => {
    expect(readUpiDemoPayee({ VITE_HOLLER_DEMO_UPI_VPA: "demo@upi" })).toEqual({
      vpa: "demo@upi",
      payeeName: "demo@upi",
    });
  });

  it("uses the configured display name when present", () => {
    expect(
      readUpiDemoPayee({
        VITE_HOLLER_DEMO_UPI_VPA: "demo@upi",
        VITE_HOLLER_DEMO_UPI_PAYEE_NAME: "Holler Demo Kitchen",
      }),
    ).toEqual({ vpa: "demo@upi", payeeName: "Holler Demo Kitchen" });
  });
});

describe("buildUpiPaymentLink", () => {
  it("builds a deep link for a round-rupee amount", () => {
    const link = buildUpiPaymentLink({
      vpa: "demo@upi",
      payeeName: "Holler Demo",
      amountPaise: 125500,
      note: "A184",
    });
    expect(link).toBe("upi://pay?pa=demo%40upi&pn=Holler%20Demo&am=1255.00&cu=INR&tn=A184");
  });

  // Pinned in the T11 brief and asserted identically on the Rust side
  // (`edge/printer/src/upi.rs::tests::shared_vector_matches_the_pinned_typescript_output`)
  // — two independent implementations of one link format in two languages
  // is exactly how a format drifts; this vector is the cheapest thing that
  // catches it.
  it("matches the shared cross-language vector (edge/printer/src/upi.rs)", () => {
    const link = buildUpiPaymentLink({
      vpa: "demo@upi",
      payeeName: "Holler Demo Kitchen",
      amountPaise: 125550,
      note: "A184",
    });
    expect(link).toBe(
      "upi://pay?pa=demo%40upi&pn=Holler%20Demo%20Kitchen&am=1255.50&cu=INR&tn=A184",
    );
  });

  it("builds a deep link for a sub-rupee remainder", () => {
    const link = buildUpiPaymentLink({
      vpa: "demo@upi",
      payeeName: "Holler Demo",
      amountPaise: 12505,
      note: "A184",
    });
    expect(link).toContain("am=125.05");
  });

  it("builds a deep link for a large total", () => {
    const link = buildUpiPaymentLink({
      vpa: "demo@upi",
      payeeName: "Holler Demo",
      amountPaise: 100000000,
      note: "A184",
    });
    expect(link).toContain("am=1000000.00");
  });

  it("url-encodes a payee name containing spaces and special characters", () => {
    const link = buildUpiPaymentLink({
      vpa: "demo@upi",
      payeeName: "Holler & Co",
      amountPaise: 100,
      note: "A184",
    });
    expect(link).toContain("pn=Holler%20%26%20Co");
  });

  it("carries the human-facing invoice/order number in tn, never a UUID", () => {
    const link = buildUpiPaymentLink({
      vpa: "demo@upi",
      payeeName: "Holler Demo",
      amountPaise: 100,
      note: "INV/FY26/PNQ/001423",
    });
    expect(link).toContain("tn=INV%2FFY26%2FPNQ%2F001423");
  });

  it("rejects a negative amount", () => {
    expect(() =>
      buildUpiPaymentLink({ vpa: "demo@upi", payeeName: "Holler Demo", amountPaise: -100, note: "A184" }),
    ).toThrow();
  });
});
