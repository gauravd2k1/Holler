// Billing display/action rules (docs/spec/payments.md, ADR-016). Business
// logic lives here, not in JSX (CLAUDE.md §Coding rules) — every function is
// pure and money-safe: nothing here ever does float arithmetic on paise, and
// nothing here computes a tax or tender amount the edge did not already
// return (CLAUDE.md: the edge computes, the UI formats).

import type {
  AuthenticatedPrincipal,
  CanonicalOrder,
  CashShift,
  DiscountDefinition,
  Invoice,
  Payment,
} from "@holler/contracts";
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

// -------------------------------------------------------------- split bills --
// ADR-016 §4: a split bill is N invoices sharing one `split_group_id`, each
// independently numbered and independently payable — never a split entity.
// `Db::issue_split_invoices_with_outbox` is the SOLE authority on whether a
// split reconstructs the order's lines exactly (§66); everything below is a
// UI-only preview/builder, never the gate (task requirement).

/** Every payment allocated against ONE invoice — a single tender can, in
 * general, settle more than one invoice (`PaymentAllocationSchema`'s own
 * comment), so filtering by `allocations` rather than assuming a 1:1 payment-
 * to-invoice relationship is what makes "which parts remain unpaid" correct
 * once a split exists. `usePaymentsForOrderQuery` already returns every
 * payment for the whole order (across every split part); this narrows it to
 * the ones relevant to `invoiceId` before `amountDuePaise` is applied. */
export function paymentsForInvoice(
  payments: readonly Payment[],
  invoiceId: string,
): readonly Payment[] {
  return payments.filter((p) => p.allocations.some((a) => a.invoice_id === invoiceId));
}

/** One invoice-to-be, as staged in the split-bill builder before
 * `issueSplitInvoices` is called — order_item_id -> quantity this part
 * bills. UI state only; an entry of `0` or an absent key both mean "this
 * part bills none of this line" and are equivalent (`splitPartToRequest`
 * drops non-positive entries so the wire shape never carries a zero-quantity
 * share the edge would reject anyway). */
export interface SplitPartDraft {
  quantities: Record<string, number>;
}

/** Turns one staged [`SplitPartDraft`] into the wire shape
 * `issueSplitInvoices` needs. Never called the authority: the edge alone
 * decides whether the resulting parts reconstruct the order (§66). */
export function splitPartToRequest(draft: SplitPartDraft): {
  lines: { orderItemId: string; quantity: number }[];
} {
  return {
    lines: Object.entries(draft.quantities)
      .filter(([, quantity]) => quantity > 0)
      .map(([orderItemId, quantity]) => ({ orderItemId, quantity })),
  };
}

/** How much of `orderItemId`'s quantity has been assigned across every
 * staged part so far — a PREVIEW only, shown next to the order line's own
 * quantity so a cashier can see at a glance whether a split looks right
 * before submitting. The edge's own §66 conservation check is what actually
 * accepts or rejects the split; this number is never used to block
 * submission, only to inform it. */
export function totalQuantityAssignedPreview(
  orderItemId: string,
  parts: readonly SplitPartDraft[],
): number {
  return parts.reduce((sum, p) => sum + (p.quantities[orderItemId] ?? 0), 0);
}

/** `true` once every part carries at least one positive-quantity line —
 * catches the empty-part shape the edge rejects as `EMPTY_SPLIT_PART` before
 * a doomed submission is attempted, without attempting the §66 conservation
 * check itself (never the gate on THAT). */
export function everySplitPartHasALine(parts: readonly SplitPartDraft[]): boolean {
  return (
    parts.length > 0 &&
    parts.every((p) => Object.values(p.quantities).some((q) => q > 0))
  );
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

// -------------------------------------------------------------- discounts --
// ADR-016 §28, docs/spec/compliance.md. The edge (`apps/pos/src-tauri/src/
// domain/discount.rs`) is AUTHORITATIVE for every one of these gates —
// `requires_reason`/`required_permission` are re-checked there and the
// invoice is rejected outright if either fails, regardless of what this
// module decides. Everything below exists only so the cashier is not
// invited to attempt something the edge will refuse, and so the discount
// preview shown before "Issue Bill" is pressed matches what the edge will
// actually compute — it is never trusted as the final figure.

const BPS_DENOMINATOR = 10000;

/** Only a `LINE`-scope, currently-active, currently-effective discount can
 * be offered at all — `BILL` scope is unimplemented (this track's disclosed
 * limitation) and an inactive/not-yet-effective row is config a manager has
 * queued up but not turned on. `nowIso` is injected (never read from a bare
 * `Date`/`new Date()` here) so this stays a pure, testable function. */
export function isDiscountOfferable(def: DiscountDefinition, nowIso: string): boolean {
  if (def.scope !== "LINE") return false;
  if (!def.is_active) return false;
  if (def.effective_from > nowIso) return false;
  if (def.effective_to !== null && def.effective_to < nowIso) return false;
  return true;
}

export function discountRequiresReason(def: DiscountDefinition): boolean {
  return def.requires_reason;
}

/** `true` only when `def` names no permission, or the principal already
 * carries it. A `required_permission` naming a value outside the frozen
 * `Permission` enum (the contract types it as a bare string, not the enum,
 * precisely so a discount can name a permission that gap will eventually
 * close) still compares correctly here — this checks plain string
 * membership, not an enum cast. */
export function canApplyDiscount(
  principal: AuthenticatedPrincipal | null,
  def: DiscountDefinition,
): boolean {
  if (!principal) return false;
  if (def.required_permission === null) return true;
  const granted: readonly string[] = principal.permissions;
  return granted.includes(def.required_permission);
}

/** Integer basis-point/paise preview of what applying `def` to one line
 * (`unitPricePaise`) will produce — mirrors the rounding policy of
 * `apps/pos/src-tauri/src/domain/discount.rs::resolve_line_discount_per_unit_paise`
 * (half-up, capped by `max_discount_paise` for PERCENT) using only integer
 * arithmetic, never a float division of basis points. This is a PREVIEW for
 * the billing screen only — CLAUDE.md "the edge computes, the UI formats"
 * still holds: `issueInvoice` never sends this number, only the definition
 * id and reason, and the edge recomputes it independently. */
export function previewLineDiscountPerUnitPaise(
  def: DiscountDefinition,
  unitPricePaise: number,
): number {
  if (def.method === "PERCENT") {
    const bps = def.value_bps ?? 0;
    const raw = unitPricePaise * bps;
    const computed = Math.trunc((raw + BPS_DENOMINATOR / 2) / BPS_DENOMINATOR);
    return def.max_discount_paise !== null ? Math.min(computed, def.max_discount_paise) : computed;
  }
  return def.value_paise ?? 0;
}

/** One line's discount as the cashier has staged it in the billing UI,
 * before `issueInvoice` is called — UI state, not a wire shape. */
export interface StagedLineDiscount {
  orderItemId: string;
  discountDefinitionId: string;
  reason: string;
}

/** `true` once every staged discount satisfies its own definition's
 * `requires_reason` gate — used to enable/disable the "Issue Bill" control
 * so a cashier is not invited to submit a request the edge will reject for
 * a missing reason. Does NOT check `required_permission` (that is already
 * enforced by only offering the definition via `canApplyDiscount` in the
 * first place, so a staged entry can only exist for a definition the
 * cashier is already permitted to use). */
export function stagedDiscountsAreComplete(
  staged: readonly StagedLineDiscount[],
  definitionsById: ReadonlyMap<string, DiscountDefinition>,
): boolean {
  return staged.every((s) => {
    const def = definitionsById.get(s.discountDefinitionId);
    if (!def) return false;
    return !def.requires_reason || s.reason.trim() !== "";
  });
}

/** The order line a staged discount applies to, purely for showing its name/
 * quantity/unit price on the billing screen next to the discount picker. */
export function orderItemById(
  order: CanonicalOrder | null | undefined,
  orderItemId: string,
): CanonicalOrder["items"][number] | undefined {
  return order?.items.find((i) => i.id === orderItemId);
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
    // ADR-016 §28 discount gates (apps/pos/src-tauri/src/domain/discount.rs)
    // — every one of these is the edge's OWN enforcement of a governance
    // field the UI already tried to honour; if one of these is ever seen it
    // means the staged discount reached the edge despite that, so the
    // edge's own specific message is shown verbatim rather than re-worded.
    case "DISCOUNT_REASON_REQUIRED":
    case "DISCOUNT_PERMISSION_DENIED":
    case "DISCOUNT_NOT_ACTIVE":
    case "DISCOUNT_DEFINITION_NOT_FOUND":
      return err.message;
    case "DISCOUNT_SCOPE_NOT_SUPPORTED":
      return "Bill-level discounts are not available in this version — apply a per-item discount instead.";
    case "DISCOUNT_MISCONFIGURED":
      return `${err.message} — ask a manager to fix this discount's configuration.`;
    case "INVALID_INPUT":
      // Covers two distinct guards that both surface as `INVALID_INPUT`:
      // the edge tax engine's own validity check on a resolved discount
      // (non-negative, not exceeding the line's unit price), and
      // `Db::issue_split_invoices_with_outbox`'s §66 conservation check on a
      // split that does not reconstruct the order's lines exactly
      // (over/under-billed). Both messages already name the specific
      // order_item and mismatch — shown verbatim per §64.
      return err.message;
    case "SPLIT_REQUIRES_AT_LEAST_TWO_PARTS":
      return "A split bill needs at least two parts — issue a normal bill instead.";
    case "EMPTY_SPLIT_PART":
      return "Every part of a split bill must bill at least one item.";
    case "ORDER_ITEM_NOT_FOUND":
      return err.message;
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
