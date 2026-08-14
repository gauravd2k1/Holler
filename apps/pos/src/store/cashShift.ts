import { create } from "zustand";
import { findOpenCashShift } from "../lib/tauri";

// Which cash shift, if any, is currently open for the logged-in cashier on
// this device (§39). `openShiftId` is in-memory only, like `store/auth.ts`'s
// principal — a fresh app launch starts with it `null`. Unlike a stale T9
// report once claimed, this is no longer a dead end after a restart: T9
// retry adds `holler_edge_database::Db::find_open_cash_shift` (an OPEN-shift
// lookup keyed on device_id/cashier_user_id, not an id the POS has to
// remember), and `recoverOpenShift` below calls it through
// `findOpenCashShift` — the automatic recovery path a restart needs
// (CLAUDE.md M2 acceptance item 2: "the loss not happening counts", not an
// API capable of preventing it). A cashier is never asked to type a shift
// id.
interface CashShiftState {
  openShiftId: string | null;
  /** `true` once startup recovery has run (successfully or not) for the
   * current login — gates rendering the "Open Shift" control so it never
   * flashes before a real recovery attempt has had a chance to find an
   * orphaned shift. */
  recovered: boolean;
  setOpenShiftId: (id: string | null) => void;
  /** Looks up `cashierUserId`'s currently OPEN shift on this device and
   * adopts it if one exists — idempotent and safe to call every time the
   * billing screen mounts (e.g. after a restart, or simply navigating back
   * to it): a `null` result just means no shift is open, not an error. */
  recoverOpenShift: (cashierUserId: string) => Promise<void>;
}

export const useCashShiftStore = create<CashShiftState>((set, get) => ({
  openShiftId: null,
  recovered: false,
  setOpenShiftId: (id) => set({ openShiftId: id }),
  recoverOpenShift: async (cashierUserId) => {
    // Do not clobber a shift already known this session (e.g. just opened
    // by `handleOpenShift`) with a redundant lookup.
    if (get().openShiftId !== null) {
      set({ recovered: true });
      return;
    }
    const shift = await findOpenCashShift(cashierUserId);
    set({ openShiftId: shift?.id ?? null, recovered: true });
  },
}));
