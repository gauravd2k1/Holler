import { Fragment, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import type { Kot, KotStatus } from "@holler/contracts";
import {
  useKotsForOrderQuery,
  useKotStatusTransitionsQuery,
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
  kotStatusToneClass,
  kotTransitionActionLabel,
  buildKotTransitionTable,
  legalNextKotStatuses,
  orderStatusLabel,
  stationsForKots,
} from "../domain/kitchen";
import { useAuthStore } from "../store/auth";
import { formatIST } from "../lib/datetime";
import { markBillTapped } from "../lib/perf";
import { PrintFailureBanner } from "./PrintFailureBanner";
import { SyncBlockedBanner } from "./SyncBlockedBanner";
import { LowStockBanner } from "./LowStockBanner";
import { OutletName } from "./OutletName";

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
      // Confirm is the moment stock moves: deduction runs INSIDE
      // confirm_order's transaction (edge/database/src/lib.rs, the single
      // call site of deduct_stock_for_confirmed_order), so by the time this
      // resolves the ledger rows are committed and every stock read is
      // stale. Nothing invalidated them before 2026-08-27, so a cashier who
      // checked stock, sold, and checked again saw no change — and a number
      // that does not move after a sale is a number that stops being
      // trusted. The gaps report moves for the same reason: a line with no
      // recipe lands there at confirm too.
      await queryClient.invalidateQueries({ queryKey: queryKeys.currentStock });
      await queryClient.invalidateQueries({ queryKey: queryKeys.stockDeductionGaps });
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
      <SyncBlockedBanner />
      <LowStockBanner />
      <header className="holler-header">
        <div className="holler-header__brand">
          <img src="/holler_no_bg.png" alt="Holler" className="holler-logo" />
          <OutletName />
          <h1>Orders</h1>
        </div>
        <div className="holler-header__spacer" />
        <button type="button" className="btn" onClick={() => void navigate({ to: "/" })}>
          Back to POS
        </button>
      </header>
      <div className="screen-body">
      {ordersQuery.isLoading && <p>Loading orders…</p>}
      {ordersQuery.isError && <p role="alert">Could not load orders.</p>}
      {/* Loading and error had words; zero orders rendered a table with a
          header row and an empty body. This is the FIRST screen after every
          clean reset, so an empty table with no explanation is what a new
          shift — and a demo — opens on. */}
      {!ordersQuery.isLoading && !ordersQuery.isError && (ordersQuery.data ?? []).length === 0 && (
        <p className="screen-empty">No orders yet. Start one on the POS screen.</p>
      )}
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
      {/* The table renders only when it has rows: a header row over an
          empty body reads as a broken screen, and the empty state above
          already says what is going on. */}
      {(ordersQuery.data ?? []).length > 0 && (
      <div className="card">
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
            // Keyed Fragment, not the `<>` shorthand — the shorthand cannot
            // carry a key at all, which produced a real "each child in a
            // list should have a unique key" console warning (found in a
            // real-browser pass, T7; pre-existing, not introduced by the
            // cell-content changes in this same file).
            <Fragment key={order.holler_order_id}>
              <tr key={order.holler_order_id}>
                {/* Human-facing display number only — never the row's UUID
                    (CLAUDE.md §Money/time/identifiers). display_number is
                    nullable only for pre-0.4.0 legacy rows; "unnumbered" is
                    shown rather than falling back to the id.

                    RENDERED AS STORED. The `#` is part of the minted value
                    (`format_order_display_number`, edge/database/src/repo.rs
                    returns "#A184"), so adding one here produced "##A184" on
                    screen. The admin console and the captain page print the
                    column raw for that reason. */}
                <td>{order.display_number !== null ? order.display_number : "unnumbered"}</td>
                {/* NOT the raw enum. DINE_IN on a screen is a variable name;
                    the status column beside it has read as plain language
                    since M2 and this column was still shouting. Same helper,
                    so the two cannot drift apart. */}
                <td>{orderStatusLabel(order.order_type)}</td>
                {/* Never colour-only (docs/spec/kitchen.md §KDS, applies
                    wherever status is rendered): plain-language text, not a
                    coloured dot. */}
                <td>{orderStatusLabel(order.status)}</td>
                <td>{order.items.length}</td>
                <td className="money">{formatPaiseAsRupees(order.total_paise)}</td>
                <td>{formatIST(order.timestamps.created_at)}</td>
                <td className="order-actions">
                  {canOfferConfirm(order.status, principal) && (
                    <button
                      type="button"
                      className="btn btn--primary"
                      disabled={confirmingId === order.holler_order_id}
                      onClick={() => void handleConfirm(order.holler_order_id)}
                    >
                      {confirmingId === order.holler_order_id ? "Confirming…" : "Confirm"}
                    </button>
                  )}
                  {canOfferSendToKitchen(order.status, principal) && (
                    <button
                      type="button"
                      className="btn btn--primary"
                      disabled={sendingId === order.holler_order_id}
                      onClick={() => void handleSendToKitchen(order.holler_order_id)}
                    >
                      {sendingId === order.holler_order_id ? "Sending…" : "Send to Kitchen"}
                    </button>
                  )}
                  {order.status !== "DRAFT" && (
                    <button
                      type="button"
                      className="btn"
                      onClick={() =>
                        setExpandedOrderId(
                          expandedOrderId === order.holler_order_id ? null : order.holler_order_id,
                        )
                      }
                    >
                      {expandedOrderId === order.holler_order_id ? "Hide Kitchen" : "Kitchen"}
                    </button>
                  )}
                  {order.status !== "DRAFT" && order.status !== "CANCELLED" && (
                    <button
                      type="button"
                      className="btn"
                      onClick={() => {
                        // Perf marker, the NEAR end of "till tap -> bill
                        // open". Stamped on the tap itself rather than after
                        // navigation, because the interval starts when the
                        // cashier's finger lands, not when the router agrees
                        // to move. Far end: `till_bill_screen_opened` in
                        // BillingScreen, which computes and REPORTS the
                        // difference -- this line no longer has to be paired
                        // with that one by hand (lib/perf.ts).
                        markBillTapped(order.holler_order_id);
                        console.log(
                          `HOLLER-PERF ts=${new Date().toISOString()} event=till_bill_tapped id=${order.holler_order_id}`,
                        );
                        void navigate({
                          to: "/orders/$orderId/billing",
                          params: { orderId: order.holler_order_id },
                        });
                      }}
                    >
                      Bill
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
            </Fragment>
          ))}
        </tbody>
      </table>
      </div>
      )}
      </div>
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
  const transitionsQuery = useKotStatusTransitionsQuery();
  const [transitioningId, setTransitioningId] = useState<string | null>(null);
  const [transitionError, setTransitionError] = useState<string | null>(null);

  const kots = kotsQuery.data ?? [];
  const stations = stationsForKots(kots);
  const transitionTable = transitionsQuery.data
    ? buildKotTransitionTable(transitionsQuery.data)
    : undefined;

  // D14 PART 1 LIVES IN `KitchenChangedListener` NOW, MOUNTED IN `App`.
  //
  // It was here, and being here was the defect: React mounts this panel only
  // while its order's Kitchen view is expanded, so with every panel collapsed
  // — the normal state — nothing in the process was subscribed and the
  // `holler://kitchen-changed` event was emitted into nothing. Moving it up
  // fixes the order list too, which is the same defect wearing a second bug
  // report (`docs/m7-b2-sinks.md`).
  //
  // Nothing replaces it here. A second subscription at panel level would be
  // two listeners invalidating the same keys.

  async function handleTransition(kot: Kot, newStatus: KotStatus) {
    if (!canOfferKotTransition(principal)) return;
    setTransitioningId(kot.id);
    setTransitionError(null);
    try {
      await transitionKotStatus(orderId, kot.id, newStatus);
      await queryClient.invalidateQueries({ queryKey: queryKeys.kots(orderId) });
      await queryClient.invalidateQueries({ queryKey: queryKeys.orders });
    } catch (err) {
      // D14 PART 2: A REFUSED MOVE MEANS THIS VIEW IS STALE, SO RE-READ
      // BEFORE SAYING ANYTHING.
      //
      // The edge refuses a transition it considers illegal from the status it
      // holds. If this screen offered that move, this screen's idea of the
      // status is wrong — which is the whole of VV-009. Re-fetching first
      // means the operator reads the error beside the CORRECTED row and the
      // button that caused it is already gone.
      //
      // STRUCTURAL, AND IT COVERS THE CASE WHERE PART 1 FAILS. If the live
      // update never arrives — the hub thread died, the event name drifted,
      // the window was not ready — this path still repairs the view on the
      // first press. Two independent mechanisms for one guarantee, the weaker
      // of which needs no infrastructure at all.
      await queryClient.invalidateQueries({ queryKey: queryKeys.kots(orderId) });
      await queryClient.refetchQueries({ queryKey: queryKeys.kots(orderId) });
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
              {/* Ticket sequence only — never a truncated UUID. A partial
                  UUID is still a UUID, and the defect class this avoids is
                  named in `edge/printer/src/template.rs`. */}
              <td>#{kot.sequence}</td>
              <td>{stationNameByCode.get(kot.station) ?? kot.station}</td>
              {/* Colour is ADDED to the words, never substituted for them
                  (docs/spec/kitchen.md §KDS: never colour-only, always show
                  time/status too). The badge carries the label; the Updated
                  column beside it grounds the claim in a time. */}
              <td>
                <span className={kotStatusToneClass(kot.status)}>
                  {kotStatusLabel(kot.status)}
                </span>
              </td>
              <td>
                {kot.items.map((i) => `${i.quantity}x ${i.name}`).join(", ")}
              </td>
              <td>{formatIST(kot.updated_at)}</td>
              <td>
                {/* D14 PART 3: VERBS, AND ONLY THE MOVES THE EDGE ALLOWS.
                    The list comes from `list_kot_status_transitions`, which
                    reads the edge's own table — this screen no longer keeps a
                    copy to drift from. While that query is still loading the
                    table is undefined and NO buttons render, because a button
                    offered on a guess is what VV-009 caught. */}
                {canOfferKotTransition(principal) &&
                  legalNextKotStatuses(transitionTable, kot.status).map((next) => (
                    <button
                      key={next}
                      type="button"
                      className="btn"
                      disabled={transitioningId === kot.id}
                      onClick={() => void handleTransition(kot, next)}
                    >
                      {transitioningId === kot.id ? "…" : kotTransitionActionLabel(next)}
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
