import { useNavigate } from "@tanstack/react-router";
import { useOrdersQuery } from "../lib/queries";
import { formatPaiseAsRupees } from "../domain/money";

// The only reporting permitted in Milestone 1 (CLAUDE.md EXCLUDES: "reporting
// beyond a basic order list"). No filtering, totals-by-day, or exports.
export function OrderListScreen() {
  const navigate = useNavigate();
  const ordersQuery = useOrdersQuery();

  return (
    <main className="order-list-screen">
      <header>
        <h1>Orders</h1>
        <button type="button" onClick={() => void navigate({ to: "/" })}>
          Back to POS
        </button>
      </header>
      {ordersQuery.isLoading && <p>Loading orders…</p>}
      {ordersQuery.isError && <p role="alert">Could not load orders.</p>}
      <table>
        <thead>
          <tr>
            <th>Order</th>
            <th>Type</th>
            <th>Status</th>
            <th>Items</th>
            <th>Total</th>
            <th>Created</th>
          </tr>
        </thead>
        <tbody>
          {(ordersQuery.data ?? []).map((order) => (
            <tr key={order.holler_order_id}>
              <td>{order.holler_order_id}</td>
              <td>{order.order_type}</td>
              <td>{order.status}</td>
              <td>{order.items.length}</td>
              <td>{formatPaiseAsRupees(order.total_paise)}</td>
              <td>{order.timestamps.created_at}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </main>
  );
}
