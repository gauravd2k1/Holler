/**
 * Ticket latency, measured and reported by the KDS itself.
 *
 * WHY THIS EXISTS. The `HOLLER-PERF` markers on both sides were timestamps
 * and nothing more: one line in the POS terminal, one in the browser console,
 * correlated by order id and subtracted BY HAND. That is a half-built
 * feature. Nobody does that arithmetic under demo pressure, so the number
 * stayed unmeasured for days and the honest entry in docs/demo-status.md
 * stayed empty. A measurement nobody can read is not a measurement.
 *
 * Nothing new goes on the wire: `KdsLanMessage::KotUpserted` has always
 * carried `sent_at` (edge/device/src/contract.rs). The KDS can therefore
 * compute the whole interval on its own.
 *
 * TWO NUMBERS, AND THE DIFFERENCE MATTERS.
 *
 *   wireMs    sent_at (the TILL's clock) -> painted (THIS device's clock).
 *             The end-to-end number, and the one a person means by "how long
 *             did it take to reach the kitchen". It includes CLOCK SKEW
 *             between the two machines. On a KDS running on the till itself
 *             that is zero and the number is exact. On a phone or a second
 *             laptop whose clock is thirty seconds off, this reads thirty
 *             seconds off -- and a phone clock CAN be off. It is reported
 *             anyway, because it is the real question, but never alone.
 *
 *   renderMs  message received -> painted, both on THIS device's clock.
 *             Immune to skew. It cannot see network time, so it is not the
 *             answer on its own either -- but if wireMs looks absurd and
 *             renderMs is 4ms, the fault is the clocks, not the software.
 *
 * Reporting one without the other is how a skewed clock gets mistaken for a
 * performance problem at exactly the wrong moment.
 */

export interface TicketLatency {
  orderId: string;
  /** Till clock -> this device's clock. Includes any skew between them. */
  wireMs: number;
  /** Received -> painted, entirely on this device's clock. Skew-free. */
  renderMs: number;
}

interface Pending {
  sentAtMs: number;
  receivedAtMs: number;
}

const pending = new Map<string, Pending>();
const completed: TicketLatency[] = [];
const MAX_KEPT = 50;

/** Called when a `kot_upserted` frame arrives, before React has rendered it. */
export function noteTicketReceived(kotId: string, sentAt: string, receivedAtMs = Date.now()): void {
  const sentAtMs = Date.parse(sentAt);
  // An unparseable sent_at must not poison the table with NaN. Drop the
  // sample rather than record a number that is not one.
  if (Number.isNaN(sentAtMs)) return;
  // FIRST arrival only. A ticket is upserted again on every status change,
  // and overwriting here would silently re-time an already-painted ticket
  // against a later frame -- turning a status bump into a fake 4ms "render".
  if (pending.has(kotId)) return;
  pending.set(kotId, { sentAtMs, receivedAtMs });
}

/**
 * Called the first time a ticket is painted. Returns null when there is
 * nothing to report — an unknown id, or one already completed — so the caller
 * cannot log a second, meaningless line for the same ticket.
 */
export function noteTicketPainted(
  kotId: string,
  orderId: string,
  paintedAtMs = Date.now(),
): TicketLatency | null {
  const entry = pending.get(kotId);
  if (!entry) return null;
  pending.delete(kotId);

  const latency: TicketLatency = {
    orderId,
    wireMs: paintedAtMs - entry.sentAtMs,
    renderMs: paintedAtMs - entry.receivedAtMs,
  };
  completed.push(latency);
  if (completed.length > MAX_KEPT) completed.shift();
  return latency;
}

export function recentLatencies(): readonly TicketLatency[] {
  return completed;
}

/** Worst case and most recent — the two a rehearsal actually needs. The
 * average is deliberately absent: one 3-second ticket in front of a client is
 * the event, and an average is what hides it. */
export function latencySummary(): { last: TicketLatency | null; worstWireMs: number | null; count: number } {
  if (completed.length === 0) return { last: null, worstWireMs: null, count: 0 };
  return {
    last: completed[completed.length - 1]!,
    worstWireMs: Math.max(...completed.map((l) => l.wireMs)),
    count: completed.length,
  };
}

/** Test seam. Never called by the app. */
export function resetPerfForTest(): void {
  pending.clear();
  completed.length = 0;
}
