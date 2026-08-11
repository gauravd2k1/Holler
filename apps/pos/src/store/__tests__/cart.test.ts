import { describe, expect, it, vi, beforeEach } from "vitest";
import type { CanonicalOrder, MenuItem } from "@holler/contracts";

// Mocks the Tauri boundary itself, not the store's logic — proves the store
// converges on whatever `lib/tauri` (i.e. SQLite, via the real edge crate)
// reports, never on an optimistic local guess.
const getActiveDraftOrderMock = vi.fn();
const createOrderMock = vi.fn();
const addOrderItemMock = vi.fn();
const removeOrderItemMock = vi.fn();
const updateOrderShapeMock = vi.fn();

vi.mock("../../lib/tauri", async () => {
  const actual = await vi.importActual<typeof import("../../lib/tauri")>("../../lib/tauri");
  return {
    ...actual,
    getActiveDraftOrder: (...args: unknown[]) => getActiveDraftOrderMock(...args),
    createOrder: (...args: unknown[]) => createOrderMock(...args),
    addOrderItem: (...args: unknown[]) => addOrderItemMock(...args),
    removeOrderItem: (...args: unknown[]) => removeOrderItemMock(...args),
    updateOrderShape: (...args: unknown[]) => updateOrderShapeMock(...args),
  };
});

const { useCartStore } = await import("../cart");

const MENU_ITEMS: MenuItem[] = [
  {
    id: "item-1",
    outlet_id: "outlet-1",
    category_id: "cat-1",
    name: "Paneer Tikka",
    base_price_paise: 25000,
    is_available: true,
    config_version: 1,
    schema_version: 1,
  },
];

function persistedOrder(items: CanonicalOrder["items"]): CanonicalOrder {
  return {
    holler_order_id: "order-1",
    external_order_id: null,
    source: "POS",
    outlet_id: "outlet-1",
    order_type: "DINE_IN",
    status: "DRAFT",
    table_id: "table-1",
    customer: null,
    delivery_address: null,
    items,
    subtotal_paise: items.reduce((sum, i) => sum + i.line_total_paise, 0),
    discount_paise: 0,
    packaging_paise: 0,
    delivery_charge_paise: 0,
    taxes_paise: 0,
    aggregator_discount_paise: 0,
    merchant_discount_paise: 0,
    total_paise: items.reduce((sum, i) => sum + i.line_total_paise, 0),
    payment_status: "UNPAID",
    payment_source: null,
    preparation_time_minutes: null,
    rider: null,
    timestamps: {
      created_at: "2026-08-10T10:00:00.000Z",
      confirmed_at: null,
      updated_at: "2026-08-10T10:00:00.000Z",
    },
    source_payload: null,
    schema_version: 1,
  };
}

beforeEach(() => {
  getActiveDraftOrderMock.mockReset();
  createOrderMock.mockReset();
  addOrderItemMock.mockReset();
  removeOrderItemMock.mockReset();
  updateOrderShapeMock.mockReset();
  useCartStore.setState({
    orderId: null,
    orderStatus: null,
    orderType: "DINE_IN",
    tableId: null,
    lines: [],
    hydrated: false,
    pending: false,
    error: null,
  });
});

describe("cart store — crash recovery (THE TEST THAT MATTERS)", () => {
  it("restores the exact lines, quantities and prices SQLite reports, from a brand-new store with no prior in-memory cart", async () => {
    // What the edge's SQLite already durably holds — built independently of
    // any store/UI state, standing in for "what survived the crash".
    const survivingOrder = persistedOrder([
      {
        id: "oi-1",
        menu_item_id: "item-1",
        variant_id: null,
        quantity: 2,
        unit_price_paise: 25000,
        line_total_paise: 50000,
        modifiers: [],
        notes: null,
      },
      {
        id: "oi-2",
        menu_item_id: "item-1",
        variant_id: null,
        quantity: 1,
        unit_price_paise: 25000,
        line_total_paise: 25000,
        modifiers: [],
        notes: "extra spicy",
      },
    ]);
    getActiveDraftOrderMock.mockResolvedValue(survivingOrder);

    // A brand-new store, as a freshly (re)started app would have — nothing
    // in memory beyond the defaults set in `beforeEach`, i.e. the in-memory
    // cart from "before the crash" is gone.
    expect(useCartStore.getState().lines).toEqual([]);
    expect(useCartStore.getState().orderId).toBeNull();

    await useCartStore.getState().hydrate(MENU_ITEMS);

    const state = useCartStore.getState();
    expect(state.hydrated).toBe(true);
    expect(state.orderId).toBe("order-1");
    expect(state.orderType).toBe("DINE_IN");
    expect(state.tableId).toBe("table-1");
    expect(state.lines).toHaveLength(2);
    expect(state.lines[0]).toMatchObject({
      lineId: "oi-1",
      menuItemName: "Paneer Tikka",
      quantity: 2,
      unitPricePaise: 25000,
    });
    expect(state.lines[1]).toMatchObject({
      lineId: "oi-2",
      quantity: 1,
      notes: "extra spicy",
    });
  });

  it("leaves the cart empty when there is nothing to recover, without treating it as an error", async () => {
    getActiveDraftOrderMock.mockResolvedValue(null);
    await useCartStore.getState().hydrate(MENU_ITEMS);
    const state = useCartStore.getState();
    expect(state.hydrated).toBe(true);
    expect(state.orderId).toBeNull();
    expect(state.lines).toEqual([]);
    expect(state.error).toBeNull();
  });

  it("does not re-fetch on a second hydrate call (idempotent startup recovery)", async () => {
    getActiveDraftOrderMock.mockResolvedValue(null);
    await useCartStore.getState().hydrate(MENU_ITEMS);
    await useCartStore.getState().hydrate(MENU_ITEMS);
    expect(getActiveDraftOrderMock).toHaveBeenCalledTimes(1);
  });
});

describe("cart store — write-through mutations", () => {
  it("addItem creates the DRAFT order on the first line and never touches create_order again", async () => {
    createOrderMock.mockResolvedValue(
      persistedOrder([
        {
          id: "oi-1",
          menu_item_id: "item-1",
          variant_id: null,
          quantity: 1,
          unit_price_paise: 25000,
          line_total_paise: 25000,
          modifiers: [],
          notes: null,
        },
      ]),
    );
    addOrderItemMock.mockResolvedValue(
      persistedOrder([
        {
          id: "oi-1",
          menu_item_id: "item-1",
          variant_id: null,
          quantity: 1,
          unit_price_paise: 25000,
          line_total_paise: 25000,
          modifiers: [],
          notes: null,
        },
        {
          id: "oi-2",
          menu_item_id: "item-1",
          variant_id: null,
          quantity: 1,
          unit_price_paise: 25000,
          line_total_paise: 25000,
          modifiers: [],
          notes: null,
        },
      ]),
    );

    await useCartStore.getState().addItem(
      { menuItemId: "item-1", variantId: null, unitPricePaise: 25000, quantity: 1, notes: null },
      MENU_ITEMS,
    );
    expect(createOrderMock).toHaveBeenCalledTimes(1);
    expect(useCartStore.getState().orderId).toBe("order-1");
    expect(useCartStore.getState().lines).toHaveLength(1);

    await useCartStore.getState().addItem(
      { menuItemId: "item-1", variantId: null, unitPricePaise: 25000, quantity: 1, notes: null },
      MENU_ITEMS,
    );
    expect(createOrderMock).toHaveBeenCalledTimes(1);
    expect(addOrderItemMock).toHaveBeenCalledWith("order-1", expect.objectContaining({ menu_item_id: "item-1" }));
    expect(useCartStore.getState().lines).toHaveLength(2);
  });

  it("a failed write is surfaced as an error and does not fabricate a line", async () => {
    createOrderMock.mockRejectedValue({ code: "UNKNOWN_ERROR", message: "disk full" });
    await useCartStore.getState().addItem(
      { menuItemId: "item-1", variantId: null, unitPricePaise: 25000, quantity: 1, notes: null },
      MENU_ITEMS,
    );
    const state = useCartStore.getState();
    expect(state.orderId).toBeNull();
    expect(state.lines).toEqual([]);
    expect(state.error).toBe("disk full");
  });

  it("removeItem replaces lines from the edge's response, not by filtering locally", async () => {
    useCartStore.setState({
      orderId: "order-1",
      lines: [
        { lineId: "oi-1", menuItemId: "item-1", menuItemName: "Paneer Tikka", variantId: null, unitPricePaise: 25000, quantity: 1, notes: null },
      ],
    });
    removeOrderItemMock.mockResolvedValue(persistedOrder([]));

    await useCartStore.getState().removeItem("oi-1", MENU_ITEMS);
    expect(removeOrderItemMock).toHaveBeenCalledWith("order-1", "oi-1");
    expect(useCartStore.getState().lines).toEqual([]);
  });
});

// docs/retro.md P0 regression (task T14): the order's shape must stay
// editable for the order's whole DRAFT lifetime — these are the regression
// tests, and must fail against the pre-fix behaviour (setOrderType/
// setTableId silently ignored once orderId was set at all).
describe("cart store — order shape stays editable through DRAFT", () => {
  it("persists an order-type change through update_order_shape once a line has landed", async () => {
    useCartStore.setState({
      orderId: "order-1",
      orderStatus: "DRAFT",
      orderType: "DINE_IN",
      tableId: "table-1",
    });
    updateOrderShapeMock.mockResolvedValue({
      ...persistedOrder([]),
      order_type: "TAKEAWAY",
      table_id: "table-1",
      status: "DRAFT",
    });

    await useCartStore.getState().setOrderType("TAKEAWAY");

    expect(updateOrderShapeMock).toHaveBeenCalledWith("order-1", "TAKEAWAY", "table-1");
    expect(useCartStore.getState().orderType).toBe("TAKEAWAY");
  });

  it("persists a table selection through update_order_shape and the order becomes sendable", async () => {
    useCartStore.setState({
      orderId: "order-1",
      orderStatus: "DRAFT",
      orderType: "DINE_IN",
      tableId: null,
      lines: [
        {
          lineId: "oi-1",
          menuItemId: "item-1",
          menuItemName: "Paneer Tikka",
          variantId: null,
          unitPricePaise: 25000,
          quantity: 1,
          notes: null,
        },
      ],
    });
    updateOrderShapeMock.mockResolvedValue({
      ...persistedOrder([]),
      order_type: "DINE_IN",
      table_id: "table-9",
      status: "DRAFT",
    });

    await useCartStore.getState().setTableId("table-9");

    expect(updateOrderShapeMock).toHaveBeenCalledWith("order-1", "DINE_IN", "table-9");
    const state = useCartStore.getState();
    expect(state.tableId).toBe("table-9");
    // The stuck-cashier escape: DINE_IN + a table + a line is sendable.
    expect(state.orderType).toBe("DINE_IN");
    expect(state.lines).toHaveLength(1);
  });

  it("does not call update_order_shape and reports nothing changed once the order has left DRAFT", async () => {
    useCartStore.setState({
      orderId: "order-1",
      orderStatus: "CONFIRMED",
      orderType: "DINE_IN",
      tableId: "table-1",
    });

    await useCartStore.getState().setOrderType("TAKEAWAY");
    await useCartStore.getState().setTableId(null);

    expect(updateOrderShapeMock).not.toHaveBeenCalled();
    expect(useCartStore.getState().orderType).toBe("DINE_IN");
    expect(useCartStore.getState().tableId).toBe("table-1");
  });

  it("startup-hydrate: a recovered DRAFT order's shape can be changed and persists", async () => {
    getActiveDraftOrderMock.mockResolvedValue({
      ...persistedOrder([]),
      order_type: "DINE_IN",
      table_id: null,
      status: "DRAFT",
    });
    await useCartStore.getState().hydrate(MENU_ITEMS);
    expect(useCartStore.getState().orderId).toBe("order-1");
    expect(useCartStore.getState().orderStatus).toBe("DRAFT");

    updateOrderShapeMock.mockResolvedValue({
      ...persistedOrder([]),
      order_type: "DINE_IN",
      table_id: "table-1",
      status: "DRAFT",
    });
    await useCartStore.getState().setTableId("table-1");

    expect(updateOrderShapeMock).toHaveBeenCalledWith("order-1", "DINE_IN", "table-1");
    expect(useCartStore.getState().tableId).toBe("table-1");
  });
});
