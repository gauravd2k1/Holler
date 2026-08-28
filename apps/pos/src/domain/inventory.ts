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

/**
 * Micro-units per DISPLAYED unit, which is not the same for every dimension.
 *
 * The edge stores micro-units of a BASE unit, and the base unit differs:
 * gram, LITRE and piece, each x10^6 (CLAUDE.md, edge/database/src/inventory/
 * units.rs — `grams`/`pieces` multiply by 1e6, `litres` by 1e6, `millilitres`
 * by 1e3).
 *
 * So dividing every dimension by 1e6 and printing "ml" — which this module did
 * until 2026-08-27 — reported every VOLUME quantity as LITRES under a
 * millilitre label, understating it 1000-fold. Soda Water's `litres(5)`
 * reorder level rendered as "5ml", which read as an absurd threshold rather
 * than as the display bug it was. Deduction was never affected: storage and
 * recipe authoring were consistent throughout, and only this formatter was
 * wrong.
 *
 * Entry uses the same units these print, so the two agree:
 * `human_quantity_to_micro` (apps/pos/src-tauri/src/commands/inventory.rs)
 * reads MASS as grams, VOLUME as millilitres, COUNT as pieces.
 */
function microPerDisplayUnit(dimension: string): number {
  return (dimension as InventoryDimension) === "VOLUME" ? 1_000 : MICRO_PER_UNIT;
}

/** The three inventory dimensions the edge stores on `inventory_item.dimension`
 * (ADR-018) — MASS in micro-grams, VOLUME in micro-LITRES, COUNT in
 * micro-pieces; displayed as g / ml / pcs respectively. Any
 * other string is a contract drift the frontend has never seen, so callers
 * fall back to a neutral label rather than guessing. */
export type InventoryDimension = "MASS" | "VOLUME" | "COUNT";

/**
 * The unit a human TYPES for this dimension, spelled out.
 *
 * Entry is grams / millilitres / pieces -- `human_quantity_to_micro`
 * (apps/pos/src-tauri/src/commands/inventory.rs) is the authority, and this
 * must not drift from it. An unlabelled quantity field is a 1000x error
 * waiting to happen: someone counting oil in litres types 5, records five
 * millilitres, and a mis-entered count feeds the variance report directly.
 * Every quantity input names its unit for the SELECTED item, because the unit
 * depends on that item's dimension and not on the screen.
 */
export function entryUnitName(dimension: string): string {
  switch (dimension as InventoryDimension) {
    case "MASS":
      return "grams";
    case "VOLUME":
      return "millilitres";
    case "COUNT":
      return "pieces";
    default:
      return dimension;
  }
}

/**
 * The one-line restatement shown live under a quantity field.
 *
 * The failure this catches is INTENT, not reading: someone counting oil in
 * litres types 5 whatever the field is labelled, because they are counting,
 * not reading a form. A label is read once when the screen opens; this line
 * changes under the cursor as the digits land, and reads back the number in
 * the unit it will actually be stored as, naming the item so the wrong row
 * is caught in the same glance.
 *
 * Deliberately NOT a converted micro-unit figure. Re-deriving the edge's
 * conversion in TypeScript is how the two drift, and the edge is the
 * authority on quantity exactly as it is on money. This restates what was
 * typed -- no arithmetic beyond digit grouping, which is itself part of the
 * signal: 5000 renders "5,000 millilitres", not an undifferentiated run of
 * zeroes.
 *
 * The unit word is spelled out rather than symbolised ("millilitres", not
 * "ml") because a symbol is glanceable-past and this line exists to be read.
 *
 * `qualifier` exists because the two callers capture semantically different
 * quantities and a parallel verb hides it. Wastage is a MOVEMENT, and
 * "Wasting 5 millilitres" is unambiguous. A count is a BALANCE, and
 * "Counting 5 millilitres" reads just as easily as "recording that 5ml was
 * used" -- someone reading it that way enters a consumption figure into a
 * balance field, which is a 100% variance error that looks entirely
 * reasonable on screen. The count screen closes that with "on hand".
 */
export function entryIntentEcho(
  verb: string,
  quantity: number,
  dimension: string,
  itemName: string,
  qualifier?: string,
): string {
  const line = `${verb} ${quantity.toLocaleString("en-IN")} ${entryUnitName(dimension)} of ${itemName}`;
  return qualifier ? `${line} ${qualifier}` : line;
}

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
 * "1.5g", `formatMicroQuantity(-250_000, "VOLUME")` -> "-250ml",
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
  const perUnit = microPerDisplayUnit(dimension);
  const whole = Math.trunc(abs / perUnit);
  const remainder = abs % perUnit;
  const sign = negative ? "-" : "";
  const unit = unitLabel(dimension);
  if (remainder === 0) {
    return `${sign}${whole}${unit}`;
  }
  // Pad to the scale actually in use — 6 digits for gram/piece micro-units,
  // 3 for the micro-litres a millilitre is made of. Padding to 6 for VOLUME
  // would print 250 micro-litres as ".000250ml" instead of ".25ml".
  const width = String(perUnit).length - 1;
  const remainderStr = remainder.toString().padStart(width, "0").replace(/0+$/, "");
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

/**
 * `true` when the books say you hold less than nothing.
 *
 * UNCONDITIONAL — a reorder level is not consulted, and must never be. The
 * two are different signals with different audiences: a reorder level answers
 * "should I buy more", which needs a threshold somebody chose, while a
 * negative balance answers "my books are wrong — I sold what I do not have",
 * which needs no configuration to be worth saying.
 *
 * Before 2026-08-27 negatives borrowed the low-stock threshold, so Red Chilli
 * Powder at -1.6 g was flagged and Salt at -1.2 g was flagged NOTHING, purely
 * because nobody had configured salt. A real failure with an absent signal, in
 * the feature built to prevent exactly that.
 *
 * Negative stock remains PERMITTED (ADR-018 Rule 1: stock never blocks a
 * sale). Surfacing it is not blocking it.
 */
export function isNegativeStock(line: CurrentStockLine): boolean {
  return line.current_quantity_micro < 0;
}

export function negativeStockLines(lines: readonly CurrentStockLine[]): CurrentStockLine[] {
  return lines.filter(isNegativeStock);
}

/**
 * Lines needing a human's attention, negatives first.
 *
 * A negative line is reported ONCE even when it also sits under a configured
 * reorder level — it is the stronger statement, and listing an item twice
 * teaches a cashier to skim the banner.
 */
export function stockAttentionLines(lines: readonly CurrentStockLine[]): {
  negative: CurrentStockLine[];
  low: CurrentStockLine[];
} {
  const negative = negativeStockLines(lines);
  const negativeIds = new Set(negative.map((l) => l.inventory_item_id));
  return {
    negative,
    low: lowStockLines(lines).filter((l) => !negativeIds.has(l.inventory_item_id)),
  };
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
