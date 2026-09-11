import { describe, expect, it } from "vitest";
import { formatIST } from "../datetime";

describe("formatIST", () => {
  it("renders a UTC timestamp in IST, offset +5:30 from UTC", () => {
    // 2026-01-15T12:00:00Z is 17:30 IST the same day.
    expect(formatIST("2026-01-15T12:00:00.000Z")).toBe("15 Jan 2026, 05:30 pm IST");
  });

  it("crosses a calendar day when the IST offset pushes it past midnight", () => {
    // 2026-01-15T19:00:00Z is 2026-01-16T00:30 IST — a different day than
    // the stored UTC date, the exact class of bug a bare `.slice(0, 10)`
    // on the ISO string would get wrong.
    expect(formatIST("2026-01-15T19:00:00.000Z")).toBe("16 Jan 2026, 12:30 am IST");
  });

  it("returns an em dash for null, undefined or empty input", () => {
    expect(formatIST(null)).toBe("—");
    expect(formatIST(undefined)).toBe("—");
    expect(formatIST("")).toBe("—");
  });

  it("returns the input unchanged for an unparseable timestamp, never a fabricated time", () => {
    expect(formatIST("not-a-date")).toBe("not-a-date");
  });
});
