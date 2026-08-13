// Kitchen-facing display/action rules (docs/spec/kitchen.md, ADR-014).
// Business logic lives here, not in JSX (CLAUDE.md §Coding rules).

import type { AuthenticatedPrincipal, Kot, KotStatus, OrderStatus } from "@holler/contracts";
import { hasPermission } from "./permissions";

/** Mirrors `edge/database/src/repo.rs`'s `LEGAL_KOT_TRANSITIONS` exactly —
 * the UI must never offer a transition the edge will reject. NEW ->
 * ACKNOWLEDGED -> PREPARING -> READY -> SERVED, CANCELLED from any
 * non-terminal status. */
const LEGAL_KOT_TRANSITIONS: Record<KotStatus, readonly KotStatus[]> = {
  NEW: ["ACKNOWLEDGED", "CANCELLED"],
  ACKNOWLEDGED: ["PREPARING", "CANCELLED"],
  PREPARING: ["READY", "CANCELLED"],
  READY: ["SERVED"],
  SERVED: [],
  CANCELLED: [],
};

export function legalNextKotStatuses(status: KotStatus): readonly KotStatus[] {
  return LEGAL_KOT_TRANSITIONS[status];
}

export function canOfferKotTransition(
  principal: AuthenticatedPrincipal | null,
): boolean {
  // No dedicated kitchen permission exists in @holler/contracts yet
  // (PermissionSchema, identity.ts) — order.modify is the closest owning
  // permission for an order-state action and is what this task's other
  // order-mutation commands (confirm_order) already gate on.
  return hasPermission(principal, "order.modify");
}

/** docs/spec/kitchen.md §KDS: "never color-only, always show time/status
 * too." Applies wherever a KOT or order status is rendered, not just the
 * KDS screen itself — this is the one function every status badge in this
 * app must render through. */
export function kotStatusLabel(status: KotStatus): string {
  switch (status) {
    case "NEW":
      return "New";
    case "ACKNOWLEDGED":
      return "Acknowledged";
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

export function orderStatusLabel(status: OrderStatus): string {
  return status
    .toLowerCase()
    .split("_")
    .map((word) => word[0]!.toUpperCase() + word.slice(1))
    .join(" ");
}

/** A cashier-appropriate message for a rejected KOT transition or
 * send-to-kitchen call — never surfaces the raw error code/message crossing
 * the Tauri boundary for the documented failure modes. */
export function kitchenErrorMessage(err: unknown): string {
  const code = (err as { code?: unknown } | null)?.code;
  switch (code) {
    case "ORDER_NOT_SENDABLE_TO_KITCHEN":
      return "This order cannot be sent to the kitchen in its current status.";
    case "NOTHING_TO_SEND_TO_KITCHEN":
      return "Everything on this order has already been sent to the kitchen.";
    case "ILLEGAL_KOT_STATUS_TRANSITION":
      return "This ticket cannot move to that status from where it is now.";
    case "NO_PRINTER_ROUTED":
      return "No active printer is configured for that station.";
    case "UNROUTED_KITCHEN_ITEMS":
      // Unlike the other cases, the edge already built a cashier-legible,
      // item-naming message here (apps/pos/src-tauri/src/error.rs) —
      // "2 items have no kitchen station — not sent: <names>". Nothing was
      // sent to any kitchen, so surfacing it verbatim is the fix for
      // docs/backlog-m2.md's "mixed order sends silently" defect: the
      // cashier must be told *which* dish did not go, not just that
      // something failed (docs/spec/ordering.md §64).
      return unroutedKitchenItemsMessage(err);
    default:
      return "Could not complete the kitchen action. Please try again.";
  }
}

function unroutedKitchenItemsMessage(err: unknown): string {
  const message = (err as { message?: unknown } | null)?.message;
  if (typeof message === "string" && message.length > 0) return message;
  // Defensive fallback only — the edge always populates `message` for this
  // code (error.rs `UnroutedKitchenItems` arm), so this branch should be
  // unreachable in practice.
  return "Some items have no kitchen station and were not sent. Check with a manager before retrying.";
}

/** The order-level statuses from which "Send to Kitchen" is a legal action
 * per `require_sendable_order` (edge/database/src/repo.rs): CONFIRMED (first
 * send), SENT_TO_KITCHEN/PREPARING (a later send after items were added). */
const SENDABLE_ORDER_STATUSES: readonly OrderStatus[] = [
  "CONFIRMED",
  "SENT_TO_KITCHEN",
  "PREPARING",
];

export function canOfferSendToKitchen(
  status: OrderStatus,
  principal: AuthenticatedPrincipal | null,
): boolean {
  return SENDABLE_ORDER_STATUSES.includes(status) && hasPermission(principal, "order.modify");
}

/** Every station code a set of KOTs currently spans, for "which stations
 * this order routed to" (task requirement #2), de-duplicated and sorted. */
export function stationsForKots(kots: readonly Kot[]): string[] {
  return Array.from(new Set(kots.map((k) => k.station))).sort();
}
