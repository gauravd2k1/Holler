import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { listGoodsReceipts } from "../lib/api";
import { formatMicro } from "../lib/quantity";
import { formatPaise } from "../lib/money";

/**
 * The goods-receipt list.
 *
 * THIS SCREEN SHOWS THE CLOUD'S REPLICA AND SAYS SO. A goods receipt is
 * EDGE-AUTHORITATIVE (ADR-019): the outlet recorded it, the cloud holds a copy,
 * and the two can legitimately differ — a receipt taken while the uplink was
 * down is real, complete and correct at the till and simply absent here until
 * it replays. Presenting this as "the receipts" would make an operator read a
 * sync delay as missing stock.
 *
 * Same rule the same ADR applies to purchase-order receipt progress, where the
 * edge and the cloud each derive a different number from their own rows and
 * both are right: show both, label them, never reconcile them.
 *
 * ALL THREE QUANTITY COLUMNS ARE RENDERED. `entered` is what the receiver
 * typed, `pack size` is what it was multiplied by, `base` is the result. When a
 * receipt turns out 1000x wrong, "what did they actually type?" has to be
 * answerable from this table (ADR-019 §3) — a screen that showed only the base
 * quantity would be the one place that question goes to die.
 */
export function GoodsReceiptsScreen() {
  const [cursor, setCursor] = useState<string | undefined>(undefined);
  const receipts = useQuery({
    queryKey: ["goods-receipts", cursor ?? "first"],
    queryFn: () => listGoodsReceipts(cursor),
  });

  if (receipts.isLoading) return <p>Loading goods receipts…</p>;
  if (receipts.isError) {
    return <p className="error">Could not load goods receipts: {String(receipts.error)}</p>;
  }

  const page = receipts.data;
  const items = page?.items ?? [];

  return (
    <section>
      <h1>Goods receipts</h1>
      <p className="replica-note" role="note">
        <strong>This is the cloud's copy of what the tills recorded.</strong> A receipt
        taken while an outlet was offline is complete and correct at that till and appears
        here only once it has replayed. A receipt missing from this list has not
        necessarily been missed.
      </p>

      {items.length === 0 ? (
        <p>No goods receipts have replayed for this outlet yet.</p>
      ) : (
        items.map((grn) => (
          <article key={grn.id}>
            <h2>{grn.grn_number}</h2>
            <p>
              Received {grn.received_at} · business date {grn.business_date}
              {" · "}
              {/*
                Nulls are shown as absences, never as a placeholder that implies
                a link. A GRN never blocks on a PO: goods arrive against an order
                that never synced, against one amended after dispatch, and
                against none at all.
              */}
              {grn.purchase_order_id === null ? (
                <span className="muted">no purchase order</span>
              ) : (
                <>PO {grn.purchase_order_id}</>
              )}
              {" · "}
              {grn.supplier_id === null ? (
                <span className="muted">no supplier recorded</span>
              ) : (
                <>supplier {grn.supplier_id}</>
              )}
            </p>
            <table>
              <thead>
                <tr>
                  <th>Item</th>
                  <th>Entered</th>
                  <th>Pack size</th>
                  <th>Base quantity</th>
                  <th>Dimension</th>
                  <th>Line total</th>
                </tr>
              </thead>
              <tbody>
                {grn.lines.map((line) => (
                  <tr key={line.id}>
                    <td>{line.inventory_item_id}</td>
                    <td>{formatMicro(line.entered_quantity_micro)}</td>
                    <td>{formatMicro(line.pack_size_micro_applied)}</td>
                    <td>{formatMicro(line.base_quantity_micro)}</td>
                    <td>{line.quantity_dimension}</td>
                    <td>{formatPaise(line.line_total_paise)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </article>
        ))
      )}

      {/*
        A next page exists only when the server says so. Paging on "the page
        came back full" costs one guaranteed empty round trip at the end of
        every list and reads as a bug the first time someone notices it.
      */}
      {page?.next_cursor != null && (
        <button type="button" onClick={() => setCursor(page.next_cursor ?? undefined)}>
          Load more
        </button>
      )}
    </section>
  );
}
