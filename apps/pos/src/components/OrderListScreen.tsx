import { useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { useOrdersQuery, queryKeys } from "../lib/queries";
import { formatPaiseAsRupees } from "../domain/money";
import { confirmOrder } from "../lib/tauri";
import { hasPermission } from "../domain/permissions";
import { canOfferConfirm, confirmErrorMessage } from "../domain/orderActions";
import { useAuthStore } from "../store/auth";

// The only reporting permitted in Milestone 1 (CLAUDE.md EXCLUDES: "reporting
// beyond a basic order list"). No filtering, totals-by-day, or exports.
export function OrderListScreen() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const principal = useAuthStore((s) => s.principal);
  const ordersQuery = useOrdersQuery();

  const canModifyOrder = hasPermission(principal, "order.modify");

  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const [confirmError, setConfirmError] = useState<string | null>(null);

  async function handleConfirm(orderId: string) {
    // Permission is enforced here, not just visually: an unauthorized
    // cashier cannot reach `confirmOrder` at all.
    if (!canModifyOrder) return;
    setConfirmingId(orderId);
    setConfirmError(null);
    try {
      await confirmOrder(orderId);
      // Refetch from the edge rather than optimistically flipping local
      // state — a failed confirm must leave the displayed order matching
      // its actual status, and a successful one should reflect exactly
      // what the edge persisted (including `confirmed_at`).
      await queryClient.invalidateQueries({ queryKey: queryKeys.orders });
    } catch (err) {
      setConfirmError(confirmErrorMessage(err));
    } finally {
      setConfirmingId(null);
    }
  }

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
      {confirmError && (
        <p className="order-confirm-error" role="alert">
          {confirmError}
        </p>
      )}
      <table>
        <thead>
          <tr>
            <th>Order</th>
            <th>Type</th>
            <th>Status</th>
            <th>Items</th>
            <th>Total</th>
            <th>Created</th>
            <th>Actions</th>
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
              <td>
                {canOfferConfirm(order.status, principal) && (
                  <button
                    type="button"
                    disabled={confirmingId === order.holler_order_id}
                    onClick={() => void handleConfirm(order.holler_order_id)}
                  >
                    {confirmingId === order.holler_order_id ? "Confirming…" : "Confirm"}
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </main>
  );
}
