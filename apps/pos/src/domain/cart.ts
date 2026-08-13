import type { OrderType } from "@holler/contracts";
import { sumPaise } from "./money";

// Milestone 1 excludes tax/discount computation (Milestone 3) — a cart line
// carries only quantity * unit price. The order-level tax/discount fields
// that CanonicalOrder carries are displayed once the backend returns them,
// never computed here.
//
// M3 Track B (docs/m3-planning.md): a line's total is no longer
// `unitPricePaise * quantity` recomputed here — that formula silently
// ignores modifier price deltas, which is exactly the fiction the M2->M3
// close-out called out ("204/204 scenarios without ever exercising a
// modifier price delta"). `lineTotalPaise` below is instead the edge's own
// computed `order_item.line_total_paise`, carried through unchanged — the
// one definition of the money invariant lives in `edge/database`
// (`packages/contracts/sqlite/0003_order_item_modifiers.sql`'s "MONEY
// INVARIANT" comment), and this module must not duplicate it with a formula
// that can drift from that one.

export interface CartLineModifier {
  modifierId: string;
  groupName: string;
  optionName: string;
  priceDeltaPaise: number;
}

export interface CartLine {
  /**
   * The persisted `order_item.id` this line mirrors. Every `CartLine` the
   * UI ever renders corresponds to a row already committed to the edge's
   * SQLite database (docs/backlog-m2.md "POS cart persistence") — there is
   * no client-only, not-yet-written cart state, so this is never a
   * throwaway client-generated id.
   */
  lineId: string;
  menuItemId: string;
  menuItemName: string;
  variantId: string | null;
  unitPricePaise: number;
  quantity: number;
  notes: string | null;
  /** This line's real modifier selections, as persisted — never trusted to
   * be inferred from what the cashier tapped locally. */
  modifiers: CartLineModifier[];
  /** The edge's computed `(unit_price_paise + sum(modifier price deltas)) *
   * quantity`. Authoritative — never recomputed client-side. */
  lineTotalPaise: number;
}

/** The order types this milestone supports (docs/spec/ordering.md). */
export const SUPPORTED_ORDER_TYPES: readonly OrderType[] = [
  "DINE_IN",
  "TAKEAWAY",
  "DELIVERY",
];

export function lineTotal(line: CartLine): number {
  return line.lineTotalPaise;
}

export function cartSubtotalPaise(lines: readonly CartLine[]): number {
  return sumPaise(lines.map(lineTotal));
}

/** A DINE_IN order must have a table selected before it can be sent. */
export function requiresTable(orderType: OrderType): boolean {
  return orderType === "DINE_IN";
}

export function canSendOrder(orderType: OrderType, tableId: string | null, lines: readonly CartLine[]): boolean {
  if (lines.length === 0) return false;
  if (requiresTable(orderType) && !tableId) return false;
  return true;
}
