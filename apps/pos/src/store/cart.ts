import { create } from "zustand";
import type { MenuItem, OrderType } from "@holler/contracts";
import type { CartLine } from "../domain/cart";
import { menuItemNameResolver, orderToCartLines, isRecoverableDraft } from "../domain/cartSync";
import { addOrderItem, createOrder, getActiveDraftOrder, isTauriCommandError, removeOrderItem } from "../lib/tauri";
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

  /** Legal only before the first line lands — once a DRAFT order exists its
   * `order_type`/`table_id` are fixed for its lifetime (no
   * update-order-type command exists, and letting the UI pretend otherwise
   * would show a value SQLite does not have). */
  setOrderType: (orderType: OrderType) => void;
  setTableId: (tableId: string | null) => void;

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
  orderType: "DINE_IN",
  tableId: null,
  lines: [],
  hydrated: false,
  pending: false,
  error: null,

  setOrderType: (orderType) =>
    set((state) => (state.orderId === null ? { orderType } : state)),
  setTableId: (tableId) => set((state) => (state.orderId === null ? { tableId } : state)),

  hydrate: async (menuItems) => {
    if (get().hydrated) return;
    set({ pending: true });
    try {
      const order = await getActiveDraftOrder();
      if (order && isRecoverableDraft(order)) {
        set({
          orderId: order.holler_order_id,
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

  clearAfterHandoff: () => set({ orderId: null, lines: [], tableId: null, error: null }),
}));
