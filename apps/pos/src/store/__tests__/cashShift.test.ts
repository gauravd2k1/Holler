import { describe, expect, it, vi, beforeEach } from "vitest";

// T9 retry, Defect 2: mocks the Tauri boundary itself, not the store's own
// logic — proves the store recovers whatever `lib/tauri` (i.e. SQLite, via
// the real edge crate's `find_open_cash_shift`) reports, never an
// in-memory guess that a restart would erase.
const findOpenCashShiftMock = vi.fn();

vi.mock("../../lib/tauri", async () => {
  const actual = await vi.importActual<typeof import("../../lib/tauri")>("../../lib/tauri");
  return {
    ...actual,
    findOpenCashShift: (...args: unknown[]) => findOpenCashShiftMock(...args),
  };
});

const { useCashShiftStore } = await import("../cashShift");

const OPEN_SHIFT = {
  id: "shift-1",
  outlet_id: "outlet-1",
  device_id: "device-1",
  cashier_user_id: "user-1",
  status: "OPEN",
  opened_at: "2026-08-14T09:00:00Z",
  opening_cash_paise: 200000,
  closed_at: null,
  expected_cash_paise: null,
  actual_cash_paise: null,
  variance_paise: null,
  variance_reason: null,
  business_date: "2026-08-14",
  movements: [],
  created_at: "2026-08-14T09:00:00Z",
  updated_at: "2026-08-14T09:00:00Z",
  version: 1,
  schema_version: 1,
} as const;

beforeEach(() => {
  findOpenCashShiftMock.mockReset();
  useCashShiftStore.setState({ openShiftId: null, recovered: false });
});

describe("useCashShiftStore.recoverOpenShift", () => {
  it("adopts an orphaned OPEN shift found for this cashier/device — the restart-recovery path", async () => {
    findOpenCashShiftMock.mockResolvedValue(OPEN_SHIFT);
    await useCashShiftStore.getState().recoverOpenShift("user-1");
    expect(findOpenCashShiftMock).toHaveBeenCalledWith("user-1");
    expect(useCashShiftStore.getState().openShiftId).toBe("shift-1");
    expect(useCashShiftStore.getState().recovered).toBe(true);
  });

  it("leaves openShiftId null when nothing is open, without throwing", async () => {
    findOpenCashShiftMock.mockResolvedValue(null);
    await useCashShiftStore.getState().recoverOpenShift("user-1");
    expect(useCashShiftStore.getState().openShiftId).toBeNull();
    expect(useCashShiftStore.getState().recovered).toBe(true);
  });

  it("does not overwrite a shift already known this session with a redundant lookup", async () => {
    useCashShiftStore.setState({ openShiftId: "already-known", recovered: false });
    await useCashShiftStore.getState().recoverOpenShift("user-1");
    expect(findOpenCashShiftMock).not.toHaveBeenCalled();
    expect(useCashShiftStore.getState().openShiftId).toBe("already-known");
    expect(useCashShiftStore.getState().recovered).toBe(true);
  });
});
