import { useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import type { Kot, KotStatus } from "@holler/contracts";
import {
  useKotsForOrderQuery,
  useOrdersQuery,
  useStationsQuery,
  queryKeys,
} from "../lib/queries";
import { formatPaiseAsRupees } from "../domain/money";
import { confirmOrder, sendOrderToKitchen, transitionKotStatus } from "../lib/tauri";
import { hasPermission } from "../domain/permissions";
import { canOfferConfirm, confirmErrorMessage } from "../domain/orderActions";
import {
  canOfferSendToKitchen,
  canOfferKotTransition,
  kitchenErrorMessage,
  kotStatusLabel,
  legalNextKotStatuses,
  orderStatusLabel,
  stationsForKots,
} from "../domain/kitchen";
import { useAuthStore } from "../store/auth";
import { PrintFailureBanner } from "./PrintFailureBanner";

// The only reporting permitted in Milestone 1 (CLAUDE.md EXCLUDES: "reporting
// beyond a basic order list"). No filtering, totals-by-day, or exports.
// Milestone 2 adds send-to-kitchen and each order's KOT/status display
// in-place — still a list, not a new reporting surface.
export function OrderListScreen() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const principal = useAuthStore((s) => s.principal);
  const ordersQuery = useOrdersQuery();
  const stationsQuery = useStationsQuery();

  const canModifyOrder = hasPermission(principal, "order.modify");

  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const [confirmError, setConfirmError] = useState<string | null>(null);
  const [sendingId, setSendingId] = useState<string | null>(null);
  const [sendError, setSendError] = useState<string | null>(null);
  const [expandedOrderId, setExpandedOrderId] = useState<string | null>(null);

  const stationNameByCode = new Map(
    (stationsQuery.data ?? []).map((s) => [s.code, s.name] as const),
  );

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

  async function handleSendToKitchen(orderId: string) {
    // Permission is enforced here, not just visually — the button is only
    // rendered when `canOfferSendToKitchen` is true, and this is the second
    // gate before the command is actually issued. The edge independently
    // re-checks the order's real status regardless (sync.md §50.1).
    if (!hasPermission(principal, "order.modify")) return;
    setSendingId(orderId);
    setSendError(null);
    try {
      await sendOrderToKitchen(orderId);
      await queryClient.invalidateQueries({ queryKey: queryKeys.orders });
      await queryClient.invalidateQueries({ queryKey: queryKeys.kots(orderId) });
      setExpandedOrderId(orderId);
    } catch (err) {
      setSendError(kitchenErrorMessage(err));
    } finally {
      setSendingId(null);
    }
  }

  return (
    <main className="order-list-screen">
      <PrintFailureBanner />
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
      {sendError && (
        <p className="order-confirm-error" role="alert">
          {sendError}
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
            <>
              <tr key={order.holler_order_id}>
                <td>{order.holler_order_id}</td>
                <td>{order.order_type}</td>
                {/* Never colour-only (docs/spec/kitchen.md §KDS, applies
                    wherever status is rendered): plain-language text, not a
                    coloured dot. */}
                <td>{orderStatusLabel(order.status)}</td>
                <td>{order.items.length}</td>
                <td>{formatPaiseAsRupees(order.total_paise)}</td>
                <td>{order.timestamps.created_at}</td>
                <td className="order-actions">
                  {canOfferConfirm(order.status, principal) && (
                    <button
                      type="button"
                      disabled={confirmingId === order.holler_order_id}
                      onClick={() => void handleConfirm(order.holler_order_id)}
                    >
                      {confirmingId === order.holler_order_id ? "Confirming…" : "Confirm"}
                    </button>
                  )}
                  {canOfferSendToKitchen(order.status, principal) && (
                    <button
                      type="button"
                      disabled={sendingId === order.holler_order_id}
                      onClick={() => void handleSendToKitchen(order.holler_order_id)}
                    >
                      {sendingId === order.holler_order_id ? "Sending…" : "Send to Kitchen"}
                    </button>
                  )}
                  {order.status !== "DRAFT" && (
                    <button
                      type="button"
                      onClick={() =>
                        setExpandedOrderId(
                          expandedOrderId === order.holler_order_id ? null : order.holler_order_id,
                        )
                      }
                    >
                      {expandedOrderId === order.holler_order_id ? "Hide Kitchen" : "Kitchen"}
                    </button>
                  )}
                </td>
              </tr>
              {expandedOrderId === order.holler_order_id && (
                <tr key={`${order.holler_order_id}-kots`}>
                  <td colSpan={7}>
                    <KotsPanel
                      orderId={order.holler_order_id}
                      principal={principal}
                      stationNameByCode={stationNameByCode}
                    />
                  </td>
                </tr>
              )}
            </>
          ))}
        </tbody>
      </table>
    </main>
  );
}

function KotsPanel({
  orderId,
  principal,
  stationNameByCode,
}: {
  orderId: string;
  principal: ReturnType<typeof useAuthStore.getState>["principal"];
  stationNameByCode: Map<string, string>;
}) {
  const queryClient = useQueryClient();
  const kotsQuery = useKotsForOrderQuery(orderId);
  const [transitioningId, setTransitioningId] = useState<string | null>(null);
  const [transitionError, setTransitionError] = useState<string | null>(null);

  const kots = kotsQuery.data ?? [];
  const stations = stationsForKots(kots);

  async function handleTransition(kot: Kot, newStatus: KotStatus) {
    if (!canOfferKotTransition(principal)) return;
    setTransitioningId(kot.id);
    setTransitionError(null);
    try {
      await transitionKotStatus(orderId, kot.id, newStatus);
      await queryClient.invalidateQueries({ queryKey: queryKeys.kots(orderId) });
      await queryClient.invalidateQueries({ queryKey: queryKeys.orders });
    } catch (err) {
      setTransitionError(kitchenErrorMessage(err));
    } finally {
      setTransitioningId(null);
    }
  }

  if (kotsQuery.isLoading) return <p>Loading tickets…</p>;
  if (kots.length === 0) return <p>No kitchen tickets yet for this order.</p>;

  return (
    <div className="kots-panel">
      <p className="kots-stations">
        Routed to: {stations.map((code) => stationNameByCode.get(code) ?? code).join(", ")}
      </p>
      {transitionError && (
        <p className="order-confirm-error" role="alert">
          {transitionError}
        </p>
      )}
      <table className="kots-table">
        <thead>
          <tr>
            <th>Ticket</th>
            <th>Station</th>
            <th>Status</th>
            <th>Items</th>
            <th>Updated</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          {kots.map((kot) => (
            <tr key={kot.id}>
              <td>
                #{kot.sequence} · {kot.id.slice(0, 8)}
              </td>
              <td>{stationNameByCode.get(kot.station) ?? kot.station}</td>
              {/* Non-colour-only status: plain text label plus the
                  timestamp that grounds it (docs/spec/kitchen.md §KDS). */}
              <td>{kotStatusLabel(kot.status)}</td>
              <td>
                {kot.items.map((i) => `${i.quantity}x ${i.name}`).join(", ")}
              </td>
              <td>{kot.updated_at}</td>
              <td>
                {canOfferKotTransition(principal) &&
                  legalNextKotStatuses(kot.status).map((next) => (
                    <button
                      key={next}
                      type="button"
                      disabled={transitioningId === kot.id}
                      onClick={() => void handleTransition(kot, next)}
                    >
                      {transitioningId === kot.id ? "…" : kotStatusLabel(next)}
                    </button>
                  ))}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
