import { useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { useCurrentStockQuery } from "../lib/queries";
import { queryKeys } from "../lib/queries";
import { recordWastage } from "../lib/tauri";
import { canCountInventory, formatMicroQuantity, inventoryErrorMessage } from "../domain/inventory";
import { useAuthStore } from "../store/auth";

// Wastage reason codes — mirrors `stock_ledger_entry.reason_code`'s own
// column comment ("wastage: SPOILAGE, PREP_LOSS, BREAKAGE, ...") in
// `edge/database/src/error.rs`'s doc comment on `WastageReasonRequired`. Not
// a `packages/contracts` enum (no mirror exists for this M4 wastage
// vocabulary), so this is the one place in this crate a wastage reason code
// is spelled out.
const WASTAGE_REASON_CODES = ["SPOILAGE", "PREP_LOSS", "BREAKAGE", "OTHER"] as const;

// Gated on `inventory.count` per this track's dispatch brief. Note:
// `edge/database/src/error.rs`'s doc comment on `WastageReasonRequired`
// states wastage recording is gated on `inventory.manage` — a discrepancy
// with the brief, reported rather than silently resolved either way; this
// screen follows the brief's explicit instruction.
export function WastageScreen() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const principal = useAuthStore((s) => s.principal);
  const stockQuery = useCurrentStockQuery();
  const lines = stockQuery.data ?? [];

  const [itemId, setItemId] = useState("");
  const [quantity, setQuantity] = useState("");
  const [reasonCode, setReasonCode] = useState<(typeof WASTAGE_REASON_CODES)[number]>("SPOILAGE");
  const [note, setNote] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const canSubmit = canCountInventory(principal);
  const selectedLine = lines.find((l) => l.inventory_item_id === itemId) ?? null;
  const parsedQuantity = Number.parseInt(quantity, 10);
  const quantityValid = quantity.trim() !== "" && Number.isInteger(parsedQuantity) && parsedQuantity > 0;
  const canOfferSubmit = canSubmit && itemId !== "" && quantityValid && !submitting;

  async function handleSubmit() {
    if (!canOfferSubmit || !principal) return;
    setSubmitting(true);
    setError(null);
    setSuccess(null);
    try {
      const entry = await recordWastage({
        inventoryItemId: itemId,
        quantity: parsedQuantity,
        reasonCode,
        note: note.trim() === "" ? null : note.trim(),
        createdByUserId: principal.user_id,
      });
      await queryClient.invalidateQueries({ queryKey: queryKeys.currentStock });
      setSuccess(
        `Recorded ${formatMicroQuantity(entry.quantity_applied_micro, entry.dimension)} of ${entry.inventory_item_name} as wastage.`,
      );
      setQuantity("");
      setNote("");
    } catch (err) {
      setError(inventoryErrorMessage(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <main className="wastage-screen">
      <header>
        <h1>Record Wastage</h1>
        <button type="button" onClick={() => void navigate({ to: "/inventory/stock" })}>
          Back to Stock
        </button>
      </header>

      {!canSubmit && (
        <p role="alert">You do not have permission to record wastage.</p>
      )}

      <div className="wastage-form">
        <label>
          Item
          <select value={itemId} onChange={(e) => setItemId(e.target.value)} disabled={!canSubmit}>
            <option value="">Select item…</option>
            {lines.map((l) => (
              <option key={l.inventory_item_id} value={l.inventory_item_id}>
                {l.inventory_item_name}
              </option>
            ))}
          </select>
        </label>

        <label>
          Quantity {selectedLine ? `(whole ${selectedLine.dimension === "MASS" ? "grams" : selectedLine.dimension === "VOLUME" ? "millilitres" : "pieces"})` : ""}
          <input
            inputMode="numeric"
            value={quantity}
            onChange={(e) => setQuantity(e.target.value)}
            disabled={!canSubmit}
          />
        </label>

        <label>
          Reason
          <select
            value={reasonCode}
            onChange={(e) => setReasonCode(e.target.value as (typeof WASTAGE_REASON_CODES)[number])}
            disabled={!canSubmit}
          >
            {WASTAGE_REASON_CODES.map((code) => (
              <option key={code} value={code}>
                {code}
              </option>
            ))}
          </select>
        </label>

        <label>
          Note (optional)
          <input value={note} onChange={(e) => setNote(e.target.value)} disabled={!canSubmit} />
        </label>

        {error && (
          <p className="wastage-error" role="alert">
            {error}
          </p>
        )}
        {success && <p className="wastage-success">{success}</p>}

        <button type="button" disabled={!canOfferSubmit} onClick={() => void handleSubmit()}>
          {submitting ? "Recording…" : "Record Wastage"}
        </button>
      </div>
    </main>
  );
}
