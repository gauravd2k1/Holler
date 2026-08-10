// Pure translation between the wire `CanonicalOrder` (what SQLite actually
// holds, via the Tauri boundary) and the cart's local `CartLine[]`. Kept
// separate from `store/cart.ts` so the mapping — the part that must be
// exactly right for crash recovery to show the truth — is unit-testable
// without a Zustand store or a Tauri mock in the way.
//
// docs/backlog-m2.md "POS cart persistence" (reopened 2026-08-10): the cart
// is never allowed to diverge from what `CanonicalOrder` says is durable.
// Every cart mutation in this app re-derives its lines from the order the
// edge just returned rather than adjusting local state optimistically — this
// module is the one place that derivation happens.

import type { CanonicalOrder, MenuItem } from "@holler/contracts";
import type { CartLine } from "./cart";

/** Resolves a menu item's display name for cart rendering. `CanonicalOrder`
 * items carry only `menu_item_id` (docs/spec/ordering.md — line items are a
 * price/quantity snapshot, not a menu join), so recovering a cart after
 * restart needs the current menu loaded to show a name at all. Falls back to
 * the raw id — never hides a recovered line for want of a name. */
export function menuItemNameResolver(menuItems: readonly MenuItem[]): (menuItemId: string) => string {
  const nameById = new Map(menuItems.map((item) => [item.id, item.name] as const));
  return (menuItemId) => nameById.get(menuItemId) ?? menuItemId;
}

/** The only place a `CanonicalOrder` becomes cart state. Non-DRAFT orders
 * have no cart representation — this app's cart only ever mirrors a DRAFT
 * order (docs/spec/ordering.md: amendment is DRAFT-only). */
export function orderToCartLines(
  order: Pick<CanonicalOrder, "items">,
  resolveName: (menuItemId: string) => string,
): CartLine[] {
  return order.items.map((item) => ({
    lineId: item.id,
    menuItemId: item.menu_item_id,
    menuItemName: resolveName(item.menu_item_id),
    variantId: item.variant_id,
    unitPricePaise: item.unit_price_paise,
    quantity: item.quantity,
    notes: item.notes,
  }));
}

/** Whether a recovered order is one the cart should actually adopt: DRAFT
 * (amendable) — a CONFIRMED/later order belongs on the Orders screen, not
 * back in an editable cart. */
export function isRecoverableDraft(order: Pick<CanonicalOrder, "status">): boolean {
  return order.status === "DRAFT";
}
