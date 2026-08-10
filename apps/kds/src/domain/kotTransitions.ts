// KOT status flow driven by a cook's touch (docs/spec/kitchen.md §KDS,
// ADR-014 §6). The KDS is never authoritative for KOT state: this module
// only decides which transition button to *offer*. The edge validates the
// transition for real and answers with `kot_upserted` or a rejection; the UI
// renders whatever the edge confirms, not what was tapped.
import type { KotStatus } from "@holler/contracts";

// SERVED is the last state this screen offers a transition into. CANCELLED
// is edge/back-of-house driven (order-level), not a KDS button — kitchen.md
// does not ask KDS to cancel tickets, and Milestone 2 excludes the expo/pass
// screen where cross-cutting cancellation would live.
const FORWARD_FLOW: readonly KotStatus[] = [
  "NEW",
  "ACKNOWLEDGED",
  "PREPARING",
  "READY",
  "SERVED",
];

/** The single next status a cook can advance a ticket to from `current`, or
 * `null` if the ticket is already at the end of the flow (or in a state –
 * CANCELLED – that this screen never advances). */
export function nextStatus(current: KotStatus): KotStatus | null {
  const idx = FORWARD_FLOW.indexOf(current);
  if (idx === -1 || idx === FORWARD_FLOW.length - 1) return null;
  return FORWARD_FLOW[idx + 1];
}

/** Cook-facing label for the button that requests `nextStatus(current)`. */
export function nextStatusLabel(current: KotStatus): string | null {
  const next = nextStatus(current);
  switch (next) {
    case "ACKNOWLEDGED":
      return "Accept";
    case "PREPARING":
      return "Start preparing";
    case "READY":
      return "Mark ready";
    case "SERVED":
      return "Mark served";
    default:
      return null;
  }
}

export function statusLabel(status: KotStatus): string {
  switch (status) {
    case "NEW":
      return "New";
    case "ACKNOWLEDGED":
      return "Accepted";
    case "PREPARING":
      return "Preparing";
    case "READY":
      return "Ready";
    case "SERVED":
      return "Served";
    case "CANCELLED":
      return "Cancelled";
  }
}
