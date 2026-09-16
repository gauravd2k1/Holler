// Kitchen-facing display/action rules (docs/spec/kitchen.md, ADR-014).
// Business logic lives here, not in JSX (CLAUDE.md §Coding rules).

import type { AuthenticatedPrincipal, Kot, KotStatus, OrderStatus } from "@holler/contracts";
import { hasPermission } from "./permissions";

/** The edge's transition table, as the UI consumes it. Built from what
 * `list_kot_status_transitions` returns — there is no copy of the table in
 * this file any more.
 *
 * THERE USED TO BE ONE, with a comment promising it "mirrors
 * `edge/database/src/repo.rs`'s `LEGAL_KOT_TRANSITIONS` exactly". Nothing
 * could check that promise, and a UI that offers a move the edge refuses is
 * the D14 defect seen from the other side: VV-009 watched the till offer
 * "Acknowledged" for a ticket the kitchen had already acknowledged and get
 * "This ticket cannot move to that status from where it is now." */
export type KotTransitionTable = ReadonlyMap<KotStatus, readonly KotStatus[]>;

export function buildKotTransitionTable(
  pairs: readonly (readonly [string, readonly string[]])[],
): KotTransitionTable {
  return new Map(
    pairs.map(([from, tos]) => [from as KotStatus, tos as readonly KotStatus[]]),
  );
}

/** The moves legal from `status`, according to the edge.
 *
 * An ABSENT table yields NO moves, deliberately. Until the edge has answered,
 * this screen cannot know which buttons are safe to offer, and offering one
 * on a guess is exactly what produced the rejection in VV-009. A terminal
 * status legitimately has no entry, and returns nothing for the same
 * reason. */
export function legalNextKotStatuses(
  table: KotTransitionTable | undefined,
  status: KotStatus,
): readonly KotStatus[] {
  return table?.get(status) ?? [];
}

/** The verb a person presses, as opposed to the state the ticket lands in.
 * A button labelled "Acknowledged" describes a status; a button labelled
 * "Acknowledge" describes what pressing it does. */
export function kotTransitionActionLabel(status: KotStatus): string {
  switch (status) {
    case "ACKNOWLEDGED":
      return "Acknowledge";
    case "PREPARING":
      return "Start preparing";
    case "READY":
      return "Mark ready";
    case "SERVED":
      return "Mark served";
    case "CANCELLED":
      return "Cancel ticket";
    case "NEW":
      return "Reopen";
  }
}

/** The CSS modifier for a status badge. Colour is ADDED to the text label,
 * never substituted for it (docs/spec/kitchen.md §KDS: "never colour-only,
 * always show time/status too") — `kotStatusLabel` still renders inside. */
export function kotStatusToneClass(status: KotStatus): string {
  switch (status) {
    case "NEW":
      return "kot-status kot-status--new";
    case "ACKNOWLEDGED":
      return "kot-status kot-status--acknowledged";
    case "PREPARING":
      return "kot-status kot-status--preparing";
    case "READY":
      return "kot-status kot-status--ready";
    case "SERVED":
      return "kot-status kot-status--served";
    case "CANCELLED":
      return "kot-status kot-status--cancelled";
  }
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

/**
 * SCREAMING_SNAKE to Title Case, for any contract enum a human reads.
 *
 * Takes a plain string rather than `OrderStatus`: the formatting has nothing
 * to do with which enum it came from, and narrowing it forced a cast at the
 * order-type column (`DINE_IN` was reaching the screen raw because the only
 * humaniser in the app refused to accept it). A cast to satisfy a type that
 * was too narrow is the type being wrong, not the call.
 */
export function orderStatusLabel(status: string): string {
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
