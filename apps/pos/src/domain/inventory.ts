// Milestone 4 (ADR-018) POS-side inventory logic — units, low-stock
// detection, permission gates and error display. Pure functions only; every
// screen in `components/` calls into this module rather than computing any
// of this inline (CLAUDE.md: business logic outside UI components).
//
// Quantities are integer MICRO-units end to end (value * 10^6, scale in the
// field name) — the money=paise rule generalised (CLAUDE.md v0.5.0). This
// module formats for display only, exactly as `domain/money.ts` formats
// paise: integer div/mod, never a float division of a micro-quantity by
// 1_000_000.

import type { AuthenticatedPrincipal } from "@holler/contracts";
import { hasPermission } from "./permissions";
import { isTauriCommandError } from "../lib/tauri";
import type { CurrentStockLine, StockCount, StockCountVarianceReport } from "../lib/tauri";

const MICRO_PER_UNIT = 1_000_000;

/** The three inventory dimensions the edge stores on `inventory_item.dimension`
 * (ADR-018) — MASS in grams, VOLUME in millilitres, COUNT in pieces. Any
 * other string is a contract drift the frontend has never seen, so callers
 * fall back to a neutral label rather than guessing. */
export type InventoryDimension = "MASS" | "VOLUME" | "COUNT";

function unitLabel(dimension: string): string {
  switch (dimension as InventoryDimension) {
    case "MASS":
      return "g";
    case "VOLUME":
      return "ml";
    case "COUNT":
      return "pcs";
    default:
      return dimension;
  }
}

/** Throws if `micro` is not a safe integer — callers must never pass a float
 * micro-quantity (the same discipline `money.ts`'s `assertIntegerPaise`
 * applies to paise). */
function assertIntegerMicro(micro: number): void {
  if (!Number.isInteger(micro)) {
    throw new Error(`quantity must be an integer number of micro-units, got ${micro}`);
  }
}

/**
 * Formats an integer micro-quantity plus its dimension as a human quantity
 * with a unit suffix, e.g. `formatMicroQuantity(1_500_000, "MASS")` ->
 * "1.5g", `formatMicroQuantity(-250_000, "VOLUME")` -> "-0.25ml",
 * `formatMicroQuantity(0, "COUNT")` -> "0pcs".
 *
 * Uses only integer arithmetic (div/mod by 1_000_000) and string
 * manipulation — never a float division. The fractional remainder is shown
 * verbatim, trimmed of trailing zeros, never rounded: this is a lossless
 * display of an exact stored integer, not an approximation.
 *
 * Negative stock is LEGAL and is a variance signal, never an error (ADR-018
 * §"Stock never blocks a sale") — this function never clamps a negative
 * value to zero and never special-cases it as a failure.
 */
export function formatMicroQuantity(micro: number, dimension: string): string {
  assertIntegerMicro(micro);
  const negative = micro < 0;
  const abs = Math.abs(micro);
  const whole = Math.trunc(abs / MICRO_PER_UNIT);
  const remainder = abs % MICRO_PER_UNIT;
  const sign = negative ? "-" : "";
  const unit = unitLabel(dimension);
  if (remainder === 0) {
    return `${sign}${whole}${unit}`;
  }
  const remainderStr = remainder.toString().padStart(6, "0").replace(/0+$/, "");
  return `${sign}${whole}.${remainderStr}${unit}`;
}

/**
 * `StockDeductionGap.quantity` is a plain count of sellable units — NOT a
 * micro-quantity (`apps/pos/src-tauri/src/dto.rs` `StockDeductionGap` doc
 * comment: "nothing resolved to an ingredient, which is the point of the
 * row"). This is the ONLY correct formatter for that field; running it
 * through `formatMicroQuantity` would silently divide a whole-number sale
 * count by a million.
 */
export function formatGapQuantity(quantity: number): string {
  if (!Number.isInteger(quantity)) {
    throw new Error(`gap quantity must be an integer count, got ${quantity}`);
  }
  return `${quantity}`;
}

// ------------------------------------------------------------ low stock --
// M4 acceptance criterion 4: an ingredient crossing its reorder level must
// be visible to a human on the POS, not merely present in a table.

/** `true` only when a reorder level is actually configured — a `null`
 * `reorder_level_micro` means no threshold is configured, and such an item
 * is never "low" (task requirement: never silently treat an unconfigured
 * item as low). */
export function isLowStock(line: CurrentStockLine): boolean {
  if (line.reorder_level_micro === null) return false;
  return line.current_quantity_micro <= line.reorder_level_micro;
}

export function lowStockLines(lines: readonly CurrentStockLine[]): CurrentStockLine[] {
  return lines.filter(isLowStock);
}

// ------------------------------------------------------------ permissions --
// `inventory.manage` and `inventory.count` (`packages/contracts` identity.ts
// `PermissionSchema`). Reads a cashier needs during service — the low-stock
// signal itself — are deliberately NOT gated behind either permission: any
// authenticated principal may see that stock is low, the same way any
// cashier already sees `PrintFailureBanner` with no permission check. The
// gates below apply only to the write actions and to the two
// management-level report/detail screens (current stock detail, items-sold-
// with-no-recipe report), which is a judgement call this track's dispatch
// left to the builder.

export function canManageInventory(principal: AuthenticatedPrincipal | null): boolean {
  return hasPermission(principal, "inventory.manage");
}

export function canCountInventory(principal: AuthenticatedPrincipal | null): boolean {
  return hasPermission(principal, "inventory.count");
}

// ----------------------------------------------------------- error display --
// §64 is binding: every one of these must tell a cashier whether
// intervention is necessary and what it is, never "Something went wrong".

export function inventoryErrorMessage(err: unknown): string {
  if (!isTauriCommandError(err)) {
    return "Could not complete the inventory action. Please try again.";
  }
  switch (err.code) {
    case "NOT_FOUND":
      return err.message;
    case "UNKNOWN_DIMENSION":
      return err.message;
    case "WASTAGE_REASON_REQUIRED":
      return "A reason is required for a wastage entry.";
    case "WASTAGE_QUANTITY_NOT_POSITIVE":
      return "Enter a quantity greater than zero for a wastage entry.";
    case "STOCK_COUNT_NOT_OPEN":
      // The edge's own message already names the count id and its current
      // status (error.rs `StockCountNotOpen`) — shown verbatim per §64. This
      // is the "surface the rejection as a clear message, not a silent
      // no-op" case for a completed count.
      return err.message;
    default:
      return "Could not complete the inventory action. Please try again.";
  }
}

/** `true` only for the one case a stock-count-line screen must recover from
 * by explaining the count is no longer editable, rather than treating the
 * edit as a generic failure. */
export function isStockCountNotOpen(err: unknown): boolean {
  return isTauriCommandError(err) && err.code === "STOCK_COUNT_NOT_OPEN";
}

export function isCountOpen(count: StockCount | null | undefined): boolean {
  return count?.status === "OPEN";
}

/** Purely for the variance screen's own guard against fetching a report for
 * a count that has not been completed yet — the edge's own report query is
 * still the sole authority on the arithmetic inside `StockCountVarianceReport`
 * (CLAUDE.md: never recompute variance arithmetic in TypeScript). */
export function isCountCompleted(count: StockCount | null | undefined): boolean {
  return count?.status === "COMPLETED";
}

/**
 * Formats a basis-point variance percentage (`StockCountVarianceLine
 * .variance_percentage_bps`, 100 bps = 1%) as a percent string, e.g. `250`
 * -> "2.50%", `-50` -> "-0.50%". Integer div/mod only, the same discipline
 * `formatMicroQuantity`/`money.ts` apply — never a float division of bps by
 * 100.
 */
export function formatVarianceBps(bps: number): string {
  if (!Number.isInteger(bps)) {
    throw new Error(`variance_percentage_bps must be an integer, got ${bps}`);
  }
  const negative = bps < 0;
  const abs = Math.abs(bps);
  const whole = Math.trunc(abs / 100);
  const remainder = (abs % 100).toString().padStart(2, "0");
  const sign = negative ? "-" : "";
  return `${sign}${whole}.${remainder}%`;
}

/** Re-exported purely so screens importing from this module do not also need
 * a direct import from `lib/tauri` just for the type. */
export type { StockCountVarianceReport };
