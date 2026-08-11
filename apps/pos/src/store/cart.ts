import { create } from "zustand";
import type { MenuItem, OrderStatus, OrderType } from "@holler/contracts";
import type { CartLine } from "../domain/cart";
import { menuItemNameResolver, orderToCartLines, isRecoverableDraft } from "../domain/cartSync";
import {
  addOrderItem,
  createOrder,
  getActiveDraftOrder,
  isTauriCommandError,
  removeOrderItem,
  updateOrderShape,
} from "../lib/tauri";
import type { NewOrderItemRequest } from "../lib/tauri";

// docs/backlog-m2.md "POS cart persistence" (reopened 2026-08-10): the
// cashier's in-progress work must live in SQLite as it happens, not in
// browser memory until Send. Every mutation below writes through to the
// already-persisted DRAFT order via the real Tauri commands
// (`create_order` for the first line, `add_order_item`/`remove_order_item`
// after), and always replaces `lines` from what that write returned — never
// an optimistic local bump — so the cart can never show a line SQLite does
// not have (task requirement: "the screen lying about what is durable" is
// the same class of bug as the crash itself).
//
// docs/retro.md P0 regression (task T14): `orderType`/`tableId` used to be
// legal to change only before the first line landed, which combined with
// "a DRAFT order is created on the first line" to permanently lock every
// order's shape at whatever it defaulted to. The order's shape is now
// editable for its whole DRAFT lifetime — `setOrderType`/`setTableId` write
// through `update_order_shape` exactly like `addItem`/`removeItem` write
// through their commands, and `orderStatus` (not merely "does an order
// exist") is what gates whether that is currently legal.

export interface NewCartItemInput {
  menuItemId: string;
  variantId: string | null;
  unitPricePaise: number;
  quantity: number;
  notes: string | null;
}

interface CartState {
  /** The persisted DRAFT order this cart mirrors, or `null` before the
   * first line has landed. */
  orderId: string | null;
  /** The mirrored order's status, or `null` before the first line has
   * landed. Shape edits (`setOrderType`/`setTableId`) are legal exactly
   * when this is `null` (no order yet) or `"DRAFT"` — never derived from
   * `orderId` alone, since that stays non-null for this order's entire
   * life on this screen. */
  orderStatus: OrderStatus | null;
  orderType: OrderType;
  tableId: string | null;
  lines: CartLine[];
  /** Whether startup recovery (`hydrate`) has run. Gates rendering the POS
   * screen's cart so it never flashes empty and then repopulates. */
  hydrated: boolean;
  /** True while a write to the edge is in flight — callers use this to
   * disable cart-mutating controls rather than letting two writes race. */
  pending: boolean;
  /** The last write's failure, cashier-visible (never swallowed). Cleared
   * at the start of the next attempt. */
  error: string | null;

  /** Legal whenever the mirrored order is still DRAFT (or does not exist
   * yet). Persists through `update_order_shape` once an order exists —
   * never just an in-memory bump, so the cart can never claim a shape
   * SQLite does not actually have. A no-op once the order has left DRAFT;
   * the UI is expected to also disable the controls that call this via
   * `orderStatus`, but the store enforces it independently rather than
   * trusting the UI alone. */
  setOrderType: (orderType: OrderType) => Promise<void>;
  setTableId: (tableId: string | null) => Promise<void>;

  hydrate: (menuItems: readonly MenuItem[]) => Promise<void>;
  addItem: (input: NewCartItemInput, menuItems: readonly MenuItem[]) => Promise<void>;
  removeItem: (orderItemId: string, menuItems: readonly MenuItem[]) => Promise<void>;
  /** The order is already fully persisted (every line landed as it was
   * added) — "Send" hands it off to the Orders screen's confirm/kitchen
   * flow rather than creating anything new. This just resets the active
   * cart for the next order. */
  clearAfterHandoff: () => void;
}

function toItemRequest(input: NewCartItemInput): NewOrderItemRequest {
  return {
    menu_item_id: input.menuItemId,
    variant_id: input.variantId,
    quantity: input.quantity,
    unit_price_paise: input.unitPricePaise,
    notes: input.notes,
  };
}

function errorMessage(err: unknown): string {
  if (isTauriCommandError(err)) return err.message;
  return "Could not save that change. Please try again.";
}

export const useCartStore = create<CartState>((set, get) => ({
  orderId: null,
  orderStatus: null,
  orderType: "DINE_IN",
  tableId: null,
  lines: [],
  hydrated: false,
  pending: false,
  error: null,

  setOrderType: async (orderType) => {
    const { orderId, orderStatus, tableId } = get();
    if (orderId === null) {
      // Nothing persisted yet — there is no order row to write through to.
      set({ orderType });
      return;
    }
    if (orderStatus !== "DRAFT") return;
    set({ pending: true, error: null });
    try {
      const order = await updateOrderShape(orderId, orderType, tableId);
      set({
        orderType: order.order_type,
        tableId: order.table_id,
        orderStatus: order.status,
      });
    } catch (err) {
      set({ error: errorMessage(err) });
    } finally {
      set({ pending: false });
    }
  },

  setTableId: async (tableId) => {
    const { orderId, orderStatus, orderType } = get();
    if (orderId === null) {
      set({ tableId });
      return;
    }
    if (orderStatus !== "DRAFT") return;
    set({ pending: true, error: null });
    try {
      const order = await updateOrderShape(orderId, orderType, tableId);
      set({
        orderType: order.order_type,
        tableId: order.table_id,
        orderStatus: order.status,
      });
    } catch (err) {
      set({ error: errorMessage(err) });
    } finally {
      set({ pending: false });
    }
  },

  hydrate: async (menuItems) => {
    if (get().hydrated) return;
    set({ pending: true });
    try {
      const order = await getActiveDraftOrder();
      if (order && isRecoverableDraft(order)) {
        set({
          orderId: order.holler_order_id,
          orderStatus: order.status,
          orderType: order.order_type,
          tableId: order.table_id,
          lines: orderToCartLines(order, menuItemNameResolver(menuItems)),
        });
      }
    } catch (err) {
      // A failed recovery read must be visible, not silently treated as
      // "nothing to recover" — that would be exactly the loss this exists
      // to prevent, just relocated to startup.
      set({ error: errorMessage(err) });
    } finally {
      set({ hydrated: true, pending: false });
    }
  },

  addItem: async (input, menuItems) => {
    set({ pending: true, error: null });
    try {
      const { orderId, orderType, tableId } = get();
      const item = toItemRequest(input);
      const order =
        orderId === null
          ? await createOrder(orderType, tableId, [item])
          : await addOrderItem(orderId, item);
      set({
        orderId: order.holler_order_id,
        orderStatus: order.status,
        orderType: order.order_type,
        tableId: order.table_id,
        lines: orderToCartLines(order, menuItemNameResolver(menuItems)),
      });
    } catch (err) {
      set({ error: errorMessage(err) });
    } finally {
      set({ pending: false });
    }
  },

  removeItem: async (orderItemId, menuItems) => {
    const { orderId } = get();
    if (orderId === null) return;
    set({ pending: true, error: null });
    try {
      const order = await removeOrderItem(orderId, orderItemId);
      set({ lines: orderToCartLines(order, menuItemNameResolver(menuItems)) });
    } catch (err) {
      set({ error: errorMessage(err) });
    } finally {
      set({ pending: false });
    }
  },

  clearAfterHandoff: () =>
    set({ orderId: null, orderStatus: null, lines: [], tableId: null, error: null }),
}));
