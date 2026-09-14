/**
 * Till interaction latency, measured and reported by the till itself.
 *
 * The companion to `apps/kds/src/lib/perf.ts`, and it exists for the same
 * reason: the `HOLLER-PERF` markers were two bare timestamps in a console,
 * correlated by order id and subtracted BY HAND. Nobody does that under demo
 * pressure, so the number stayed unmeasured. A measurement nobody can read is
 * not a measurement.
 *
 * SIMPLER THAN THE KDS CASE, AND WORTH SAYING WHY. Both ends of this interval
 * are stamped on THIS machine, in one process, by one clock. There is no skew
 * to report and no second number to disambiguate it — unlike the KDS, where
 * `sent_at` comes off the till's clock and the paint happens on the kitchen
 * screen's. One number here is the whole truth.
 *
 * WHAT IT MEASURES. `till_bill_tapped` is stamped on the TAP itself, not after
 * the router agrees to move, because the interval a cashier feels starts when
 * their finger lands. `till_bill_screen_opened` is stamped when the billing
 * screen mounts. So this covers navigation, the invoice query and first paint
 * — everything between the finger and something to look at.
 *
 * Over 1s is a demo blocker (CLAUDE.md, scope until Wednesday).
 */

export interface BillOpenLatency {
  orderId: string;
  ms: number;
}

const pending = new Map<string, number>();
const completed: BillOpenLatency[] = [];
const MAX_KEPT = 50;

/** Stamped on the Bill tap, before navigation is requested. */
export function markBillTapped(orderId: string, atMs = Date.now()): void {
  pending.set(orderId, atMs);
}

/**
 * Stamped when the billing screen mounts. Returns null when this open was not
 * preceded by a tap we saw — a deep link, a reload, or a back-navigation —
 * because timing those against a stale tap would invent a number.
 */
export function markBillOpened(orderId: string, atMs = Date.now()): BillOpenLatency | null {
  const tappedAt = pending.get(orderId);
  if (tappedAt === undefined) return null;
  pending.delete(orderId);

  const latency: BillOpenLatency = { orderId, ms: atMs - tappedAt };
  completed.push(latency);
  if (completed.length > MAX_KEPT) completed.shift();
  return latency;
}

/** Last and worst. The average is deliberately absent, same as the KDS: one
 * slow bill in front of a client is the event, and an average hides it. */
export function billOpenSummary(): { last: BillOpenLatency | null; worstMs: number | null; count: number } {
  if (completed.length === 0) return { last: null, worstMs: null, count: 0 };
  return {
    last: completed[completed.length - 1]!,
    worstMs: Math.max(...completed.map((l) => l.ms)),
    count: completed.length,
  };
}

// ---------------------------------------------------------------- toggle --

const STORAGE_KEY = "holler.perf";

/** The till has no address bar, so `?perf=1` (the KDS's toggle) is not
 * reachable here. Ctrl+Alt+P toggles it instead and the choice persists, so a
 * rehearsal can turn it on once. Every access is wrapped: localStorage throws
 * in some webview configurations, and an instrumentation toggle must never be
 * the thing that takes the till down. */
export function isPerfEnabled(): boolean {
  try {
    return window.localStorage.getItem(STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

export function setPerfEnabled(on: boolean): void {
  try {
    if (on) window.localStorage.setItem(STORAGE_KEY, "1");
    else window.localStorage.removeItem(STORAGE_KEY);
  } catch {
    /* ignore — the overlay simply will not persist */
  }
}

/** Test seam. Never called by the app. */
export function resetPerfForTest(): void {
  pending.clear();
  completed.length = 0;
}
