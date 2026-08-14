import { create } from "zustand";

// Which cash shift, if any, is currently open for the logged-in cashier on
// this device (§39). In-memory only, like `store/auth.ts`'s principal — a
// fresh app launch requires opening (or resuming, once a query exists) a
// shift again. `holler_edge_database` currently exposes no query to list
// open shifts for an outlet/device (T9 report: only `get_cash_shift(id)` by
// id), so this store is the only place the POS remembers which shift is
// open across the session; it cannot yet recover one after a restart.
interface CashShiftState {
  openShiftId: string | null;
  setOpenShiftId: (id: string | null) => void;
}

export const useCashShiftStore = create<CashShiftState>((set) => ({
  openShiftId: null,
  setOpenShiftId: (id) => set({ openShiftId: id }),
}));
