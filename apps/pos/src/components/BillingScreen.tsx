import { useState } from "react";
import { useParams, useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import type { PaymentMethod } from "@holler/contracts";
import {
  useCashShiftQuery,
  useInvoicesForOrderQuery,
  useOrderQuery,
  usePaymentsForOrderQuery,
  queryKeys,
} from "../lib/queries";
import { formatPaiseAsRupees, parseRupeesToPaise } from "../domain/money";
import {
  amountDuePaise,
  billingErrorMessage,
  canOfferBilling,
  canOfferReversal,
  isVarianceReasonRequired,
  pendingTenderTotalPaise,
  projectedVariancePaise,
  remainingAfterPendingPaise,
  type PendingTenderEntry,
} from "../domain/billing";
import {
  closeCashShift,
  issueInvoice,
  openCashShift,
  recordPayment,
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

  const orderQuery = useOrderQuery(orderId);
  const invoicesQuery = useInvoicesForOrderQuery(orderId);
  const paymentsQuery = usePaymentsForOrderQuery(orderId);
  const shiftQuery = useCashShiftQuery(openShiftId);

  const [issueError, setIssueError] = useState<string | null>(null);
  const [issuing, setIssuing] = useState(false);

  const [pendingTenders, setPendingTenders] = useState<PendingTender[]>([]);
  const [tenderError, setTenderError] = useState<string | null>(null);
  const [submittingTender, setSubmittingTender] = useState(false);

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

  const invoices = invoicesQuery.data ?? [];
  const invoice = invoices[0] ?? null;
  const payments = paymentsQuery.data ?? [];
  const canBill = canOfferBilling(principal);
  const canVoid = canOfferReversal(principal);

  async function handleIssueInvoice() {
    if (!orderId || !principal) return;
    setIssuing(true);
    setIssueError(null);
    try {
      await issueInvoice(orderId, principal.user_id);
      await queryClient.invalidateQueries({ queryKey: queryKeys.invoices(orderId) });
    } catch (err) {
      setIssueError(billingErrorMessage(err));
    } finally {
      setIssuing(false);
    }
  }

  function addPendingTender() {
    setPendingTenders((prev) => [
      ...prev,
      { method: "CASH", amountPaise: 0, tenderedRupees: "" },
    ]);
  }

  function updatePendingTender(index: number, patch: Partial<PendingTender>) {
    setPendingTenders((prev) => prev.map((t, i) => (i === index ? { ...t, ...patch } : t)));
  }

  function removePendingTender(index: number) {
    setPendingTenders((prev) => prev.filter((_, i) => i !== index));
  }

  async function handleSubmitTenders() {
    if (!orderId || !principal || !invoice) return;
    setTenderError(null);
    for (const t of pendingTenders) {
      if (t.amountPaise <= 0) {
        setTenderError("Every tender line needs an amount greater than zero.");
        return;
      }
    }
    setSubmittingTender(true);
    try {
      for (const t of pendingTenders) {
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
          createdByUserId: principal.user_id,
        });
      }
      setPendingTenders([]);
      await queryClient.invalidateQueries({ queryKey: queryKeys.payments(orderId) });
      if (openShiftId) {
        await queryClient.invalidateQueries({ queryKey: queryKeys.cashShift(openShiftId) });
      }
    } catch (err) {
      setTenderError(billingErrorMessage(err));
    } finally {
      setSubmittingTender(false);
    }
  }

  async function handleVoid(paymentId: string, amountPaise: number) {
    if (!orderId || !principal) return;
    setTenderError(null);
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
        createdByUserId: principal.user_id,
      });
      await queryClient.invalidateQueries({ queryKey: queryKeys.payments(orderId) });
    } catch (err) {
      setTenderError(billingErrorMessage(err));
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

  const due = invoice ? amountDuePaise(invoice, payments) : 0;
  const enteredTotal = pendingTenderTotalPaise(pendingTenders);
  const remaining = invoice ? remainingAfterPendingPaise(invoice, payments, pendingTenders) : 0;
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

      <section className="invoice-panel">
        <h2>Bill</h2>
        {invoicesQuery.isLoading && <p>Loading bill…</p>}
        {!invoice && !invoicesQuery.isLoading && (
          <button type="button" disabled={!canBill || issuing} onClick={() => void handleIssueInvoice()}>
            {issuing ? "Issuing…" : "Issue Bill"}
          </button>
        )}
        {issueError && (
          <p className="billing-error" role="alert">
            {issueError}
          </p>
        )}
        {invoice && (
          <div>
            <p>
              Invoice {invoice.invoice_number} — {invoice.status}
            </p>
            <table>
              <thead>
                <tr>
                  <th>Item</th>
                  <th>Qty</th>
                  <th>Taxable</th>
                  <th>CGST</th>
                  <th>SGST</th>
                  <th>IGST</th>
                  <th>Total</th>
                </tr>
              </thead>
              <tbody>
                {invoice.lines.map((line) => (
                  <tr key={line.id}>
                    <td>{line.description}</td>
                    <td>{line.quantity}</td>
                    <td>{formatPaiseAsRupees(line.taxable_value_paise)}</td>
                    <td>{formatPaiseAsRupees(line.cgst_paise)}</td>
                    <td>{formatPaiseAsRupees(line.sgst_paise)}</td>
                    <td>{formatPaiseAsRupees(line.igst_paise)}</td>
                    <td>{formatPaiseAsRupees(line.total_paise)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <p>Subtotal: {formatPaiseAsRupees(invoice.subtotal_paise)}</p>
            <p>Round off: {formatPaiseAsRupees(invoice.round_off_paise)}</p>
            <p className="grand-total">Grand Total: {formatPaiseAsRupees(invoice.grand_total_paise)}</p>
            <p className="amount-due">Amount Due: {formatPaiseAsRupees(due)}</p>
          </div>
        )}
      </section>

      {invoice && (
        <section className="payments-panel">
          <h2>Payments</h2>
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
              {payments.map((p) => (
                <tr key={p.id}>
                  <td>{p.method}</td>
                  <td>{formatPaiseAsRupees(p.amount_paise)}</td>
                  <td>{p.status}</td>
                  <td>{p.reverses_payment_id ?? "—"}</td>
                  <td>
                    {/* No "edit payment" affordance exists anywhere in this
                        screen — a correction is always a new reversal row
                        (task requirement, docs/spec/payments.md). */}
                    {canVoid && p.reverses_payment_id === null && p.status === "CAPTURED" && (
                      <button type="button" onClick={() => void handleVoid(p.id, p.amount_paise)}>
                        Void / Refund
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          <h3>Take Payment</h3>
          {pendingTenders.map((t, i) => (
            <div key={i} className="pending-tender-row">
              <select
                value={t.method}
                onChange={(e) => updatePendingTender(i, { method: e.target.value as PaymentMethod })}
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
                  updatePendingTender(i, { amountPaise: parseRupeesToPaise(e.target.value) ?? 0 })
                }
              />
              {t.method === "CASH" && (
                <input
                  placeholder="Tendered ₹ (optional)"
                  inputMode="decimal"
                  value={t.tenderedRupees}
                  onChange={(e) => updatePendingTender(i, { tenderedRupees: e.target.value })}
                />
              )}
              <button type="button" onClick={() => removePendingTender(i)}>
                Remove
              </button>
            </div>
          ))}
          <button type="button" disabled={!canBill} onClick={addPendingTender}>
            + Add Tender
          </button>
          <p>Entered so far: {formatPaiseAsRupees(enteredTotal)}</p>
          <p>Remaining after entered tenders: {formatPaiseAsRupees(remaining)}</p>
          {tenderError && (
            <p className="billing-error" role="alert">
              {tenderError}
            </p>
          )}
          <button
            type="button"
            disabled={!canBill || pendingTenders.length === 0 || submittingTender}
            onClick={() => void handleSubmitTenders()}
          >
            {submittingTender ? "Recording…" : "Record Payment(s)"}
          </button>
        </section>
      )}
    </main>
  );
}
