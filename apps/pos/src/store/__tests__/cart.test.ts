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
const updateOrderItemQuantityMock = vi.fn();

vi.mock("../../lib/tauri", async () => {
  const actual = await vi.importActual<typeof import("../../lib/tauri")>("../../lib/tauri");
  return {
    ...actual,
    getActiveDraftOrder: (...args: unknown[]) => getActiveDraftOrderMock(...args),
    createOrder: (...args: unknown[]) => createOrderMock(...args),
    addOrderItem: (...args: unknown[]) => addOrderItemMock(...args),
    removeOrderItem: (...args: unknown[]) => removeOrderItemMock(...args),
    updateOrderShape: (...args: unknown[]) => updateOrderShapeMock(...args),
    updateOrderItemQuantity: (...args: unknown[]) => updateOrderItemQuantityMock(...args),
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
    tax_profile_id: null,
    hsn_sac: null,
    config_version: 1,
    schema_version: 1,
  },
];

function persistedOrder(items: CanonicalOrder["items"]): CanonicalOrder {
  return {
    holler_order_id: "order-1",
    display_number: "A1",
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
  updateOrderItemQuantityMock.mockReset();
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
  it("addItem creates the DRAFT order on the first line, then raises quantity on the same line rather than adding a second (docs/backlog-m2.md)", async () => {
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
    // Mirrors what the real edge does: persists exactly the requested
    // quantity and reports it back — not a fixed canned value, so this
    // catches the store sending the wrong running total on tap N.
    updateOrderItemQuantityMock.mockImplementation(
      (_orderId: string, _orderItemId: string, quantity: number) =>
        Promise.resolve(
          persistedOrder([
            {
              id: "oi-1",
              menu_item_id: "item-1",
              variant_id: null,
              quantity,
              unit_price_paise: 25000,
              line_total_paise: 25000 * quantity,
              modifiers: [],
              notes: null,
            },
          ]),
        ),
    );

    // Five taps of the same item.
    for (let i = 0; i < 5; i += 1) {
      await useCartStore.getState().addItem(
        { menuItemId: "item-1", variantId: null, unitPricePaise: 25000, quantity: 1, notes: null },
        MENU_ITEMS,
      );
    }

    expect(createOrderMock).toHaveBeenCalledTimes(1);
    // Every tap after the first raises the existing line's quantity — never
    // a second call to add_order_item for the same plain item.
    expect(addOrderItemMock).not.toHaveBeenCalled();
    expect(updateOrderItemQuantityMock).toHaveBeenCalledTimes(4);
    expect(updateOrderItemQuantityMock).toHaveBeenLastCalledWith("order-1", "oi-1", 5);
    // One line, quantity 5 — not five lines of quantity 1.
    expect(useCartStore.getState().lines).toHaveLength(1);
    expect(useCartStore.getState().lines[0]?.quantity).toBe(5);
  });

  it("a modifier line never merges into a plain line of the same menu item", async () => {
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
          line_total_paise: 28000,
          modifiers: [
            {
              modifier_id: "00000000-0000-7000-8000-000000000099",
              group_name: "Extras",
              option_name: "Extra cheese",
              price_delta_paise: 3000,
            },
          ],
          notes: null,
        },
      ]),
    );

    await useCartStore.getState().addItem(
      { menuItemId: "item-1", variantId: null, unitPricePaise: 25000, quantity: 1, notes: null },
      MENU_ITEMS,
    );
    await useCartStore.getState().addItem(
      {
        menuItemId: "item-1",
        variantId: null,
        unitPricePaise: 25000,
        quantity: 1,
        notes: null,
        modifiers: [{ groupName: "Extras", optionName: "Extra cheese", priceDeltaPaise: 3000 }],
      },
      MENU_ITEMS,
    );

    expect(updateOrderItemQuantityMock).not.toHaveBeenCalled();
    expect(addOrderItemMock).toHaveBeenCalledTimes(1);
    const [, sentItem] = addOrderItemMock.mock.calls[0] as [string, { modifiers: unknown[] }];
    expect(sentItem.modifiers).toHaveLength(1);
    const state = useCartStore.getState();
    expect(state.lines).toHaveLength(2);
    expect(state.lines[1]?.modifiers[0]?.priceDeltaPaise).toBe(3000);
    // The delta is visible in the line total, not just tucked in the array.
    expect(state.lines[1]?.lineTotalPaise).toBe(28000);
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

  it("setLineQuantity surfaces ORDER_ITEM_ALREADY_TICKETED verbatim rather than silently diverging from the kitchen", async () => {
    useCartStore.setState({
      orderId: "order-1",
      orderStatus: "SENT_TO_KITCHEN",
      lines: [
        {
          lineId: "oi-1",
          menuItemId: "item-1",
          menuItemName: "Paneer Tikka",
          variantId: null,
          unitPricePaise: 25000,
          quantity: 1,
          notes: null,
          modifiers: [],
          lineTotalPaise: 25000,
        },
      ],
    });
    updateOrderItemQuantityMock.mockRejectedValue({
      code: "ORDER_ITEM_ALREADY_TICKETED",
      message:
        "order item oi-1 on order order-1 is already ticketed at the kitchen; its quantity cannot be changed in place — cancel the line and add a replacement with the new quantity",
    });

    await useCartStore.getState().setLineQuantity("oi-1", 2, MENU_ITEMS);

    const state = useCartStore.getState();
    expect(state.error).toContain("already ticketed at the kitchen");
    // The rejected write must not have silently changed the line.
    expect(state.lines[0]?.quantity).toBe(1);
  });

  it("removeItem replaces lines from the edge's response, not by filtering locally", async () => {
    useCartStore.setState({
      orderId: "order-1",
      lines: [
        {
          lineId: "oi-1",
          menuItemId: "item-1",
          menuItemName: "Paneer Tikka",
          variantId: null,
          unitPricePaise: 25000,
          quantity: 1,
          notes: null,
          modifiers: [],
          lineTotalPaise: 25000,
        },
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
          modifiers: [],
          lineTotalPaise: 25000,
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
