import { describe, expect, it } from "vitest";
import { DEFAULT_SLA_THRESHOLDS, elapsedMinutes, slaBucket } from "../sla";

describe("slaBucket", () => {
  it("is GREEN strictly below the green threshold", () => {
    expect(slaBucket(0, DEFAULT_SLA_THRESHOLDS)).toBe("GREEN");
    expect(slaBucket(7, DEFAULT_SLA_THRESHOLDS)).toBe("GREEN");
  });

  it("is AMBER exactly at the green boundary", () => {
    expect(slaBucket(8, DEFAULT_SLA_THRESHOLDS)).toBe("AMBER");
  });

  it("is AMBER up to and including the amber boundary", () => {
    expect(slaBucket(12, DEFAULT_SLA_THRESHOLDS)).toBe("AMBER");
  });

  it("is RED strictly above the amber boundary", () => {
    expect(slaBucket(13, DEFAULT_SLA_THRESHOLDS)).toBe("RED");
  });

  it("respects custom configured thresholds instead of the defaults", () => {
    const custom = { greenUnderMinutes: 2, amberUnderOrEqualMinutes: 4 };
    expect(slaBucket(1, custom)).toBe("GREEN");
    expect(slaBucket(2, custom)).toBe("AMBER");
    expect(slaBucket(4, custom)).toBe("AMBER");
    expect(slaBucket(5, custom)).toBe("RED");
  });
});

describe("elapsedMinutes", () => {
  it("floors partial minutes", () => {
    const created = new Date("2026-01-01T00:00:00Z").toISOString();
    const now = new Date("2026-01-01T00:04:59Z");
    expect(elapsedMinutes(created, now)).toBe(4);
  });

  it("never goes negative on clock skew", () => {
    const created = new Date("2026-01-01T00:10:00Z").toISOString();
    const now = new Date("2026-01-01T00:00:00Z");
    expect(elapsedMinutes(created, now)).toBe(0);
  });
});
