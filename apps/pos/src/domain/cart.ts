import type { OrderType } from "@holler/contracts";
import { lineTotalPaise, sumPaise } from "./money";

// Milestone 1 excludes tax/discount computation (Milestone 3) — a cart line
// carries only quantity * unit price. The order-level tax/discount fields
// that CanonicalOrder carries are displayed once the backend returns them,
// never computed here.

export interface CartLine {
  /** Client-generated id, stable for this cart line until sent. */
  lineId: string;
  menuItemId: string;
  menuItemName: string;
  variantId: string | null;
  unitPricePaise: number;
  quantity: number;
  notes: string | null;
}

/** The order types this milestone supports (docs/spec/ordering.md). */
export const SUPPORTED_ORDER_TYPES: readonly OrderType[] = [
  "DINE_IN",
  "TAKEAWAY",
  "DELIVERY",
];

export function lineTotal(line: CartLine): number {
  return lineTotalPaise(line.unitPricePaise, line.quantity);
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
