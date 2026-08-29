import { useEffect, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { queryKeys, useCurrentStockQuery } from "../lib/queries";
import { grnEntryIntentEcho, recordPurchaseReturn } from "../lib/tauri";
import type {
  GrnEntryIntentEcho,
  NewPurchaseReturnLineRequest,
  PurchaseReturn,
} from "../lib/tauri";
import {
  DECLARABLE_DIMENSIONS,
  PURCHASE_RETURN_REASONS,
  canManageProcurement,
  entryIntentEcho,
  entryIntentRate,
  formatEnteredQuantity,
  procurementErrorMessage,
} from "../domain/procurement";
import { formatMicroQuantity } from "../domain/inventory";
import { formatPaiseAsRupees, parseRupeesToPaise } from "../domain/money";
import { useAuthStore } from "../store/auth";

// ---------------------------------------------------------------------------
// SEND GOODS BACK TO A SUPPLIER (M5, ADR-019, track T4)
// ---------------------------------------------------------------------------
//
// Posts RETURN_TO_VENDOR ledger entries at the edge, in one transaction with
// the return itself. Same quantity discipline as receiving, for the same
// reason: the operator types in the SUPPLIER's unit, off the same delivery
// note, and the screen echoes what will actually leave stock before they
// commit.
//
// THE ECHO IS THE RECEIVING ECHO. `grn_entry_intent_echo` is the edge's
// purchase-unit resolution and is exactly the resolution a return line takes
// too, so it is reused rather than reimplemented — a second implementation is
// a second answer. The money it reports is ignored here: a return is valued
// by the edge at what this outlet actually paid (weighted average cost),
// unless the operator states a different figure.
//
// `return_number` is typed by the operator because contracts 0.6.0 mints a
// counter for the GRN (`grn_sequence`) and none for this document. Reported as
// a contract asymmetry by `edge/database/src/procurement/numbering.rs`, and
// not invented in this layer either.

const ECHO_DEBOUNCE_MS = 250;

interface DraftReturnLine {
  key: string;
  request: NewPurchaseReturnLineRequest;
  echo: GrnEntryIntentEcho;
}

export function PurchaseReturnScreen() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const principal = useAuthStore((s) => s.principal);
  const canReturn = canManageProcurement(principal);
  const stockQuery = useCurrentStockQuery();
  const items = stockQuery.data ?? [];

  const [supplierRef, setSupplierRef] = useState("");
  const [grnRef, setGrnRef] = useState("");
  const [returnNumber, setReturnNumber] = useState("");
  const [reason, setReason] = useState<string>(PURCHASE_RETURN_REASONS[0] ?? "OTHER");
  const [notes, setNotes] = useState("");

  const [itemId, setItemId] = useState("");
  const [purchaseUnit, setPurchaseUnit] = useState("");
  const [quantity, setQuantity] = useState("");
  // Starts empty. Never derived from the item — see ReceivingScreen for why
  // that would silently disable the mismatch guard.
  const [declaredDimension, setDeclaredDimension] = useState("");
  const [unitCostInput, setUnitCostInput] = useState("");

  const [echo, setEcho] = useState<GrnEntryIntentEcho | null>(null);
  const [echoError, setEchoError] = useState<string | null>(null);
  const [lines, setLines] = useState<DraftReturnLine[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [recorded, setRecorded] = useState<PurchaseReturn | null>(null);

  const unitCostPaise = unitCostInput.trim() === "" ? null : parseRupeesToPaise(unitCostInput);
  const unitCostValid =
    unitCostInput.trim() === "" || (unitCostPaise !== null && unitCostPaise >= 0);

  const lineComplete =
    itemId !== "" &&
    purchaseUnit.trim() !== "" &&
    quantity.trim() !== "" &&
    declaredDimension !== "" &&
    unitCostValid;

  const draftRequest: NewPurchaseReturnLineRequest | null = lineComplete
    ? {
        inventory_item_id: itemId,
        grn_line_id: null,
        entered_purchase_unit: purchaseUnit.trim(),
        entered_quantity: quantity.trim(),
        quantity_dimension: declaredDimension,
        // `null` means "value it at what this outlet actually paid". Never
        // coerced to 0 — a blank field and a zero cost are different
        // statements and the edge treats them differently.
        unit_cost_paise: unitCostPaise,
      }
    : null;

  const draftKey = draftRequest === null ? "" : JSON.stringify(draftRequest);
  const supplierIdOrNull = supplierRef.trim() === "" ? null : supplierRef.trim();

  useEffect(() => {
    if (draftKey === "") {
      setEcho(null);
      setEchoError(null);
      return;
    }
    let cancelled = false;
    const request = JSON.parse(draftKey) as NewPurchaseReturnLineRequest;
    // Free call, never stored on an object field (CLAUDE.md: receiver-bound
    // globals throw `Illegal invocation` when detached).
    const timer = setTimeout(() => {
      void (async () => {
        try {
          const next = await grnEntryIntentEcho(supplierIdOrNull, {
            inventory_item_id: request.inventory_item_id,
            entered_purchase_unit: request.entered_purchase_unit,
            entered_quantity: request.entered_quantity,
            quantity_dimension: request.quantity_dimension,
            // The echo's money is not used on this screen — the edge values
            // the return itself. Zero here states "no price asserted" rather
            // than smuggling in a made-up one.
            purchase_price_paise: 0,
            batch_code: null,
            expiry_date: null,
            purchase_order_line_id: null,
          });
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
    setUnitCostInput("");
    setEcho(null);
  }

  function removeLine(key: string) {
    setLines((current) => current.filter((l) => l.key !== key));
  }

  const canOfferSubmit =
    canReturn && lines.length > 0 && returnNumber.trim() !== "" && !submitting;

  async function handleSubmit() {
    if (!canOfferSubmit || !principal) return;
    setSubmitting(true);
    setError(null);
    try {
      const stored = await recordPurchaseReturn({
        supplierId: supplierIdOrNull,
        grnId: grnRef.trim() === "" ? null : grnRef.trim(),
        returnNumber: returnNumber.trim(),
        reason,
        notes: notes.trim() === "" ? null : notes.trim(),
        returnedByUserId: principal.user_id,
        lines: lines.map((l) => l.request),
      });
      await queryClient.invalidateQueries({ queryKey: queryKeys.currentStock });
      setRecorded(stored);
      setLines([]);
      setReturnNumber("");
      setNotes("");
    } catch (err) {
      setError(procurementErrorMessage(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <main className="purchase-return-screen">
      <header>
        <h1>Return to Supplier</h1>
        <button type="button" onClick={() => void navigate({ to: "/procurement/receive" })}>
          Receive Delivery
        </button>
        <button type="button" onClick={() => void navigate({ to: "/inventory/stock" })}>
          Back to Stock
        </button>
      </header>

      {!canReturn && <p role="alert">You do not have permission to record a return.</p>}

      {recorded && (
        <section className="purchase-return-result">
          <h2>Recorded as {recorded.return_number}</h2>
          <p>
            Stock has been reduced. Business date {recorded.business_date}. Reason{" "}
            {recorded.reason}.
          </p>
          <table>
            <thead>
              <tr>
                <th>Item</th>
                <th>Entered</th>
                <th>Taken off stock</th>
                <th>Valued at (per base unit)</th>
              </tr>
            </thead>
            <tbody>
              {recorded.lines.map((line) => {
                const item = items.find((i) => i.inventory_item_id === line.inventory_item_id);
                return (
                  <tr key={line.id}>
                    <td>{item?.inventory_item_name ?? line.inventory_item_id}</td>
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
                  </tr>
                );
              })}
            </tbody>
          </table>
          <button type="button" onClick={() => setRecorded(null)}>
            Record Another Return
          </button>
        </section>
      )}

      <section className="purchase-return-header-fields">
        <h2>Return</h2>
        <label>
          Your return reference
          <input
            value={returnNumber}
            onChange={(e) => setReturnNumber(e.target.value)}
            disabled={!canReturn}
          />
        </label>
        <p className="receiving-hint">
          Use the number on your own return note. This till does not issue one.
        </p>
        <label>
          Supplier reference (optional)
          <input
            value={supplierRef}
            onChange={(e) => setSupplierRef(e.target.value)}
            disabled={!canReturn}
          />
        </label>
        <label>
          Delivery this came from (optional)
          <input value={grnRef} onChange={(e) => setGrnRef(e.target.value)} disabled={!canReturn} />
        </label>
        <label>
          Reason
          <select value={reason} onChange={(e) => setReason(e.target.value)} disabled={!canReturn}>
            {PURCHASE_RETURN_REASONS.map((r) => (
              <option key={r} value={r}>
                {r}
              </option>
            ))}
          </select>
        </label>
        <label>
          Notes (optional)
          <input value={notes} onChange={(e) => setNotes(e.target.value)} disabled={!canReturn} />
        </label>
      </section>

      <section className="purchase-return-line-entry">
        <h2>Add an item going back</h2>
        <label>
          Item
          <select value={itemId} onChange={(e) => setItemId(e.target.value)} disabled={!canReturn}>
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
            disabled={!canReturn}
          />
        </label>

        <label>
          {purchaseUnit.trim() === ""
            ? "Quantity — enter the supplier’s unit above first"
            : `Quantity going back, in ${purchaseUnit.trim()}`}
          <span className="stock-entry-input-row">
            <input
              inputMode="decimal"
              value={quantity}
              onChange={(e) => setQuantity(e.target.value)}
              disabled={!canReturn || purchaseUnit.trim() === ""}
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
            disabled={!canReturn}
          >
            <option value="">Select…</option>
            {DECLARABLE_DIMENSIONS.map((d) => (
              <option key={d} value={d}>
                {d === "MASS" ? "Weight (MASS)" : d === "VOLUME" ? "Volume (VOLUME)" : "Count (COUNT)"}
              </option>
            ))}
          </select>
        </label>
        <p className="receiving-hint">
          Answer from the delivery note, not from how the item is set up here.
        </p>

        <label>
          Credit per base unit (₹, optional)
          <input
            inputMode="decimal"
            value={unitCostInput}
            onChange={(e) => setUnitCostInput(e.target.value)}
            disabled={!canReturn}
          />
        </label>
        <p className="receiving-hint">
          Leave blank to value the return at what this outlet actually paid. Blank is not zero.
        </p>

        {echoError !== null && (
          <p className="receiving-echo-error" role="alert">
            {echoError}
          </p>
        )}
        {echo !== null && (
          <div className="receiving-echo">
            <strong>{entryIntentEcho(echo)}</strong>
            <div>{entryIntentRate(echo)}</div>
            <div>This much will be taken off stock.</div>
          </div>
        )}

        <button
          type="button"
          disabled={!canReturn || draftRequest === null || echo === null}
          onClick={addLine}
        >
          Add to Return
        </button>
      </section>

      <section className="purchase-return-lines">
        <h2>Going back ({lines.length})</h2>
        {lines.length === 0 && <p>Nothing added yet.</p>}
        <ul>
          {lines.map((line) => (
            <li key={line.key}>
              <strong>{entryIntentEcho(line.echo)}</strong>
              <span> · {entryIntentRate(line.echo)}</span>
              <button type="button" onClick={() => removeLine(line.key)} disabled={submitting}>
                Remove
              </button>
            </li>
          ))}
        </ul>

        {error !== null && (
          <p className="purchase-return-error" role="alert">
            {error}
          </p>
        )}
        {canReturn && lines.length > 0 && returnNumber.trim() === "" && (
          <p className="receiving-hint">Enter your return reference above before recording.</p>
        )}

        <button type="button" disabled={!canOfferSubmit} onClick={() => void handleSubmit()}>
          {submitting ? "Recording…" : "Record Return"}
        </button>
      </section>
    </main>
  );
}
