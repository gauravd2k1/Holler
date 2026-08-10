import { describe, expect, it } from "vitest";
import { nextStatus, nextStatusLabel, statusLabel } from "../kotTransitions";

describe("nextStatus", () => {
  it("walks the forward flow NEW -> ACKNOWLEDGED -> PREPARING -> READY -> SERVED", () => {
    expect(nextStatus("NEW")).toBe("ACKNOWLEDGED");
    expect(nextStatus("ACKNOWLEDGED")).toBe("PREPARING");
    expect(nextStatus("PREPARING")).toBe("READY");
    expect(nextStatus("READY")).toBe("SERVED");
  });

  it("offers no further transition once SERVED", () => {
    expect(nextStatus("SERVED")).toBeNull();
  });

  it("offers no transition for CANCELLED — not a KDS-driven state", () => {
    expect(nextStatus("CANCELLED")).toBeNull();
  });
});

describe("nextStatusLabel / statusLabel", () => {
  it("has a cook-facing label for every offered transition", () => {
    expect(nextStatusLabel("NEW")).toBe("Accept");
    expect(nextStatusLabel("READY")).toBe("Mark served");
  });

  it("labels every status for display", () => {
    for (const status of ["NEW", "ACKNOWLEDGED", "PREPARING", "READY", "SERVED", "CANCELLED"] as const) {
      expect(statusLabel(status)).toBeTruthy();
    }
  });
});
