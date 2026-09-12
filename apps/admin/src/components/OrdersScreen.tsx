import { useQuery } from "@tanstack/react-query";
import { listOrders } from "../lib/api";
import { formatPaiseAsRupees } from "../lib/money";
import { formatIST } from "../lib/datetime";

/**
 * The order list.
 *
 * THIS SCREEN SHOWS THE CLOUD'S REPLICA AND SAYS SO, for the same reason
 * `GoodsReceiptsScreen` does. An order is EDGE-AUTHORITATIVE (§50.1): the till
 * takes it, the cloud replays it, and an order rung while the uplink was down
 * is real, complete and correct at the outlet while being absent here. Calling
 * this "the orders" would invite an operator to read a sync delay as a day with
 * no trade.
 *
 * NO UUID REACHES THE SCREEN. `display_number` is what a human uses ("Order
 * #A184"); an order that somehow carries none is shown as "no number" rather
 * than as its id, the same withholding `SuppliersScreen` and
 * `GoodsReceiptsScreen` already do.
 *
 * THE ITEMS COLUMN COUNTS LINES, IT DOES NOT NAME THEM, because `OrderItem`
 * carries `menu_item_id` and NO `name` -- the third surface hit by the same
 * missing contract shape as `GoodsReceiptLineReadSchema` and
 * `SupplierItemSchema`. A name could be had by joining the menu, but that
 * would show TODAY'S name for an order taken last week, and the whole point of
 * `unit_price_paise` being a snapshot is that a line is what it was when it
 * was sold. Reported, not worked around.
 *
 * NOT A REPORTING SURFACE. There is no revenue total, no grouping and no date
 * filter: reporting depth is M7, and a sum computed over "whatever has replayed
 * so far" would be a number an operator could act on and should not.
 */
export function OrdersScreen() {
  const orders = useQuery({ queryKey: ["orders"], queryFn: listOrders });

  if (orders.isLoading) return <p>Loading orders…</p>;
  if (orders.isError) {
    return <p className="error">Could not load orders: {String(orders.error)}</p>;
  }

  const rows = orders.data ?? [];

  return (
    <section>
      <h1>Orders</h1>
      <p className="replica-note" role="note">
        This is the cloud's copy of what the outlet has sent. An order taken while
        the till was offline appears here once it syncs — until then the till's own
        list is the complete one.
      </p>

      {rows.length === 0 ? (
        <p>No orders have reached the cloud for this outlet yet.</p>
      ) : (
        <table className="data-table">
          <thead>
            <tr>
              <th scope="col">Order</th>
              <th scope="col">Taken</th>
              <th scope="col">Type</th>
              <th scope="col">Status</th>
              <th scope="col">Items</th>
              <th scope="col">Payment</th>
              <th scope="col" className="numeric">
                Total
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((order) => (
              <tr key={order.holler_order_id}>
                <td>{order.display_number ?? "no number"}</td>
                <td>{formatIST(order.timestamps.created_at)}</td>
                <td>{humanise(order.order_type)}</td>
                <td>{humanise(order.status)}</td>
                <td>
                  {order.items.length === 0 ? (
                    // An order with a total and no lines is what a PARTLY
                    // replayed order looks like from here: the parent landed
                    // and its item events did not. Naming that beats an empty
                    // cell, which reads as a rendering fault.
                    <span className="muted">no lines synced</span>
                  ) : (
                    summarise(order.items)
                  )}
                </td>
                <td>{humanise(order.payment_status)}</td>
                <td className="numeric money">{formatPaiseAsRupees(order.total_paise)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

/** DINE_IN -> Dine in. Contract values are SCREAMING_SNAKE; bills are not. */
function humanise(value: string): string {
  const words = value.toLowerCase().replace(/_/g, " ");
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/**
 * "3 lines, 5 items". NOT the dish names: `OrderItem` has no `name` field, and
 * joining the live menu for one would print today's name against a line sold
 * under the old one.
 */
function summarise(items: { quantity: number }[]): string {
  const units = items.reduce((total, item) => total + item.quantity, 0);
  const lines = `${items.length} ${items.length === 1 ? "line" : "lines"}`;
  return `${lines}, ${units} ${units === 1 ? "item" : "items"}`;
}
