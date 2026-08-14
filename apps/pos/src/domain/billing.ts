// Billing display/action rules (docs/spec/payments.md, ADR-016). Business
// logic lives here, not in JSX (CLAUDE.md §Coding rules) — every function is
// pure and money-safe: nothing here ever does float arithmetic on paise, and
// nothing here computes a tax or tender amount the edge did not already
// return (CLAUDE.md: the edge computes, the UI formats).

import type { AuthenticatedPrincipal, CashShift, Invoice, Payment } from "@holler/contracts";
import { sumPaise } from "./money";
import { hasPermission } from "./permissions";
import { isTauriCommandError } from "../lib/tauri";

/** Only a CAPTURED payment counts toward what has actually settled — a
 * reversal is itself a CAPTURED row with a non-positive `amount_paise`
 * (ADR-016 §payment.ts), so summing every CAPTURED row's `amount_paise`
 * nets a forward tender against its reversal automatically. PENDING/FAILED
 * tenders (Milestone 7 gateway states, unused in M3) never contributed
 * money and must not be counted. */
export function totalSettledPaise(payments: readonly Payment[]): number {
  return sumPaise(
    payments.filter((p) => p.status === "CAPTURED").map((p) => p.amount_paise),
  );
}

/** What remains to be collected against one invoice — may be negative if
 * the invoice has been overpaid (e.g. a cash tender larger than the total,
 * whose `change_paise` already accounts for the difference at the register,
 * not here). */
export function amountDuePaise(invoice: Invoice, payments: readonly Payment[]): number {
  return invoice.grand_total_paise - totalSettledPaise(payments);
}

/** One tender the cashier has entered in the split-payment UI but not yet
 * submitted — this is UI state, not a `Payment`, so it carries no id/status. */
export interface PendingTenderEntry {
  amountPaise: number;
}

/** The running total of every tender entered so far in a split-payment
 * screen, before any of it is submitted — §35's worked example (₹500 cash +
 * ₹1,000 UPI + ₹500 card = ₹2,000) is exactly this sum. */
export function pendingTenderTotalPaise(entries: readonly PendingTenderEntry[]): number {
  return sumPaise(entries.map((e) => e.amountPaise));
}

/** What is still owed after the tenders currently entered (but not yet
 * submitted) in a split-payment screen are applied — negative once the
 * cashier has entered enough to cover the bill (change is due). */
export function remainingAfterPendingPaise(
  invoice: Invoice,
  payments: readonly Payment[],
  pending: readonly PendingTenderEntry[],
): number {
  return amountDuePaise(invoice, payments) - pendingTenderTotalPaise(pending);
}

export function isFullySettled(invoice: Invoice, payments: readonly Payment[]): boolean {
  return amountDuePaise(invoice, payments) <= 0;
}

// ------------------------------------------------------------- permissions --
// No dedicated billing permission exists in `packages/contracts`'
// `PermissionSchema` (ADR-016 0.4.4 addendum records the same gap for the
// compliance config routes). The forward path (issue a bill, take a tender,
// open/close a shift) gates on `order.modify` — the same permission
// `confirmOrder`/`sendOrderToKitchen` already use for an order-state action.
// A reversal (void/refund a tender) gates on the stricter `order.void`,
// since it is reversing money already taken, not merely amending an order.

export function canOfferBilling(principal: AuthenticatedPrincipal | null): boolean {
  return hasPermission(principal, "order.modify");
}

export function canOfferReversal(principal: AuthenticatedPrincipal | null): boolean {
  return hasPermission(principal, "order.void");
}

// ----------------------------------------------------------- error display --
// §64 is binding: every one of these must tell a cashier whether
// intervention is necessary and what it is, never "Something went wrong".
// The edge (apps/pos/src-tauri/src/error.rs) already attaches a specific
// `code` and a message carrying the concrete figures (variance amount,
// remaining reversible amount, existing shift id) — this only chooses the
// cashier-facing wording per code, falling back to the edge's own message
// where it is already specific enough to show verbatim.

export function billingErrorMessage(err: unknown): string {
  if (!isTauriCommandError(err)) {
    return "Could not complete the billing action. Please try again.";
  }
  switch (err.code) {
    case "NOTHING_TO_BILL":
      return "This order has no lines to bill.";
    case "NO_FISCAL_PROFILE_CONFIGURED":
      return "This outlet has no billing profile configured yet — ask a manager to set up GST details before issuing a bill.";
    case "NO_ACTIVE_INVOICE_SERIES":
      return "This outlet has no active invoice numbering series — ask a manager to configure one before issuing a bill.";
    case "FORWARD_PAYMENT_AMOUNT_NOT_POSITIVE":
      return "Enter an amount greater than zero for this tender.";
    case "REVERSAL_AMOUNT_NOT_NON_POSITIVE":
      return "A void or refund cannot be entered as a positive amount.";
    case "REVERSED_PAYMENT_NOT_FOUND":
      return "The original payment for this refund could not be found.";
    case "PAYMENT_ALREADY_FULLY_REVERSED":
      return "This payment has already been fully refunded or voided.";
    case "REVERSAL_EXCEEDS_REMAINING":
      // The edge's own message already names the exact remaining amount
      // (error.rs `ReversalExceedsRemaining`) — show it verbatim rather than
      // a vaguer rewrite, per §64.
      return err.message;
    case "FORWARD_PAYMENT_EXCEEDS_REMAINING_DUE":
      // T9 retry, Defect 1 (double-settlement): the edge's own message
      // already names the invoice and the exact amount still outstanding —
      // show it verbatim rather than a vaguer rewrite, per §64. This is the
      // rejection a cashier sees if they somehow still attempt an
      // over-settling tender despite the UI gating "Add Tender" on
      // `isFullySettled`.
      return err.message;
    case "INVOICE_NOT_FOUND_FOR_PAYMENT":
      return "This bill could not be found — refresh and try again.";
    case "CASH_SHIFT_ALREADY_OPEN":
      return err.message;
    case "CASH_SHIFT_NOT_OPEN":
      return "This cash shift is not open — it may already be closed.";
    case "CASH_VARIANCE_REASON_REQUIRED":
      // The binding §39 case: the edge's message states the exact variance
      // (expected vs counted) — the UI must show it and collect a reason,
      // never present a dead end (task requirement).
      return err.message;
    case "CASH_MOVEMENT_REASON_REQUIRED":
      return "A reason is required for a paid-in or paid-out entry.";
    default:
      return "Could not complete the billing action. Please try again.";
  }
}

/** `true` only for the one case the UI must recover from by collecting a
 * reason and resubmitting the same close, rather than treating the close as
 * failed outright. */
export function isVarianceReasonRequired(err: unknown): boolean {
  return isTauriCommandError(err) && err.code === "CASH_VARIANCE_REASON_REQUIRED";
}

/** A cash shift's counted-vs-expected variance, purely for on-screen preview
 * before the cashier submits a close. `expected_cash_paise` is `null` while
 * a shift is OPEN (only the close itself fills it in) — so this projects
 * the same sum `close_cash_shift` will derive, from the shift's own
 * `movements`, using nothing but integer addition (`sumPaise`). This is
 * NEVER the authoritative figure: the edge independently re-derives and
 * enforces its own sum server-side (`close_cash_shift_impl`,
 * `edge/database/src/payment/cash_shift.rs`) and this projection is
 * discarded the moment the real response comes back. */
export function projectedVariancePaise(shift: CashShift, actualCashPaise: number): number {
  const expected = sumPaise(shift.movements.map((m) => m.amount_paise));
  return actualCashPaise - expected;
}
