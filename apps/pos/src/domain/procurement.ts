// Milestone 5 (ADR-019) POS-side procurement logic — receiving, purchase
// returns and the human-visible GRN gap report. Pure functions only; every
// screen in `components/` calls into this module rather than computing any of
// this inline (CLAUDE.md: business logic outside UI components).
//
// ---------------------------------------------------------------------------
// NOTHING HERE CONVERTS A PURCHASE UNIT, AND NOTHING HERE COMPUTES A COST.
// ---------------------------------------------------------------------------
//
// ADR-019 §3: the conversion happens EXACTLY ONCE, at the edge. The
// `entryIntentEcho` this module formats is produced by the same edge function
// the write runs (`Db::grn_entry_intent_echo`), so the echo cannot disagree
// with what is recorded. An echo computed independently in TypeScript would
// be worse than no echo at all — it would agree with itself and with nothing
// else. Everything below is display formatting over integers the edge
// supplied: integer div/mod only, never a float, exactly as `money.ts` treats
// paise and `inventory.ts` treats micro-units.

import type { AuthenticatedPrincipal } from "@holler/contracts";
import { GrnGapReasonSchema, PurchaseReturnReasonSchema } from "@holler/contracts";
import type { GrnGapReason, PurchaseReturnReason, QuantityDimension } from "@holler/contracts";
import { hasPermission } from "./permissions";
import { formatMicroQuantity } from "./inventory";
import { isTauriCommandError } from "../lib/tauri";
import type { GrnEntryIntentEcho, GrnGap } from "../lib/tauri";

/** Micro-units per ONE purchase unit. `entered_quantity_micro` counts the
 * SUPPLIER's own unit — "3 sacks = 3_000_000" — so this is the plain ×10^6
 * fixed-point scale and is NOT the item's dimensional scale (a millilitre is
 * 1_000). Mirrors `MICRO_PER_PURCHASE_UNIT` in
 * `apps/pos/src-tauri/src/commands/procurement.rs`. */
const MICRO_PER_PURCHASE_UNIT = 1_000_000;

/** The three dimensions, re-exported so screens need one import. */
export type { QuantityDimension };

/** Every dimension an operator may DECLARE on a receiving line.
 *
 * THE OPERATOR PICKS THIS. It is never defaulted from the selected inventory
 * item, and the receiving form deliberately starts it empty — if a UI
 * auto-filled it from `inventory_item.dimension`, the edge's
 * `DIMENSION_MISMATCH` comparison would become `x == x`, the guard could
 * never fire, and it would look entirely correct in review (contracts
 * 0.5.2/0.6.0, ADR-019 §6). This constant exists so the picker's options are
 * a closed set; it is not a source of a default. */
export const DECLARABLE_DIMENSIONS: readonly QuantityDimension[] = ["MASS", "VOLUME", "COUNT"];

/** The six `purchase_return.reason` values, straight off the contract enum
 * rather than re-typed here — a re-typed list is a list that drifts. */
export const PURCHASE_RETURN_REASONS: readonly PurchaseReturnReason[] =
  PurchaseReturnReasonSchema.options;

/** The eight `grn_gap.reason` values, likewise from the contract enum. */
export const GRN_GAP_REASONS: readonly GrnGapReason[] = GrnGapReasonSchema.options;

// ------------------------------------------------------------ quantities --

function assertIntegerMicro(micro: number): void {
  if (!Number.isInteger(micro)) {
    throw new Error(`quantity must be an integer number of micro-units, got ${micro}`);
  }
}

/**
 * Formats an `entered_quantity_micro` — a count of the SUPPLIER's purchase
 * units — with that unit's own label: `formatEnteredQuantity(4_000_000,
 * "SACK")` -> "4 SACK", `formatEnteredQuantity(12_500_000, "kg")` ->
 * "12.5 kg".
 *
 * Integer div/mod only. Deliberately a separate function from
 * `formatMicroQuantity`: that one scales by the item's DIMENSION (a
 * millilitre is 1_000 micro-litres) and would mis-scale a purchase unit by
 * 1000× on any VOLUME item. Two different scales, two different functions, so
 * a call site cannot pick the wrong one by accident.
 */
export function formatEnteredQuantity(micro: number, purchaseUnit: string): string {
  assertIntegerMicro(micro);
  const negative = micro < 0;
  const abs = Math.abs(micro);
  const whole = Math.trunc(abs / MICRO_PER_PURCHASE_UNIT);
  const remainder = abs % MICRO_PER_PURCHASE_UNIT;
  const sign = negative ? "-" : "";
  if (remainder === 0) {
    return `${sign}${whole} ${purchaseUnit}`;
  }
  const remainderStr = remainder.toString().padStart(6, "0").replace(/0+$/, "");
  return `${sign}${whole}.${remainderStr} ${purchaseUnit}`;
}

/**
 * THE `entryIntentEcho` (M5 acceptance criterion 4), as one line an operator
 * reads at the door with a driver waiting: "4 SACK -> 200kg of Basmati Rice".
 *
 * Receiving is the third quantity-entry path in this product and the one with
 * the worst odds — larger quantities than a stock count, read off a delivery
 * note in the supplier's units, typed by whoever is standing at the door.
 * When a receipt turns out 1000× wrong, this line is what would have caught
 * it, so it states BOTH SIDES of the conversion: what was typed, and what
 * will actually be recorded against stock.
 *
 * Every number in it came from the edge's own resolution. This function does
 * arithmetic on none of them.
 */
export function entryIntentEcho(echo: GrnEntryIntentEcho): string {
  const entered = formatEnteredQuantity(echo.entered_quantity_micro, echo.entered_purchase_unit);
  const base = formatMicroQuantity(echo.base_quantity_micro, echo.item_dimension);
  return `${entered} → ${base} of ${echo.inventory_item_name}`;
}

/** The rate the echo actually applied, spelled out under the echo line so the
 * arithmetic is checkable rather than merely asserted: "1 SACK = 50kg". The
 * rate is the edge's `pack_size_micro_applied` — the yield-adjusted figure
 * that will be SNAPSHOTTED on the row, not the supplier's current pack size,
 * which may be edited later. */
export function entryIntentRate(echo: GrnEntryIntentEcho): string {
  const rate = formatMicroQuantity(echo.pack_size_micro_applied, echo.item_dimension);
  return `1 ${echo.entered_purchase_unit} = ${rate}`;
}

/** `true` when the operator's declared dimension and the item's own dimension
 * disagree — the condition the edge records as `DIMENSION_MISMATCH`.
 *
 * Used ONLY to decide whether to draw attention to the disagreement. It never
 * corrects the declaration, and no code path anywhere in this app writes
 * `item_dimension` into `quantity_dimension`. */
export function echoHasDimensionDisagreement(echo: GrnEntryIntentEcho): boolean {
  return echo.quantity_dimension !== echo.item_dimension;
}

// ------------------------------------------------------------- gap prose --

/** One gap reason, rendered for a human.
 *
 * `title` is the reason ITSELF, never the screen's name. The M4 gaps screen
 * titles every row "Items Sold With No Recipe" regardless of reason, so a
 * `DIMENSION_MISMATCH` reads there as a missing recipe — a filed M6 defect.
 * M5 adds eight reasons; each one gets its own words here.
 *
 * `nextStep` exists because §64 binds every message: a gap a buyer cannot act
 * on is a notification, not a signal. */
export interface GrnGapReasonCopy {
  title: string;
  nextStep: string;
}

const GAP_REASON_COPY: Record<GrnGapReason, GrnGapReasonCopy> = {
  NO_PURCHASE_ORDER: {
    title: "Received with no purchase order",
    nextStep:
      "Expected for a walk-in delivery, a standing order or an emergency purchase. If this should have been ordered, raise the purchase order in the admin so the spend is on record.",
  },
  PURCHASE_ORDER_NOT_FOUND: {
    title: "Purchase order not known at this till",
    nextStep:
      "The order was referenced but has never reached this outlet. The goods are recorded either way. Check the purchase order exists and has been sent; it will match up once it syncs.",
  },
  PO_LINE_NOT_FOUND: {
    title: "Item is not on the purchase order",
    nextStep:
      "The supplier delivered something the order does not list, or the order was amended after dispatch. Confirm with the buyer whether it was intended.",
  },
  QUANTITY_EXCEEDS_ORDERED: {
    title: "More delivered than was ordered",
    nextStep:
      "The over-delivery is accepted and recorded. Check the delivery note against the order, and raise a purchase return if the excess is going back.",
  },
  NO_SUPPLIER_ITEM: {
    title: "No agreed pack size for this supplier and unit",
    nextStep:
      "The quantity was converted from the unit label instead of the supplier's agreed pack size. Add the supplier's item and pack size in the admin so the next receipt converts exactly.",
  },
  NO_UNIT_CONVERSION: {
    title: "Purchase unit could not be converted",
    nextStep:
      "The typed quantity was recorded UNCONVERTED against stock, which is very likely wrong. Add a pack size or unit conversion for this item, then correct the stock with a count.",
  },
  DIMENSION_MISMATCH: {
    title: "Declared unit disagrees with the item",
    nextStep:
      "The unit entered on the receipt is not the kind of unit this item is measured in — for example a weight against an item counted in pieces. Check the delivery note and correct the item's setup or the receipt.",
  },
  SUPPLIER_NOT_FOUND: {
    title: "Supplier not known at this till",
    nextStep:
      "The delivery is recorded against no supplier on file. Add the supplier in the admin so the receipt can be matched to their invoice.",
  },
};

/** THE ACTUAL REASON, in words — never the screen's title. Falls back to the
 * raw code for a reason this build has not heard of (a contract drift), which
 * is still more informative than a generic heading. */
export function grnGapReasonCopy(reason: string): GrnGapReasonCopy {
  const known = GAP_REASON_COPY[reason as GrnGapReason];
  return (
    known ?? {
      title: reason,
      nextStep: "This till does not recognise this gap reason. Report it with the receipt number.",
    }
  );
}

/** The prose the edge wrote about this specific gap, which is what a person
 * actually reads (`grn_gap.detail` is prose for that reason, ADR-019 §1).
 * Falls back to the reason's own next-step copy when the edge recorded no
 * detail, so a row is never blank. */
export function grnGapDetailText(gap: GrnGap): string {
  return gap.detail ?? grnGapReasonCopy(gap.reason).nextStep;
}

// ------------------------------------------------------------ permissions --

/** `procurement.manage` — receive goods, record a purchase return
 * (`packages/contracts` identity.ts `PermissionSchema`, 0.6.0).
 *
 * `procurement.approve` is deliberately NOT consulted anywhere in this app:
 * the edge never approves a purchase order and must never be able to
 * (ADR-019 §7 — there is no `role` table in SQLite and no
 * `po_approval_limit_paise` anywhere in it). Approval is an admin-UI gate. */
export function canManageProcurement(principal: AuthenticatedPrincipal | null): boolean {
  return hasPermission(principal, "procurement.manage");
}

// ----------------------------------------------------------- error display --
// §64 is binding: every message must tell the operator whether intervention
// is necessary and what it is, never "Something went wrong".
//
// NOTE WHAT IS ABSENT FROM THIS LIST: there is no message for a missing
// purchase order, an unknown supplier or an unconvertible unit, because none
// of those is an error. Each is an accepted receipt with a gap attached
// (ADR-019 §1), and turning one into a message here would be the first step
// back toward refusing the delivery.

export function procurementErrorMessage(err: unknown): string {
  if (!isTauriCommandError(err)) {
    return "Could not record this. The delivery has NOT been saved — try again before the driver leaves.";
  }
  switch (err.code) {
    case "INVALID_RECEIPT_QUANTITY":
      // The edge/Tauri message already names the offending text and what a
      // valid quantity looks like. Shown verbatim per §64.
      return err.message;
    case "INVALID_INPUT":
      return err.message;
    case "NOT_FOUND":
      return err.message;
    default:
      return "Could not record this. The delivery has NOT been saved — try again before the driver leaves.";
  }
}
