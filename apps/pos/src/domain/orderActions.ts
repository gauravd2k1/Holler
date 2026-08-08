import type { AuthenticatedPrincipal, OrderStatus } from "@holler/contracts";
import { hasPermission } from "./permissions";
import { isTauriCommandError } from "../lib/tauri";

/** An order offers the confirm action only while it is DRAFT — the edge
 * enforces DRAFT-only amendment (ordering.md), so the UI must not offer an
 * action that will be rejected for any other status. */
export function canOfferConfirm(
  status: OrderStatus,
  principal: AuthenticatedPrincipal | null,
): boolean {
  return status === "DRAFT" && hasPermission(principal, "order.modify");
}

/** A cashier-appropriate message for a rejected confirm. Never surfaces the
 * raw error code/message crossing the Tauri boundary — the one documented
 * failure mode (order no longer DRAFT) gets a plain-language explanation,
 * everything else falls back to a generic retry message. */
export function confirmErrorMessage(err: unknown): string {
  if (isTauriCommandError(err) && err.code === "ORDER_NOT_CONFIRMABLE") {
    return "This order can no longer be confirmed — it may have already been confirmed elsewhere.";
  }
  return "Could not confirm the order. Please try again.";
}
