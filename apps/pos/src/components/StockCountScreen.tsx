import { useState } from "react";
import { useNavigate, useParams } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import {
  queryKeys,
  useCurrentStockQuery,
  useStockCountLinesQuery,
  useStockCountQuery,
  useStockCountVarianceReportQuery,
} from "../lib/queries";
import { addOrUpdateStockCountLine, completeStockCount, openStockCount } from "../lib/tauri";
import {
  canCountInventory,
  entryIntentEcho,
  entryUnitName,
  formatMicroQuantity,
  formatVarianceBps,
  inventoryErrorMessage,
  isCountCompleted,
  isCountOpen,
} from "../domain/inventory";
import { useAuthStore } from "../store/auth";

/** `/inventory/counts` — start a new physical count, or jump to one already
 * known by id (there is no `list_stock_counts` command exposed to this app —
 * see this task's report). Gated on `inventory.count` (dispatch brief). */
export function StockCountListScreen() {
  const navigate = useNavigate();
  const principal = useAuthStore((s) => s.principal);
  const [note, setNote] = useState("");
  const [resumeId, setResumeId] = useState("");
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const canCount = canCountInventory(principal);

  async function handleOpen() {
    if (!canCount) return;
    setOpening(true);
    setError(null);
    try {
      const count = await openStockCount(
        principal?.user_id ?? null,
        note.trim() === "" ? null : note.trim(),
      );
      void navigate({ to: "/inventory/counts/$stockCountId", params: { stockCountId: count.id } });
    } catch (err) {
      setError(inventoryErrorMessage(err));
    } finally {
      setOpening(false);
    }
  }

  return (
    <main className="stock-count-list-screen">
      <header>
        <h1>Stock Counts</h1>
        <button type="button" onClick={() => void navigate({ to: "/inventory/stock" })}>
          Back to Stock
        </button>
      </header>

      {!canCount && <p role="alert">You do not have permission to run a stock count.</p>}

      <div className="stock-count-open-form">
        <label>
          Note (optional)
          <input value={note} onChange={(e) => setNote(e.target.value)} disabled={!canCount} />
        </label>
        {error && (
          <p className="stock-count-error" role="alert">
            {error}
          </p>
        )}
        <button type="button" disabled={!canCount || opening} onClick={() => void handleOpen()}>
          {opening ? "Opening…" : "Start New Count"}
        </button>
      </div>

      <div className="stock-count-resume-form">
        <label>
          Resume count by id
          <input value={resumeId} onChange={(e) => setResumeId(e.target.value)} />
        </label>
        <button
          type="button"
          disabled={resumeId.trim() === ""}
          onClick={() =>
            void navigate({
              to: "/inventory/counts/$stockCountId",
              params: { stockCountId: resumeId.trim() },
            })
          }
        >
          Open
        </button>
      </div>
    </main>
  );
}

/** `/inventory/counts/$stockCountId` — add/correct counted lines while
 * OPEN, complete the count, and (once COMPLETED) show its variance report.
 * A count is mutable while OPEN and rejected once COMPLETED — that rejection
 * is surfaced as a clear message via `inventoryErrorMessage`, never a
 * silent no-op. */
export function StockCountScreen() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { stockCountId } = useParams({ from: "/inventory/counts/$stockCountId" });
  const principal = useAuthStore((s) => s.principal);
  const canCount = canCountInventory(principal);

  const countQuery = useStockCountQuery(stockCountId);
  const linesQuery = useStockCountLinesQuery(stockCountId);
  const stockQuery = useCurrentStockQuery();
  const count = countQuery.data ?? null;
  const completed = isCountCompleted(count);
  const varianceQuery = useStockCountVarianceReportQuery(completed ? stockCountId : null);

  const [itemId, setItemId] = useState("");
  const [quantity, setQuantity] = useState("");
  const [lineNote, setLineNote] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [completing, setCompleting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const open = isCountOpen(count);
  const parsedQuantity = Number.parseInt(quantity, 10);
  const quantityValid = quantity.trim() !== "" && Number.isInteger(parsedQuantity) && parsedQuantity >= 0;
  const canOfferLineSubmit = canCount && open && itemId !== "" && quantityValid && !submitting;
  // The unit belongs to the SELECTED ITEM, not the screen: entry is grams for
  // MASS, millilitres for VOLUME, pieces for COUNT. The field used to read
  // "whole units" and name none of them, so counting oil in litres recorded
  // millilitres -- a silent 1000x that lands straight in the variance report.
  const selectedLine = (stockQuery.data ?? []).find((l) => l.inventory_item_id === itemId);

  async function handleAddLine() {
    if (!canOfferLineSubmit) return;
    setSubmitting(true);
    setError(null);
    try {
      await addOrUpdateStockCountLine({
        stockCountId,
        inventoryItemId: itemId,
        quantity: parsedQuantity,
        note: lineNote.trim() === "" ? null : lineNote.trim(),
      });
      await queryClient.invalidateQueries({ queryKey: queryKeys.stockCountLines(stockCountId) });
      setQuantity("");
      setLineNote("");
    } catch (err) {
      setError(inventoryErrorMessage(err));
    } finally {
      setSubmitting(false);
    }
  }

  async function handleComplete() {
    if (!canCount || !open) return;
    setCompleting(true);
    setError(null);
    try {
      await completeStockCount(stockCountId);
      await queryClient.invalidateQueries({ queryKey: queryKeys.stockCount(stockCountId) });
      await queryClient.invalidateQueries({ queryKey: queryKeys.currentStock });
    } catch (err) {
      setError(inventoryErrorMessage(err));
    } finally {
      setCompleting(false);
    }
  }

  return (
    <main className="stock-count-screen">
      <header>
        <h1>Stock Count {count ? `— ${count.status}` : ""}</h1>
        <button type="button" onClick={() => void navigate({ to: "/inventory/counts" })}>
          Back to Counts
        </button>
      </header>

      {countQuery.isLoading && <p>Loading count…</p>}
      {countQuery.isSuccess && count === null && <p role="alert">Count not found.</p>}
      {error && (
        <p className="stock-count-error" role="alert">
          {error}
        </p>
      )}

      {open && (
        <div className="stock-count-line-form">
          {!canCount && <p role="alert">You do not have permission to enter counted lines.</p>}
          <label>
            Item
            <select value={itemId} onChange={(e) => setItemId(e.target.value)} disabled={!canCount}>
              <option value="">Select item…</option>
              {(stockQuery.data ?? []).map((l) => (
                <option key={l.inventory_item_id} value={l.inventory_item_id}>
                  {l.inventory_item_name}
                </option>
              ))}
            </select>
          </label>
          <label>
            {selectedLine
              ? `Counted quantity (whole ${entryUnitName(selectedLine.dimension)})`
              : "Counted quantity — select an item first"}
            <span className="stock-entry-input-row">
              <input
                inputMode="numeric"
                value={quantity}
                onChange={(e) => setQuantity(e.target.value)}
                disabled={!canCount || !selectedLine}
              />
              {/* Echoed beside the field as well as in the label: the label is
                  read once, the suffix is visible while typing. */}
              {selectedLine && (
                <span className="stock-entry-unit">{entryUnitName(selectedLine.dimension)}</span>
              )}
            </span>
            {selectedLine && quantityValid && (
              // Live restatement of the intent, not a second label: see
              // `entryIntentEcho`. Catches the wrong unit -- and the wrong
              // row -- at the moment the intent forms.
              <span className="stock-entry-echo">
                {entryIntentEcho(
                  "Counting",
                  parsedQuantity,
                  selectedLine.dimension,
                  selectedLine.inventory_item_name,
                  // A count is a balance, not a movement -- see
                  // `entryIntentEcho`. Without this the line reads as a
                  // consumption figure and the variance is 100% wrong.
                  "on hand",
                )}
              </span>
            )}
          </label>
          <label>
            Note (optional)
            <input value={lineNote} onChange={(e) => setLineNote(e.target.value)} disabled={!canCount} />
          </label>
          <button type="button" disabled={!canOfferLineSubmit} onClick={() => void handleAddLine()}>
            {submitting ? "Saving…" : "Add / Update Line"}
          </button>
          <button
            type="button"
            disabled={!canCount || completing}
            onClick={() => void handleComplete()}
          >
            {completing ? "Completing…" : "Complete Count"}
          </button>
        </div>
      )}

      <table className="stock-count-lines-table">
        <thead>
          <tr>
            <th>Item</th>
            <th>Counted</th>
            <th>Expected</th>
          </tr>
        </thead>
        <tbody>
          {(linesQuery.data ?? []).map((l) => (
            <tr key={l.id}>
              <td>{l.inventory_item_name}</td>
              <td>{formatMicroQuantity(l.counted_quantity_micro, l.dimension)}</td>
              <td>{formatMicroQuantity(l.expected_quantity_micro, l.dimension)}</td>
            </tr>
          ))}
        </tbody>
      </table>

      {completed && (
        <section className="stock-count-variance">
          <h2>Variance Report</h2>
          {varianceQuery.isLoading && <p>Loading variance…</p>}
          {varianceQuery.data && (
            <>
              <table>
                <thead>
                  <tr>
                    <th>Item</th>
                    <th>Counted</th>
                    <th>Expected</th>
                    <th>Variance</th>
                    <th>Variance %</th>
                  </tr>
                </thead>
                <tbody>
                  {varianceQuery.data.lines.map((l) => (
                    <tr key={l.inventory_item_id}>
                      <td>{l.inventory_item_name}</td>
                      <td>{formatMicroQuantity(l.counted_quantity_micro, l.dimension)}</td>
                      <td>{formatMicroQuantity(l.expected_quantity_micro, l.dimension)}</td>
                      <td>{formatMicroQuantity(l.variance_quantity_micro, l.dimension)}</td>
                      <td>
                        {l.variance_percentage_bps === null
                          ? "—"
                          : formatVarianceBps(l.variance_percentage_bps)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {/* "Sales unaccounted" is its own named line — never folded
                  into any line's shrinkage (task requirement). */}
              <p className="stock-count-sales-unaccounted">
                Sales unaccounted: {varianceQuery.data.sales_unaccounted}
              </p>
            </>
          )}
        </section>
      )}
    </main>
  );
}
