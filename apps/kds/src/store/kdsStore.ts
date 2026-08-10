// Client-side view of kitchen state. This store is a cache of what the edge
// told us, never a second writer (ADR-014 §6, §50.1): every mutation here is
// driven by an inbound `KdsLanMessage`, except `beginPendingTransition`,
// which records that we *asked* — it never changes `status` itself.
import { create } from "zustand";
import type { Kot, KotStatus } from "@holler/contracts";

export type ConnectionStatus = "connecting" | "connected" | "stale" | "disconnected";

export interface PendingTransition {
  requestedStatus: KotStatus;
  requestedAt: number;
  /** Set once `transitionTimeoutMs` elapses with no confirming message. The
   * cook sees "not confirmed" rather than a screen that silently forgot the
   * tap or silently pretended it worked. */
  timedOut: boolean;
}

interface KdsState {
  kots: Record<string, Kot>;
  connectionStatus: ConnectionStatus;
  lastMessageAt: number | null;
  pendingByKotId: Record<string, PendingTransition>;

  applySnapshot: (kots: Kot[]) => void;
  upsertKot: (kot: Kot) => void;
  removeKot: (kotId: string) => void;
  setConnectionStatus: (status: ConnectionStatus) => void;
  noteMessageReceived: (atMs: number) => void;
  beginPendingTransition: (kotId: string, requestedStatus: KotStatus, atMs: number) => void;
  timeoutPendingTransition: (kotId: string) => void;
  clearAll: () => void;
}

export const useKdsStore = create<KdsState>((set) => ({
  kots: {},
  connectionStatus: "connecting",
  lastMessageAt: null,
  pendingByKotId: {},

  // Snapshot replaces the whole active set — it is the only way this store
  // becomes correct after a reconnect. It also clears stale pending
  // transitions: whatever the edge's authoritative state says now wins.
  applySnapshot: (kots) =>
    set(() => {
      const byId: Record<string, Kot> = {};
      for (const kot of kots) byId[kot.id] = kot;
      return { kots: byId, pendingByKotId: {} };
    }),

  upsertKot: (kot) =>
    set((state) => {
      const pending = { ...state.pendingByKotId };
      delete pending[kot.id];
      return {
        kots: { ...state.kots, [kot.id]: kot },
        pendingByKotId: pending,
      };
    }),

  removeKot: (kotId) =>
    set((state) => {
      const kots = { ...state.kots };
      delete kots[kotId];
      const pending = { ...state.pendingByKotId };
      delete pending[kotId];
      return { kots, pendingByKotId: pending };
    }),

  setConnectionStatus: (status) => set({ connectionStatus: status }),

  noteMessageReceived: (atMs) => set({ lastMessageAt: atMs }),

  beginPendingTransition: (kotId, requestedStatus, atMs) =>
    set((state) => ({
      pendingByKotId: {
        ...state.pendingByKotId,
        [kotId]: { requestedStatus, requestedAt: atMs, timedOut: false },
      },
    })),

  timeoutPendingTransition: (kotId) =>
    set((state) => {
      const existing = state.pendingByKotId[kotId];
      if (!existing || existing.timedOut) return state;
      return {
        pendingByKotId: {
          ...state.pendingByKotId,
          [kotId]: { ...existing, timedOut: true },
        },
      };
    }),

  clearAll: () => set({ kots: {}, pendingByKotId: {} }),
}));
