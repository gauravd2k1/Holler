import { useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { queryKeys, useMenuItemsQuery, useUnacceptedAggregatorOrdersQuery } from "../lib/queries";
import { acceptAggregatorOrder, type UnacceptedAggregatorOrder } from "../lib/tauri";
import { formatPaiseAsRupees } from "../domain/money";
import { formatIST } from "../lib/datetime";

// ---------------------------------------------------------------------------
// DELIVERY-PLATFORM ORDERS — M6 C1
// ---------------------------------------------------------------------------
//
// Every document on this screen ALREADY ARRIVED. ADR-022's published guarantee
// is that a new aggregator order cannot arrive while the uplink is down, and
// that one already received is fully operable offline — so this screen reads the
// till's own database and never the network, and it keeps working with the cloud
// stopped. That is exactly what C1 observes.
//
// ACCEPTING A DOCUMENT IS CREATING ITS LOCAL ORDER. There is no accept flag
// anywhere: the document is a read-only mirror of a cloud-authoritative
// aggregate, and acceptance is derived from the existence of an `order` carrying
// its `external_order_id`. Once accepted, the order is an ordinary order — it
// bills, prints and closes on the paths every other order uses, and this screen
// hands it straight to them.
//
// AN UNMATCHED LINE IS SHOWN, NOT HIDDEN. A line whose `menu_item_id` is null
// is normal (no local item maps to that platform item id) and cannot become an
// order line, because `order_item.menu_item_id` is a real NOT NULL foreign key.
// Hiding such a line would make the order look smaller than what the customer
// was told they ordered, so each one is listed and counted, and the accept
// result says how many were left behind.
export function AggregatorOrdersScreen() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const documentsQuery = useUnacceptedAggregatorOrdersQuery();
  const documents = documentsQuery.data ?? [];
  const menuItemsQuery = useMenuItemsQuery();
  const menuItemName = (menuItemId: string): string =>
    menuItemsQuery.data?.find((m) => m.id === menuItemId)?.name ?? "matched item";

  const [accepting, setAccepting] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [accepted, setAccepted] = useState<{
    orderId: string;
    displayNumber: string | null;
    externalOrderId: string;
    linesCreated: number;
    linesUnmapped: number;
  } | null>(null);

  async function onAccept(doc: UnacceptedAggregatorOrder) {
    setError(null);
    setAccepting(doc.id);
    try {
      const result = await acceptAggregatorOrder(doc.id);
      setAccepted({
        orderId: result.order.holler_order_id,
        displayNumber: result.order.display_number ?? null,
        externalOrderId: result.external_order_id,
        linesCreated: result.lines_created,
        linesUnmapped: result.lines_unmapped,
      });
      await queryClient.invalidateQueries({ queryKey: queryKeys.unacceptedAggregatorOrders });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setAccepting(null);
    }
  }

  return (
    <main className="aggregator-orders-screen">
      <header>
        <h1>Delivery-Platform Orders</h1>
        <button type="button" onClick={() => void navigate({ to: "/orders" })}>
          Orders
        </button>
        <button type="button" onClick={() => void navigate({ to: "/" })}>
          Back to Till
        </button>
      </header>

      {/* Said on the screen, not only in a comment: these arrived earlier and
          are workable with no uplink. A cashier should not wonder whether the
          internet being down means this list is stale. */}
      <p className="aggregator-orders-hint">
        These orders have already reached this till. They can be accepted, billed and closed with
        the internet down. A new platform order cannot arrive while the connection is down.
      </p>

      {documentsQuery.isLoading && <p>Loading…</p>}
      {documentsQuery.isError && (
        <p role="alert">Could not read platform orders from this till&rsquo;s database.</p>
      )}
      {error !== null && <p role="alert">{error}</p>}

      {accepted !== null && (
        <section className="aggregator-accepted" role="status">
          {/* The order's human-facing number, never its UUID. */}
          <h2>Accepted as order {accepted.displayNumber ?? "(unnumbered)"}</h2>
          <p>
            Platform order {accepted.externalOrderId} · {accepted.linesCreated} line
            {accepted.linesCreated === 1 ? "" : "s"} on the bill
            {accepted.linesUnmapped > 0
              ? ` · ${accepted.linesUnmapped} line${
                  accepted.linesUnmapped === 1 ? "" : "s"
                } could not be matched to an item on this menu and are NOT on the bill`
              : ""}
          </p>
          <button
            type="button"
            onClick={() =>
              void navigate({
                to: "/orders/$orderId/billing",
                params: { orderId: accepted.orderId },
              })
            }
          >
            Bill this order
          </button>
        </section>
      )}

      {!documentsQuery.isLoading && documents.length === 0 && (
        <p>No platform orders are waiting to be accepted.</p>
      )}

      <ul className="aggregator-orders-list">
        {documents.map((doc) => {
          const unmatched = doc.lines.filter((l) => l.menu_item_id === null).length;
          return (
            <li key={doc.id} className="aggregator-order">
              <h2>
                {doc.platform} · {doc.external_order_id}
              </h2>
              <p>
                Platform status {doc.platform_status} · received {formatIST(doc.received_at)} ·
                business date {doc.business_date} · document version {doc.document_version}
              </p>
              {/* The platform's own stated total, shown beside our lines rather
                  than instead of them: if the two disagree, a human needs to see
                  both figures, not a reconciled one. */}
              <p>
                Platform states{" "}
                <span className="money">
                  {doc.stated_total_paise === null
                    ? "no total"
                    : formatPaiseAsRupees(doc.stated_total_paise)}
                </span>
              </p>
              <table>
                <thead>
                  <tr>
                    <th>Line</th>
                    <th>Item as the platform named it</th>
                    <th>Qty</th>
                    <th>Platform price</th>
                    <th>Matched to this menu</th>
                  </tr>
                </thead>
                <tbody>
                  {doc.lines.map((line) => (
                    <tr key={line.id}>
                      <td>{line.line_number}</td>
                      <td>
                        {line.external_item_name} ({line.external_item_id})
                      </td>
                      <td>{line.quantity}</td>
                      <td className="money">
                        {line.stated_unit_price_paise === null
                          ? "—"
                          : formatPaiseAsRupees(line.stated_unit_price_paise)}
                      </td>
                      {/* The matched item's NAME, never its UUID — the
                          previous version of this cell showed the raw id on
                          every matched line, which is exactly backwards from
                          what a human at this screen needs to read. */}
                      <td>{line.menu_item_id !== null ? menuItemName(line.menu_item_id) : "not matched"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {unmatched > 0 && (
                <p className="aggregator-order-warning">
                  {unmatched} of {doc.lines.length} line{doc.lines.length === 1 ? "" : "s"} match no
                  item on this menu and cannot go on a bill. Accepting carries the rest.
                </p>
              )}
              <button
                type="button"
                disabled={accepting === doc.id}
                onClick={() => void onAccept(doc)}
              >
                {accepting === doc.id ? "Accepting…" : "Accept and start the bill"}
              </button>
            </li>
          );
        })}
      </ul>
    </main>
  );
}
