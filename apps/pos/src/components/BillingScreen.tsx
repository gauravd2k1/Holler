import { useEffect, useState } from "react";
import { useParams, useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import type { PaymentMethod } from "@holler/contracts";
import {
  useCashShiftQuery,
  useDiscountDefinitionsQuery,
  useInvoicesForOrderQuery,
  useMenuItemsQuery,
  useOrderQuery,
  usePaymentsForOrderQuery,
  queryKeys,
} from "../lib/queries";
import { formatPaiseAsRupees, parseRupeesToPaise } from "../domain/money";
import {
  amountDuePaise,
  billingErrorMessage,
  canApplyDiscount,
  canOfferBilling,
  canOfferReversal,
  discountRequiresReason,
  everySplitPartHasALine,
  isDiscountOfferable,
  isFullySettled,
  isVarianceReasonRequired,
  paymentsForInvoice,
  pendingTenderTotalPaise,
  previewLineDiscountPerUnitPaise,
  projectedVariancePaise,
  remainingAfterPendingPaise,
  splitPartToRequest,
  stagedDiscountsAreComplete,
  totalQuantityAssignedPreview,
  type PendingTenderEntry,
  type SplitPartDraft,
  type StagedLineDiscount,
} from "../domain/billing";
import {
  closeCashShift,
  issueInvoice,
  issueSplitInvoices,
  openCashShift,
  recordPayment,
  type LineDiscountRequest,
} from "../lib/tauri";
import { useAuthStore } from "../store/auth";
import { useCashShiftStore } from "../store/cashShift";

const PAYMENT_METHODS: PaymentMethod[] = [
  "CASH",
  "UPI",
  "CREDIT_CARD",
  "DEBIT_CARD",
  "WALLET",
  "GIFT_CARD",
  "BANK_TRANSFER",
];

interface PendingTender extends PendingTenderEntry {
  method: PaymentMethod;
  tenderedRupees: string;
}

/** The cashier-facing billing surface (T9): view the computed bill, take one
 * or more tenders against it (split payment across methods), void/refund a
 * tender, and open/close the cash shift. Every money value shown is copied
 * from what the edge returned — this component computes nothing except
 * summing already-known integers for display (`domain/billing.ts`). */
export function BillingScreen() {
  const { orderId } = useParams({ from: "/orders/$orderId/billing" });
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const principal = useAuthStore((s) => s.principal);
  const openShiftId = useCashShiftStore((s) => s.openShiftId);
  const setOpenShiftId = useCashShiftStore((s) => s.setOpenShiftId);
  const recoverOpenShift = useCashShiftStore((s) => s.recoverOpenShift);

  const orderQuery = useOrderQuery(orderId);
  const invoicesQuery = useInvoicesForOrderQuery(orderId);
  const paymentsQuery = usePaymentsForOrderQuery(orderId);
  const shiftQuery = useCashShiftQuery(openShiftId);
  const menuItemsQuery = useMenuItemsQuery();
  const discountDefinitionsQuery = useDiscountDefinitionsQuery();

  const [issueError, setIssueError] = useState<string | null>(null);
  const [issuing, setIssuing] = useState(false);

  // Discounts staged before the bill is issued — LINE scope only (BILL scope
  // is not implemented in this build, ADR-016 §28). Keyed by order_item_id:
  // one discount per line at most. Applies once across the whole order,
  // whether it is billed whole or split — a discount is a property of the
  // line, not of which split part happens to carry it (ADR-016 §4).
  const [stagedDiscounts, setStagedDiscounts] = useState<Record<string, StagedLineDiscount>>({});

  // Split-bill builder (ADR-016 §4): OFF (a plain "Issue Bill") unless the
  // cashier opts in. `splitParts` is UI state only — the edge alone decides
  // whether it reconstructs the order's lines exactly (§66); this never
  // gates submission, only previews it (task requirement).
  const [splitMode, setSplitMode] = useState(false);
  const [splitParts, setSplitParts] = useState<SplitPartDraft[]>([
    { quantities: {} },
    { quantities: {} },
  ]);
  const [splitError, setSplitError] = useState<string | null>(null);
  const [splitting, setSplitting] = useState(false);

  // Pending (entered but not yet submitted) tenders, keyed by invoice id —
  // a split group has more than one invoice open for payment at once, and
  // each is independently payable (ADR-016 §4): settling one must never
  // touch another's remaining due.
  const [pendingTendersByInvoice, setPendingTendersByInvoice] = useState<
    Record<string, PendingTender[]>
  >({});
  const [tenderErrorByInvoice, setTenderErrorByInvoice] = useState<Record<string, string | null>>(
    {},
  );
  const [submittingTenderInvoiceId, setSubmittingTenderInvoiceId] = useState<string | null>(null);

  const [openingCashRupees, setOpeningCashRupees] = useState("");
  const [shiftError, setShiftError] = useState<string | null>(null);
  const [actualCashRupees, setActualCashRupees] = useState("");
  const [varianceReason, setVarianceReason] = useState("");
  const [closingShift, setClosingShift] = useState(false);
  // Set once the edge has actually rejected a close for a missing variance
  // reason (§39) — distinct from the merely-projected estimate below, which
  // is a client-side guess shown before submission. This flips true only on
  // a real `CASH_VARIANCE_REASON_REQUIRED` response, so the reason field is
  // never hidden again once the edge has said it is mandatory.
  const [varianceReasonRequired, setVarianceReasonRequired] = useState(false);

  // T9 retry, Defect 2: recover a shift orphaned by a POS restart
  // automatically, the moment a cashier is known — no manual id entry.
  // `recoverOpenShift` is a no-op once `openShiftId` is already set (e.g.
  // this session already opened one), so this is safe to run on every mount.
  useEffect(() => {
    if (!principal) return;
    recoverOpenShift(principal.user_id).catch((err: unknown) => {
      setShiftError(billingErrorMessage(err));
    });
  }, [principal, recoverOpenShift]);

  const invoices = invoicesQuery.data ?? [];
  const hasInvoices = invoices.length > 0;
  const payments = paymentsQuery.data ?? [];
  const canBill = canOfferBilling(principal);
  const canVoid = canOfferReversal(principal);
  const menuItems = menuItemsQuery.data ?? [];
  const nowIso = new Date().toISOString();
  const offerableDiscounts = (discountDefinitionsQuery.data ?? []).filter((d) =>
    isDiscountOfferable(d, nowIso),
  );
  const discountsById = new Map(offerableDiscounts.map((d) => [d.id, d] as const));
  const orderItems = orderQuery.data?.items ?? [];
  const stagedList = Object.values(stagedDiscounts);
  // Disabled, not merely visually gated, per §28: the "Issue Bill" control
  // itself stays off until every staged discount either needs no reason or
  // already has one — the edge's own `DISCOUNT_REASON_REQUIRED` rejection is
  // the authority, this only avoids inviting a doomed submission.
  const discountsReady = stagedDiscountsAreComplete(stagedList, discountsById);

  function menuItemName(menuItemId: string): string {
    return menuItems.find((m) => m.id === menuItemId)?.name ?? menuItemId;
  }

  function setLineDiscount(orderItemId: string, discountDefinitionId: string) {
    if (discountDefinitionId === "") {
      setStagedDiscounts((prev) => {
        const next = { ...prev };
        delete next[orderItemId];
        return next;
      });
      return;
    }
    setStagedDiscounts((prev) => ({
      ...prev,
      [orderItemId]: { orderItemId, discountDefinitionId, reason: prev[orderItemId]?.reason ?? "" },
    }));
  }

  function setLineDiscountReason(orderItemId: string, reason: string) {
    setStagedDiscounts((prev) => {
      const existing = prev[orderItemId];
      if (!existing) return prev;
      return { ...prev, [orderItemId]: { ...existing, reason } };
    });
  }

  async function handleIssueInvoice() {
    if (!orderId || !principal) return;
    setIssuing(true);
    setIssueError(null);
    try {
      const discounts: LineDiscountRequest[] = stagedList.map((s) => ({
        orderItemId: s.orderItemId,
        discountDefinitionId: s.discountDefinitionId,
        reason: s.reason.trim() === "" ? null : s.reason.trim(),
      }));
      await issueInvoice(orderId, principal.user_id, discounts);
      setStagedDiscounts({});
      await queryClient.invalidateQueries({ queryKey: queryKeys.invoices(orderId) });
    } catch (err) {
      setIssueError(billingErrorMessage(err));
    } finally {
      setIssuing(false);
    }
  }

  // -------------------------------------------------------- split builder --
  // ADR-016 §4: a split bill is N invoices sharing one `split_group_id`.
  // `splitParts` is a plain builder — add/remove a part, assign a quantity
  // of each order line to each part. The edge alone decides whether the
  // result reconstructs the order exactly (§66); this screen never gates
  // submission on the local preview totals, only shows them.

  function addSplitPart() {
    setSplitParts((prev) => [...prev, { quantities: {} }]);
  }

  function removeSplitPart(index: number) {
    setSplitParts((prev) => prev.filter((_, i) => i !== index));
  }

  function setSplitQuantity(partIndex: number, orderItemId: string, quantity: number) {
    setSplitParts((prev) =>
      prev.map((p, i) =>
        i === partIndex ? { quantities: { ...p.quantities, [orderItemId]: quantity } } : p,
      ),
    );
  }

  async function handleIssueSplitInvoices() {
    if (!orderId || !principal) return;
    setSplitting(true);
    setSplitError(null);
    try {
      const discounts: LineDiscountRequest[] = stagedList.map((s) => ({
        orderItemId: s.orderItemId,
        discountDefinitionId: s.discountDefinitionId,
        reason: s.reason.trim() === "" ? null : s.reason.trim(),
      }));
      await issueSplitInvoices(
        orderId,
        principal.user_id,
        splitParts.map(splitPartToRequest),
        discounts,
      );
      setStagedDiscounts({});
      setSplitParts([{ quantities: {} }, { quantities: {} }]);
      setSplitMode(false);
      await queryClient.invalidateQueries({ queryKey: queryKeys.invoices(orderId) });
    } catch (err) {
      // §64: a rejected split (over- or under-billed) must say what is
      // wrong — the edge's own §66 conservation message already names the
      // offending order_item and the mismatch (`billingErrorMessage`'s
      // `INVALID_INPUT` case), shown verbatim here.
      setSplitError(billingErrorMessage(err));
    } finally {
      setSplitting(false);
    }
  }

  function addPendingTender(invoiceId: string) {
    setPendingTendersByInvoice((prev) => ({
      ...prev,
      [invoiceId]: [...(prev[invoiceId] ?? []), { method: "CASH", amountPaise: 0, tenderedRupees: "" }],
    }));
  }

  function updatePendingTender(invoiceId: string, index: number, patch: Partial<PendingTender>) {
    setPendingTendersByInvoice((prev) => ({
      ...prev,
      [invoiceId]: (prev[invoiceId] ?? []).map((t, i) => (i === index ? { ...t, ...patch } : t)),
    }));
  }

  function removePendingTender(invoiceId: string, index: number) {
    setPendingTendersByInvoice((prev) => ({
      ...prev,
      [invoiceId]: (prev[invoiceId] ?? []).filter((_, i) => i !== index),
    }));
  }

  async function handleSubmitTenders(invoiceId: string) {
    if (!orderId || !principal) return;
    const tenders = pendingTendersByInvoice[invoiceId] ?? [];
    setTenderErrorByInvoice((prev) => ({ ...prev, [invoiceId]: null }));
    for (const t of tenders) {
      if (t.amountPaise <= 0) {
        setTenderErrorByInvoice((prev) => ({
          ...prev,
          [invoiceId]: "Every tender line needs an amount greater than zero.",
        }));
        return;
      }
    }
    setSubmittingTenderInvoiceId(invoiceId);
    try {
      for (const t of tenders) {
        const isCash = t.method === "CASH";
        const tenderedPaise = isCash ? (parseRupeesToPaise(t.tenderedRupees) ?? t.amountPaise) : null;
        const changePaise = isCash ? Math.max(0, tenderedPaise! - t.amountPaise) : null;
        await recordPayment({
          orderId,
          method: t.method,
          amountPaise: t.amountPaise,
          tenderedPaise,
          changePaise,
          reference: null,
          cashShiftId: isCash ? openShiftId : null,
          reversesPaymentId: null,
          // T9 retry, Defect 1, and ADR-016 §4: naming the invoice is what
          // lets the edge reject a tender that would exceed THIS part's own
          // remaining due, without touching any other part of a split.
          invoiceId,
          createdByUserId: principal.user_id,
        });
      }
      setPendingTendersByInvoice((prev) => ({ ...prev, [invoiceId]: [] }));
      await queryClient.invalidateQueries({ queryKey: queryKeys.payments(orderId) });
      if (openShiftId) {
        await queryClient.invalidateQueries({ queryKey: queryKeys.cashShift(openShiftId) });
      }
    } catch (err) {
      setTenderErrorByInvoice((prev) => ({ ...prev, [invoiceId]: billingErrorMessage(err) }));
    } finally {
      setSubmittingTenderInvoiceId(null);
    }
  }

  async function handleVoid(invoiceId: string, paymentId: string, amountPaise: number) {
    if (!orderId || !principal) return;
    setTenderErrorByInvoice((prev) => ({ ...prev, [invoiceId]: null }));
    try {
      await recordPayment({
        orderId,
        method: payments.find((p) => p.id === paymentId)?.method ?? "CASH",
        amountPaise: -amountPaise,
        tenderedPaise: null,
        changePaise: null,
        reference: null,
        cashShiftId: openShiftId,
        reversesPaymentId: paymentId,
        // A reversal always derives its own allocation target from the
        // original payment (T9 retry) — this is ignored at the edge.
        invoiceId: null,
        createdByUserId: principal.user_id,
      });
      await queryClient.invalidateQueries({ queryKey: queryKeys.payments(orderId) });
    } catch (err) {
      setTenderErrorByInvoice((prev) => ({ ...prev, [invoiceId]: billingErrorMessage(err) }));
    }
  }

  async function handleOpenShift() {
    if (!principal) return;
    setShiftError(null);
    const openingPaise = parseRupeesToPaise(openingCashRupees);
    if (openingPaise === null || openingPaise < 0) {
      setShiftError("Enter a valid opening float amount (e.g. 2000.00).");
      return;
    }
    try {
      const shift = await openCashShift(principal.user_id, openingPaise);
      setOpenShiftId(shift.id);
      setOpeningCashRupees("");
    } catch (err) {
      setShiftError(billingErrorMessage(err));
    }
  }

  async function handleCloseShift() {
    if (!openShiftId) return;
    setShiftError(null);
    const actualPaise = parseRupeesToPaise(actualCashRupees);
    if (actualPaise === null || actualPaise < 0) {
      setShiftError("Enter the counted cash amount (e.g. 3050.00).");
      return;
    }
    setClosingShift(true);
    try {
      await closeCashShift(openShiftId, actualPaise, varianceReason.trim() || null);
      setOpenShiftId(null);
      setActualCashRupees("");
      setVarianceReason("");
      setVarianceReasonRequired(false);
    } catch (err) {
      // §39, binding: a non-zero-variance close without a reason must not be
      // a dead end — the form stays open with the reason field visible and
      // the edge's own message (naming the exact variance) shown, so the
      // cashier can supply one and retry the SAME close.
      setShiftError(billingErrorMessage(err));
      if (isVarianceReasonRequired(err)) {
        setVarianceReasonRequired(true);
      }
    } finally {
      setClosingShift(false);
    }
  }

  const projectedVariance =
    shiftQuery.data && actualCashRupees.trim() !== ""
      ? projectedVariancePaise(shiftQuery.data, parseRupeesToPaise(actualCashRupees) ?? 0)
      : null;

  return (
    <main className="billing-screen">
      <header>
        <h1>Billing — {orderQuery.data?.display_number ?? orderId}</h1>
        <button type="button" onClick={() => void navigate({ to: "/orders" })}>
          Back to Orders
        </button>
      </header>

      <section className="cash-shift-panel">
        <h2>Cash Shift</h2>
        {!openShiftId && (
          <div>
            <input
              placeholder="Opening float ₹ (e.g. 2000.00)"
              inputMode="decimal"
              value={openingCashRupees}
              onChange={(e) => setOpeningCashRupees(e.target.value)}
            />
            <button type="button" disabled={!canBill} onClick={() => void handleOpenShift()}>
              Open Shift
            </button>
          </div>
        )}
        {openShiftId && shiftQuery.data && (
          <div>
            <p>
              Shift {shiftQuery.data.id.slice(0, 8)} — opened {shiftQuery.data.opened_at} — status{" "}
              {shiftQuery.data.status}
            </p>
            {shiftQuery.data.status === "OPEN" && (
              <div>
                <input
                  placeholder="Counted cash ₹ (e.g. 3050.00)"
                  inputMode="decimal"
                  value={actualCashRupees}
                  onChange={(e) => setActualCashRupees(e.target.value)}
                />
                {(varianceReasonRequired || (projectedVariance !== null && projectedVariance !== 0)) && (
                  <input
                    placeholder="Reason for variance (required)"
                    value={varianceReason}
                    onChange={(e) => setVarianceReason(e.target.value)}
                  />
                )}
                <button
                  type="button"
                  disabled={!canBill || closingShift}
                  onClick={() => void handleCloseShift()}
                >
                  {closingShift ? "Closing…" : "Close Shift"}
                </button>
              </div>
            )}
          </div>
        )}
        {shiftError && (
          <p className="billing-error" role="alert">
            {shiftError}
          </p>
        )}
      </section>

      {!hasInvoices && !invoicesQuery.isLoading && orderItems.length > 0 && (
        <section className="discount-panel">
          <h2>Discounts</h2>
          {/* LINE scope only — a discount naming BILL scope is not offered
              here at all (§28, this track's disclosed limitation). Applies
              once across the whole order, whether billed whole or split. */}
          {offerableDiscounts.length === 0 && <p>No discount is currently configured for this outlet.</p>}
          {offerableDiscounts.length > 0 &&
            orderItems.map((item) => {
              const staged = stagedDiscounts[item.id];
              const stagedDef = staged ? discountsById.get(staged.discountDefinitionId) : undefined;
              const applicableDiscounts = offerableDiscounts.filter((d) =>
                canApplyDiscount(principal, d),
              );
              return (
                <div key={item.id} className="discount-line-row">
                  <span>
                    {menuItemName(item.menu_item_id)} x{item.quantity} (
                    {formatPaiseAsRupees(item.unit_price_paise)}/ea)
                  </span>
                  <select
                    value={staged?.discountDefinitionId ?? ""}
                    onChange={(e) => setLineDiscount(item.id, e.target.value)}
                  >
                    <option value="">No discount</option>
                    {applicableDiscounts.map((d) => (
                      <option key={d.id} value={d.id}>
                        {d.name}
                      </option>
                    ))}
                  </select>
                  {stagedDef && (
                    <span>
                      -{formatPaiseAsRupees(previewLineDiscountPerUnitPaise(stagedDef, item.unit_price_paise))}
                      /ea (preview — the edge recomputes this on issue)
                    </span>
                  )}
                  {stagedDef && discountRequiresReason(stagedDef) && (
                    <input
                      placeholder="Reason for discount (required)"
                      value={staged?.reason ?? ""}
                      onChange={(e) => setLineDiscountReason(item.id, e.target.value)}
                    />
                  )}
                </div>
              );
            })}
        </section>
      )}

      <section className="invoice-panel">
        <h2>Bill</h2>
        {invoicesQuery.isLoading && <p>Loading bill…</p>}
        {!hasInvoices && !invoicesQuery.isLoading && (
          <div>
            {!splitMode && (
              <div>
                <button
                  type="button"
                  disabled={!canBill || issuing || !discountsReady}
                  onClick={() => void handleIssueInvoice()}
                >
                  {issuing ? "Issuing…" : "Issue Bill"}
                </button>
                <button
                  type="button"
                  disabled={!canBill || orderItems.length === 0}
                  onClick={() => setSplitMode(true)}
                >
                  Split Bill
                </button>
              </div>
            )}
            {splitMode && (
              <div className="split-bill-builder">
                <h3>Split Bill — {splitParts.length} parts</h3>
                {/* ADR-016 §4/§66: the edge is the sole authority on whether
                    this reconstructs the order's lines exactly — the totals
                    below are a PREVIEW only, never the gate on submission. */}
                <table>
                  <thead>
                    <tr>
                      <th>Item</th>
                      <th>Order Qty</th>
                      {splitParts.map((_, i) => (
                        <th key={i}>
                          Part {i + 1}{" "}
                          {splitParts.length > 2 && (
                            <button type="button" onClick={() => removeSplitPart(i)}>
                              Remove
                            </button>
                          )}
                        </th>
                      ))}
                      <th>Assigned</th>
                    </tr>
                  </thead>
                  <tbody>
                    {orderItems.map((item) => {
                      const assigned = totalQuantityAssignedPreview(item.id, splitParts);
                      return (
                        <tr key={item.id}>
                          <td>{menuItemName(item.menu_item_id)}</td>
                          <td>{item.quantity}</td>
                          {splitParts.map((part, i) => (
                            <td key={i}>
                              <input
                                type="number"
                                min={0}
                                inputMode="numeric"
                                value={part.quantities[item.id] ?? 0}
                                onChange={(e) =>
                                  setSplitQuantity(i, item.id, Number(e.target.value) || 0)
                                }
                              />
                            </td>
                          ))}
                          <td className={assigned === item.quantity ? undefined : "split-mismatch"}>
                            {assigned} / {item.quantity}
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
                <button type="button" onClick={addSplitPart}>
                  + Add Part
                </button>
                <button
                  type="button"
                  disabled={
                    !canBill ||
                    splitting ||
                    splitParts.length < 2 ||
                    !everySplitPartHasALine(splitParts) ||
                    !discountsReady
                  }
                  onClick={() => void handleIssueSplitInvoices()}
                >
                  {splitting ? "Issuing…" : `Issue ${splitParts.length}-Way Split`}
                </button>
                <button type="button" onClick={() => setSplitMode(false)}>
                  Cancel Split
                </button>
                {splitError && (
                  <p className="billing-error" role="alert">
                    {splitError}
                  </p>
                )}
              </div>
            )}
          </div>
        )}
        {!discountsReady && (
          <p>Every discount that requires a reason needs one entered before the bill can be issued.</p>
        )}
        {issueError && (
          <p className="billing-error" role="alert">
            {issueError}
          </p>
        )}
      </section>

      {hasInvoices &&
        invoices.map((inv) => {
          const invoicePayments = paymentsForInvoice(payments, inv.id);
          const due = amountDuePaise(inv, invoicePayments);
          const pendingTenders = pendingTendersByInvoice[inv.id] ?? [];
          const enteredTotal = pendingTenderTotalPaise(pendingTenders);
          const remaining = remainingAfterPendingPaise(inv, invoicePayments, pendingTenders);
          // T9 retry, Defect 1: the UI stops OFFERING an over-settling
          // tender once THIS part is fully settled — settling one part of a
          // split never touches another's own remaining due (ADR-016 §4).
          const billFullySettled = isFullySettled(inv, invoicePayments);
          const tenderError = tenderErrorByInvoice[inv.id] ?? null;
          const submittingTender = submittingTenderInvoiceId === inv.id;

          return (
            <section key={inv.id} className="invoice-detail-panel">
              <h2>
                Invoice {inv.invoice_number} — {inv.status}
                {inv.split_group_id !== null &&
                  ` — split part ${inv.split_index} of ${inv.split_count}`}
              </h2>
              <table>
                <thead>
                  <tr>
                    <th>Item</th>
                    <th>Qty</th>
                    <th>Discount</th>
                    <th>Taxable</th>
                    <th>CGST</th>
                    <th>SGST</th>
                    <th>IGST</th>
                    <th>Total</th>
                  </tr>
                </thead>
                <tbody>
                  {inv.lines.map((line) => (
                    <tr key={line.id}>
                      <td>{line.description}</td>
                      <td>{line.quantity}</td>
                      <td>{formatPaiseAsRupees(line.discount_paise)}</td>
                      <td>{formatPaiseAsRupees(line.taxable_value_paise)}</td>
                      <td>{formatPaiseAsRupees(line.cgst_paise)}</td>
                      <td>{formatPaiseAsRupees(line.sgst_paise)}</td>
                      <td>{formatPaiseAsRupees(line.igst_paise)}</td>
                      <td>{formatPaiseAsRupees(line.total_paise)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <p>Subtotal: {formatPaiseAsRupees(inv.subtotal_paise)}</p>
              <p>Discount: {formatPaiseAsRupees(inv.discount_paise)}</p>
              <p>Round off: {formatPaiseAsRupees(inv.round_off_paise)}</p>
              <p className="grand-total">Grand Total: {formatPaiseAsRupees(inv.grand_total_paise)}</p>
              <p className="amount-due">
                Amount Due: {formatPaiseAsRupees(due)}
                {billFullySettled && " — settled"}
              </p>

              <h3>Payments</h3>
              <table>
                <thead>
                  <tr>
                    <th>Method</th>
                    <th>Amount</th>
                    <th>Status</th>
                    <th>Reversal of</th>
                    <th>Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {invoicePayments.map((p) => (
                    <tr key={p.id}>
                      <td>{p.method}</td>
                      <td>{formatPaiseAsRupees(p.amount_paise)}</td>
                      <td>{p.status}</td>
                      <td>{p.reverses_payment_id ?? "—"}</td>
                      <td>
                        {/* No "edit payment" affordance exists anywhere in
                            this screen — a correction is always a new
                            reversal row (task requirement,
                            docs/spec/payments.md). */}
                        {canVoid && p.reverses_payment_id === null && p.status === "CAPTURED" && (
                          <button
                            type="button"
                            onClick={() => void handleVoid(inv.id, p.id, p.amount_paise)}
                          >
                            Void / Refund
                          </button>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>

              <h4>Take Payment</h4>
              {pendingTenders.map((t, i) => (
                <div key={i} className="pending-tender-row">
                  <select
                    value={t.method}
                    onChange={(e) =>
                      updatePendingTender(inv.id, i, { method: e.target.value as PaymentMethod })
                    }
                  >
                    {PAYMENT_METHODS.map((m) => (
                      <option key={m} value={m}>
                        {m}
                      </option>
                    ))}
                  </select>
                  <input
                    placeholder="Amount ₹"
                    inputMode="decimal"
                    onChange={(e) =>
                      updatePendingTender(inv.id, i, {
                        amountPaise: parseRupeesToPaise(e.target.value) ?? 0,
                      })
                    }
                  />
                  {t.method === "CASH" && (
                    <input
                      placeholder="Tendered ₹ (optional)"
                      inputMode="decimal"
                      value={t.tenderedRupees}
                      onChange={(e) =>
                        updatePendingTender(inv.id, i, { tenderedRupees: e.target.value })
                      }
                    />
                  )}
                  <button type="button" onClick={() => removePendingTender(inv.id, i)}>
                    Remove
                  </button>
                </div>
              ))}
              <button
                type="button"
                disabled={!canBill || billFullySettled}
                onClick={() => addPendingTender(inv.id)}
              >
                + Add Tender
              </button>
              {billFullySettled && <p>This bill is fully settled — no further tender is needed.</p>}
              <p>Entered so far: {formatPaiseAsRupees(enteredTotal)}</p>
              <p>Remaining after entered tenders: {formatPaiseAsRupees(remaining)}</p>
              {tenderError && (
                <p className="billing-error" role="alert">
                  {tenderError}
                </p>
              )}
              <button
                type="button"
                disabled={!canBill || billFullySettled || pendingTenders.length === 0 || submittingTender}
                onClick={() => void handleSubmitTenders(inv.id)}
              >
                {submittingTender ? "Recording…" : "Record Payment(s)"}
              </button>
            </section>
          );
        })}
    </main>
  );
}
