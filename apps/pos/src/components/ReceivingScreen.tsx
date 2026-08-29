import { useEffect, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { queryKeys, useCurrentStockQuery } from "../lib/queries";
import { grnEntryIntentEcho, recordGoodsReceipt } from "../lib/tauri";
import type { GoodsReceiptNote, GrnEntryIntentEcho, NewGrnLineRequest } from "../lib/tauri";
import {
  DECLARABLE_DIMENSIONS,
  canManageProcurement,
  echoHasDimensionDisagreement,
  entryIntentEcho,
  entryIntentRate,
  formatEnteredQuantity,
  grnGapReasonCopy,
  procurementErrorMessage,
} from "../domain/procurement";
import { formatMicroQuantity } from "../domain/inventory";
import { formatPaiseAsRupees, parseRupeesToPaise } from "../domain/money";
import { useAuthStore } from "../store/auth";

// ---------------------------------------------------------------------------
// RECEIVE A DELIVERY (M5, ADR-019, track T4)
// ---------------------------------------------------------------------------
//
// THIS SCREEN NEVER REFUSES A DELIVERY. No purchase order, a purchase order
// this till has never synced, an item the order does not list, a supplier on
// no list, a unit nothing can convert — every one of those is recorded with a
// gap attached and the receipt COMPLETES. The commit button is never disabled
// for any of them; the only things that gate it are "no lines yet", "not
// permitted" and "already submitting".
//
// Refusing the delivery is the outage, not the protection: a refused receipt
// does not keep the crate out of the walk-in, it only stops the system
// knowing it went in.
//
// THE ECHO IS MANDATORY (M5 acceptance criterion 4). The operator types in the
// SUPPLIER's unit off a delivery note; the screen shows what will actually be
// recorded, in base units, BEFORE the commit — "4 SACK → 200kg of Basmati
// Rice", with the applied rate under it. Every figure in it is computed by
// the edge's own resolution (`grn_entry_intent_echo`), which is the same
// function the write runs, so the echo and the record cannot disagree.
//
// A `Db` READ-SURFACE GAP, REPORTED NOT ROUTED AROUND: there is no sanctioned
// read for `supplier` or `purchase_order` on the edge crate, so this screen
// takes both as typed references rather than pickers. That is the ONLY
// possible shape for the "purchase order that never synced" case, and a
// stopgap for the case where the row is present locally. See
// `apps/pos/src-tauri/src/commands/procurement.rs`'s module doc.

/** How long the screen waits after the last keystroke before asking the edge
 * to resolve the line. The echo has to feel live under the cursor — this is a
 * coalescing window, not a delay a person waits out. */
const ECHO_DEBOUNCE_MS = 250;

interface DraftLine {
  /** Screen-local key only. The receipt's real ids are minted at the edge. */
  key: string;
  request: NewGrnLineRequest;
  /** The edge's own echo for this line, captured when it was added, so the
   * review list restates both sides of the conversion rather than only what
   * was typed. */
  echo: GrnEntryIntentEcho;
}

export function ReceivingScreen() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const principal = useAuthStore((s) => s.principal);
  const canReceive = canManageProcurement(principal);
  const stockQuery = useCurrentStockQuery();
  const items = stockQuery.data ?? [];

  // ---- delivery header. Every one of these is optional and stays optional.
  const [supplierRef, setSupplierRef] = useState("");
  const [purchaseOrderRef, setPurchaseOrderRef] = useState("");
  const [deliveryNoteRef, setDeliveryNoteRef] = useState("");
  const [notes, setNotes] = useState("");

  // ---- the line being entered
  const [itemId, setItemId] = useState("");
  const [purchaseUnit, setPurchaseUnit] = useState("");
  const [quantity, setQuantity] = useState("");
  // STARTS EMPTY AND IS NEVER DEFAULTED FROM THE SELECTED ITEM. See
  // DECLARABLE_DIMENSIONS in domain/procurement.ts: auto-filling this from
  // `inventory_item.dimension` turns the edge's mismatch guard into `x == x`
  // and it can then never fire.
  const [declaredDimension, setDeclaredDimension] = useState("");
  const [priceInput, setPriceInput] = useState("");
  const [batchCode, setBatchCode] = useState("");
  const [expiryDate, setExpiryDate] = useState("");

  const [echo, setEcho] = useState<GrnEntryIntentEcho | null>(null);
  const [echoError, setEchoError] = useState<string | null>(null);

  const [lines, setLines] = useState<DraftLine[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [recorded, setRecorded] = useState<GoodsReceiptNote | null>(null);

  const pricePaise = parseRupeesToPaise(priceInput);
  const priceValid = priceInput.trim() !== "" && pricePaise !== null && pricePaise >= 0;
  const lineComplete =
    itemId !== "" &&
    purchaseUnit.trim() !== "" &&
    quantity.trim() !== "" &&
    declaredDimension !== "" &&
    priceValid;

  const draftRequest: NewGrnLineRequest | null = lineComplete
    ? {
        inventory_item_id: itemId,
        entered_purchase_unit: purchaseUnit.trim(),
        entered_quantity: quantity.trim(),
        quantity_dimension: declaredDimension,
        purchase_price_paise: pricePaise as number,
        batch_code: batchCode.trim() === "" ? null : batchCode.trim(),
        expiry_date: expiryDate.trim() === "" ? null : expiryDate.trim(),
        purchase_order_line_id: null,
      }
    : null;

  const draftKey = draftRequest === null ? "" : JSON.stringify(draftRequest);
  const supplierIdOrNull = supplierRef.trim() === "" ? null : supplierRef.trim();

  // Resolve the echo at the EDGE on every settled keystroke. Deliberately not
  // computed here: the conversion happens exactly once, in the function the
  // write itself calls (ADR-019 §3).
  useEffect(() => {
    if (draftKey === "") {
      setEcho(null);
      setEchoError(null);
      return;
    }
    let cancelled = false;
    // `setTimeout` is called FREE here, never stored on an object field —
    // these globals are receiver-bound and throw `Illegal invocation` when
    // detached (CLAUDE.md).
    const timer = setTimeout(() => {
      void (async () => {
        try {
          const next = await grnEntryIntentEcho(
            supplierIdOrNull,
            JSON.parse(draftKey) as NewGrnLineRequest,
          );
          if (!cancelled) {
            setEcho(next);
            setEchoError(null);
          }
        } catch (err) {
          if (!cancelled) {
            setEcho(null);
            setEchoError(procurementErrorMessage(err));
          }
        }
      })();
    }, ECHO_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [draftKey, supplierIdOrNull]);

  function addLine() {
    if (draftRequest === null || echo === null) return;
    setLines((current) => [
      ...current,
      { key: `${Date.now()}-${current.length}`, request: draftRequest, echo },
    ]);
    setItemId("");
    setPurchaseUnit("");
    setQuantity("");
    setDeclaredDimension("");
    setPriceInput("");
    setBatchCode("");
    setExpiryDate("");
    setEcho(null);
  }

  function removeLine(key: string) {
    setLines((current) => current.filter((l) => l.key !== key));
  }

  async function handleSubmit() {
    if (!canReceive || !principal || lines.length === 0 || submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      const grn = await recordGoodsReceipt({
        // A blank box is an ABSENT purchase order, which is an ordinary,
        // accepted receipt — never a reason to stop here.
        purchaseOrderId: purchaseOrderRef.trim() === "" ? null : purchaseOrderRef.trim(),
        supplierId: supplierIdOrNull,
        deliveryNoteRef: deliveryNoteRef.trim() === "" ? null : deliveryNoteRef.trim(),
        notes: notes.trim() === "" ? null : notes.trim(),
        receivedByUserId: principal.user_id,
        lines: lines.map((l) => l.request),
      });
      await queryClient.invalidateQueries({ queryKey: queryKeys.currentStock });
      await queryClient.invalidateQueries({ queryKey: queryKeys.grnGaps });
      setRecorded(grn);
      setLines([]);
      setPurchaseOrderRef("");
      setDeliveryNoteRef("");
      setNotes("");
    } catch (err) {
      setError(procurementErrorMessage(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <main className="receiving-screen">
      <header>
        <h1>Receive Delivery</h1>
        <button type="button" onClick={() => void navigate({ to: "/inventory/stock" })}>
          Back to Stock
        </button>
        <button type="button" onClick={() => void navigate({ to: "/procurement/gaps" })}>
          Delivery Problems
        </button>
      </header>

      {!canReceive && <p role="alert">You do not have permission to receive deliveries.</p>}

      {recorded && (
        <section className="receiving-result">
          <h2>Recorded as {recorded.grn_number}</h2>
          <p>
            Stock has been increased. Business date {recorded.business_date}.
            {recorded.purchase_order_id === null
              ? " No purchase order was recorded against this delivery."
              : ` Against purchase order reference ${recorded.purchase_order_id}.`}
          </p>
          <table>
            <thead>
              <tr>
                <th>Item</th>
                <th>Entered</th>
                <th>Recorded against stock</th>
                <th>Cost per base unit</th>
                <th>Line total</th>
              </tr>
            </thead>
            <tbody>
              {recorded.lines.map((line) => {
                const item = items.find((i) => i.inventory_item_id === line.inventory_item_id);
                return (
                  <tr key={line.id}>
                    <td>{item?.inventory_item_name ?? line.inventory_item_id}</td>
                    {/* BOTH SIDES OF THE CONVERSION, always. "What did they
                        actually type?" has to stay answerable from the row. */}
                    <td>
                      {formatEnteredQuantity(
                        line.entered_quantity_micro,
                        line.entered_purchase_unit,
                      )}
                    </td>
                    <td>
                      {item
                        ? formatMicroQuantity(line.base_quantity_micro, item.dimension)
                        : `${line.base_quantity_micro} micro-units`}
                    </td>
                    <td>{formatPaiseAsRupees(line.unit_cost_paise)}</td>
                    <td>{formatPaiseAsRupees(line.line_total_paise)}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>

          {/* M5 ACCEPTANCE CRITERION 3, at the moment it matters most: the
              operator sees what could not be matched about the delivery they
              just took, while the driver is still there. Each row carries its
              OWN reason — never one heading for all of them. */}
          {recorded.gaps.length > 0 && (
            <div className="receiving-gaps" role="alert">
              <h3>
                {recorded.gaps.length === 1
                  ? "1 thing could not be matched"
                  : `${recorded.gaps.length} things could not be matched`}
              </h3>
              <p>The delivery is recorded and stock is correct. These still need someone.</p>
              <ul>
                {recorded.gaps.map((gap) => (
                  <li key={gap.id}>
                    <strong>{grnGapReasonCopy(gap.reason).title}</strong>
                    {gap.detail !== null && <span> — {gap.detail}</span>}
                    <div>{grnGapReasonCopy(gap.reason).nextStep}</div>
                  </li>
                ))}
              </ul>
            </div>
          )}
          <button type="button" onClick={() => setRecorded(null)}>
            Receive Another Delivery
          </button>
        </section>
      )}

      <section className="receiving-header-fields">
        <h2>Delivery</h2>
        <label>
          Supplier reference (optional)
          <input
            value={supplierRef}
            onChange={(e) => setSupplierRef(e.target.value)}
            disabled={!canReceive}
          />
        </label>
        <label>
          Purchase order reference (optional)
          <input
            value={purchaseOrderRef}
            onChange={(e) => setPurchaseOrderRef(e.target.value)}
            disabled={!canReceive}
          />
        </label>
        {/* Stated on the screen, not only in the code: a missing order never
            stops a delivery being recorded. */}
        <p className="receiving-hint">
          Leave the purchase order blank if there isn&rsquo;t one, or if this till has never seen
          it. The delivery is recorded either way and the mismatch is listed for the buyer.
        </p>
        <label>
          Supplier&rsquo;s delivery note number (optional)
          <input
            value={deliveryNoteRef}
            onChange={(e) => setDeliveryNoteRef(e.target.value)}
            disabled={!canReceive}
          />
        </label>
        <label>
          Notes (optional)
          <input value={notes} onChange={(e) => setNotes(e.target.value)} disabled={!canReceive} />
        </label>
      </section>

      <section className="receiving-line-entry">
        <h2>Add an item from the delivery note</h2>

        <label>
          Item
          <select
            value={itemId}
            onChange={(e) => setItemId(e.target.value)}
            disabled={!canReceive}
          >
            <option value="">Select item…</option>
            {items.map((i) => (
              <option key={i.inventory_item_id} value={i.inventory_item_id}>
                {i.inventory_item_name}
              </option>
            ))}
          </select>
        </label>

        <label>
          Supplier&rsquo;s unit, exactly as written on the delivery note (SACK, CRATE, kg)
          <input
            value={purchaseUnit}
            onChange={(e) => setPurchaseUnit(e.target.value)}
            disabled={!canReceive}
          />
        </label>

        <label>
          {/* NEVER "Quantity" alone. An unlabelled quantity field is a 1000x
              error waiting to happen, and this is the entry path with the
              worst odds of the three. */}
          {purchaseUnit.trim() === ""
            ? "Quantity — enter the supplier’s unit above first"
            : `Quantity, in ${purchaseUnit.trim()} (as written on the delivery note)`}
          <span className="stock-entry-input-row">
            <input
              inputMode="decimal"
              value={quantity}
              onChange={(e) => setQuantity(e.target.value)}
              disabled={!canReceive || purchaseUnit.trim() === ""}
            />
            {purchaseUnit.trim() !== "" && (
              <span className="stock-entry-unit">{purchaseUnit.trim()}</span>
            )}
          </span>
        </label>

        <label>
          What kind of unit is that, as the delivery note states it?
          <select
            value={declaredDimension}
            onChange={(e) => setDeclaredDimension(e.target.value)}
            disabled={!canReceive}
          >
            <option value="">Select…</option>
            {DECLARABLE_DIMENSIONS.map((d) => (
              <option key={d} value={d}>
                {d === "MASS" ? "Weight (MASS)" : d === "VOLUME" ? "Volume (VOLUME)" : "Count (COUNT)"}
              </option>
            ))}
          </select>
        </label>
        {/* Says out loud why this is not pre-filled. Someone WILL try to
            "helpfully" default it from the item; the comment in the code is
            not visible to them, and this line is. */}
        <p className="receiving-hint">
          Answer from the delivery note, not from how the item is set up here. This is the check
          that catches an item set up in the wrong kind of unit, and it cannot catch anything if it
          is copied from the item.
        </p>

        <label>
          Price per {purchaseUnit.trim() === "" ? "purchase unit" : purchaseUnit.trim()} (₹, off the
          delivery note)
          <input
            inputMode="decimal"
            value={priceInput}
            onChange={(e) => setPriceInput(e.target.value)}
            disabled={!canReceive}
          />
        </label>

        <label>
          Batch code (optional)
          <input
            value={batchCode}
            onChange={(e) => setBatchCode(e.target.value)}
            disabled={!canReceive}
          />
        </label>
        <label>
          Expiry date (optional, YYYY-MM-DD)
          <input
            value={expiryDate}
            onChange={(e) => setExpiryDate(e.target.value)}
            disabled={!canReceive}
          />
        </label>

        {/* ===== THE ECHO. M5 acceptance criterion 4. ===== */}
        {echoError !== null && (
          <p className="receiving-echo-error" role="alert">
            {echoError}
          </p>
        )}
        {echo !== null && (
          <div className="receiving-echo">
            <strong>{entryIntentEcho(echo)}</strong>
            <div>{entryIntentRate(echo)}</div>
            <div>
              Cost {formatPaiseAsRupees(echo.unit_cost_paise)} per base unit · line total{" "}
              {formatPaiseAsRupees(echo.line_total_paise)}
            </div>
            {echoHasDimensionDisagreement(echo) && (
              <div className="receiving-echo-warning">
                You said this is {echo.quantity_dimension}; {echo.inventory_item_name} is set up as{" "}
                {echo.item_dimension}. This will be recorded and flagged for the buyer — check the
                delivery note before committing.
              </div>
            )}
            {echo.gap_reasons.length > 0 && (
              <ul className="receiving-echo-gaps">
                {/* Shown, NEVER used to block. */}
                {echo.gap_reasons.map((reason) => (
                  <li key={reason}>{grnGapReasonCopy(reason).title}</li>
                ))}
              </ul>
            )}
          </div>
        )}

        <button
          type="button"
          disabled={!canReceive || draftRequest === null || echo === null}
          onClick={addLine}
        >
          Add to Delivery
        </button>
        {draftRequest !== null && echo === null && echoError === null && (
          <p className="receiving-hint">Checking what this will record…</p>
        )}
      </section>

      <section className="receiving-lines">
        <h2>On this delivery ({lines.length})</h2>
        {lines.length === 0 && <p>Nothing added yet.</p>}
        <ul>
          {lines.map((line) => (
            <li key={line.key}>
              {/* The review list restates the echo, not just the typed
                  figure: the last chance to see "4 SACK → 200kg" before it is
                  committed. */}
              <strong>{entryIntentEcho(line.echo)}</strong>
              <span> · {entryIntentRate(line.echo)}</span>
              <span> · {formatPaiseAsRupees(line.echo.line_total_paise)}</span>
              <button type="button" onClick={() => removeLine(line.key)} disabled={submitting}>
                Remove
              </button>
            </li>
          ))}
        </ul>

        {error !== null && (
          <p className="receiving-error" role="alert">
            {error}
          </p>
        )}

        {/* NEVER disabled for a missing purchase order or an unknown
            supplier. Only for: no permission, nothing to record, in flight. */}
        <button
          type="button"
          disabled={!canReceive || lines.length === 0 || submitting}
          onClick={() => void handleSubmit()}
        >
          {submitting ? "Recording…" : "Record Delivery"}
        </button>
      </section>
    </main>
  );
}
