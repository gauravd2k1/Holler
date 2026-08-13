import { describe, expect, it, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

// Imported after the mock so the module under test picks it up.
const {
  login,
  listMenuItems,
  listMenuCategories,
  listTables,
  createOrder,
  confirmOrder,
  addOrderItem,
  removeOrderItem,
  updateOrderShape,
  updateOrderItemQuantity,
  getActiveDraftOrder,
  sendOrderToKitchen,
  listKotsForOrder,
  transitionKotStatus,
  listStations,
  listFailedPrintJobs,
  retryFailedPrintJobs,
  isTauriCommandError,
} = await import("../tauri");

beforeEach(() => {
  invokeMock.mockReset();
});

const VALID_PRINCIPAL = {
  user_id: "00000000-0000-7000-8000-000000000001",
  tenant_id: "00000000-0000-7000-8000-000000000002",
  outlet_id: "00000000-0000-7000-8000-000000000003",
  full_name: "Test Cashier",
  permissions: ["order.create"],
  authenticated_offline: true,
  schema_version: 1,
};

const VALID_TABLE = {
  id: "00000000-0000-7000-8000-000000000004",
  outlet_id: "00000000-0000-7000-8000-000000000003",
  section: "GROUND",
  label: "T4",
  seat_count: 4,
  is_active: true,
  config_version: 1,
  schema_version: 1,
};

const VALID_ORDER = {
  holler_order_id: "00000000-0000-7000-8000-000000000005",
  display_number: "A1",
  external_order_id: null,
  source: "POS",
  outlet_id: "00000000-0000-7000-8000-000000000003",
  order_type: "TAKEAWAY",
  status: "DRAFT",
  table_id: null,
  customer: null,
  delivery_address: null,
  items: [],
  subtotal_paise: 0,
  discount_paise: 0,
  packaging_paise: 0,
  delivery_charge_paise: 0,
  taxes_paise: 0,
  aggregator_discount_paise: 0,
  merchant_discount_paise: 0,
  total_paise: 0,
  payment_status: "UNPAID",
  payment_source: null,
  preparation_time_minutes: null,
  rider: null,
  timestamps: {
    created_at: "2026-08-07T10:00:00.000Z",
    confirmed_at: null,
    updated_at: "2026-08-07T10:00:00.000Z",
  },
  source_payload: null,
  schema_version: 1,
};

describe("login", () => {
  it("parses a valid principal", async () => {
    invokeMock.mockResolvedValue(VALID_PRINCIPAL);
    const principal = await login("a@b.com", "secret");
    expect(principal.user_id).toBe(VALID_PRINCIPAL.user_id);
  });

  it("throws a normalized TauriCommandError on rejection", async () => {
    invokeMock.mockRejectedValue({ code: "CREDENTIAL_MISMATCH", message: "invalid email or password" });
    await expect(login("a@b.com", "wrong")).rejects.toSatisfy((err: unknown) => isTauriCommandError(err));
  });

  it("rejects a malformed principal rather than trusting the cast", async () => {
    invokeMock.mockResolvedValue({ user_id: "not-a-uuid" });
    await expect(login("a@b.com", "secret")).rejects.toBeTruthy();
  });
});

describe("listMenuItems", () => {
  it("parses against the real contract schema, including schema_version from the Rust DTO", async () => {
    invokeMock.mockResolvedValue([
      {
        id: "00000000-0000-7000-8000-000000000006",
        outlet_id: "00000000-0000-7000-8000-000000000003",
        category_id: "00000000-0000-7000-8000-000000000007",
        name: "Paneer Tikka",
        base_price_paise: 25000,
        is_available: true,
        config_version: 1,
        schema_version: 1,
      },
    ]);
    const items = await listMenuItems();
    expect(items).toHaveLength(1);
    expect(items[0]?.base_price_paise).toBe(25000);
  });

  it("rejects a response missing schema_version rather than inventing it", async () => {
    invokeMock.mockResolvedValue([
      {
        id: "00000000-0000-7000-8000-000000000006",
        outlet_id: "00000000-0000-7000-8000-000000000003",
        category_id: "00000000-0000-7000-8000-000000000007",
        name: "Paneer Tikka",
        base_price_paise: 25000,
        is_available: true,
        config_version: 1,
      },
    ]);
    await expect(listMenuItems()).rejects.toBeTruthy();
  });
});

describe("listTables", () => {
  it("parses valid tables", async () => {
    invokeMock.mockResolvedValue([VALID_TABLE]);
    const tables = await listTables();
    expect(tables[0]?.label).toBe("T4");
  });
});

describe("createOrder", () => {
  it("parses a valid CanonicalOrder", async () => {
    invokeMock.mockResolvedValue(VALID_ORDER);
    const order = await createOrder("TAKEAWAY", null, []);
    expect(order.holler_order_id).toBe(VALID_ORDER.holler_order_id);
    expect(order.total_paise).toBe(0);
  });
});

describe("confirmOrder", () => {
  it("invokes confirm_order with the order id and Zod-parses the response", async () => {
    const confirmed = {
      ...VALID_ORDER,
      status: "CONFIRMED",
      timestamps: { ...VALID_ORDER.timestamps, confirmed_at: "2026-08-08T10:00:00.000Z" },
    };
    invokeMock.mockResolvedValue(confirmed);
    const order = await confirmOrder(VALID_ORDER.holler_order_id);
    expect(invokeMock).toHaveBeenCalledWith("confirm_order", { orderId: VALID_ORDER.holler_order_id });
    expect(order.status).toBe("CONFIRMED");
    expect(order.timestamps.confirmed_at).toBe("2026-08-08T10:00:00.000Z");
  });

  it("throws a normalized TauriCommandError on ORDER_NOT_CONFIRMABLE rejection", async () => {
    invokeMock.mockRejectedValue({
      code: "ORDER_NOT_CONFIRMABLE",
      message: "order x is not in DRAFT status and cannot be confirmed",
    });
    await expect(confirmOrder(VALID_ORDER.holler_order_id)).rejects.toSatisfy((err: unknown) =>
      isTauriCommandError(err),
    );
  });

  it("rejects a malformed response rather than trusting the cast", async () => {
    invokeMock.mockResolvedValue({ holler_order_id: "not-a-uuid" });
    await expect(confirmOrder(VALID_ORDER.holler_order_id)).rejects.toBeTruthy();
  });
});

describe("listMenuCategories", () => {
  it("parses categories from the local schema (no @holler/contracts mirror yet)", async () => {
    invokeMock.mockResolvedValue([
      {
        id: "00000000-0000-7000-8000-000000000007",
        outlet_id: "00000000-0000-7000-8000-000000000003",
        name: "Starters",
        sort_order: 1,
        config_version: 1,
      },
    ]);
    const categories = await listMenuCategories();
    expect(categories[0]?.name).toBe("Starters");
  });
});

describe("add/removeOrderItem", () => {
  it("addOrderItem invokes add_order_item with the order id and item, and parses the response", async () => {
    invokeMock.mockResolvedValue(VALID_ORDER);
    const order = await addOrderItem(VALID_ORDER.holler_order_id, {
      menu_item_id: "00000000-0000-7000-8000-000000000006",
      variant_id: null,
      quantity: 1,
      unit_price_paise: 25000,
      notes: null,
    });
    expect(invokeMock).toHaveBeenCalledWith("add_order_item", expect.objectContaining({
      orderId: VALID_ORDER.holler_order_id,
    }));
    expect(order.holler_order_id).toBe(VALID_ORDER.holler_order_id);
  });

  it("removeOrderItem invokes remove_order_item with the order and item ids", async () => {
    invokeMock.mockResolvedValue(VALID_ORDER);
    await removeOrderItem(VALID_ORDER.holler_order_id, "item-1");
    expect(invokeMock).toHaveBeenCalledWith("remove_order_item", {
      orderId: VALID_ORDER.holler_order_id,
      orderItemId: "item-1",
    });
  });

  it("normalizes an ORDER_NOT_DRAFT rejection", async () => {
    invokeMock.mockRejectedValue({ code: "ORDER_NOT_DRAFT", message: "not draft" });
    await expect(
      addOrderItem(VALID_ORDER.holler_order_id, {
        menu_item_id: "x",
        variant_id: null,
        quantity: 1,
        unit_price_paise: 100,
        notes: null,
      }),
    ).rejects.toSatisfy((err: unknown) => isTauriCommandError(err));
  });

  it("addOrderItem forwards modifiers when present, so a delta actually reaches the wire", async () => {
    invokeMock.mockResolvedValue(VALID_ORDER);
    await addOrderItem(VALID_ORDER.holler_order_id, {
      menu_item_id: "00000000-0000-7000-8000-000000000006",
      variant_id: null,
      quantity: 1,
      unit_price_paise: 25000,
      notes: null,
      modifiers: [
        {
          modifier_id: "00000000-0000-7000-8000-000000000099",
          group_name: "Extras",
          option_name: "Extra cheese",
          price_delta_paise: 3000,
        },
      ],
    });
    expect(invokeMock).toHaveBeenCalledWith(
      "add_order_item",
      expect.objectContaining({
        item: expect.objectContaining({
          modifiers: [
            expect.objectContaining({ option_name: "Extra cheese", price_delta_paise: 3000 }),
          ],
        }),
      }),
    );
  });
});

describe("updateOrderItemQuantity", () => {
  it("invokes update_order_item_quantity with orderId/orderItemId/quantity and parses the response", async () => {
    invokeMock.mockResolvedValue(VALID_ORDER);
    const order = await updateOrderItemQuantity(VALID_ORDER.holler_order_id, "item-1", 5);
    expect(invokeMock).toHaveBeenCalledWith("update_order_item_quantity", {
      orderId: VALID_ORDER.holler_order_id,
      orderItemId: "item-1",
      quantity: 5,
    });
    expect(order.holler_order_id).toBe(VALID_ORDER.holler_order_id);
  });

  it("normalizes an ORDER_ITEM_ALREADY_TICKETED rejection", async () => {
    invokeMock.mockRejectedValue({
      code: "ORDER_ITEM_ALREADY_TICKETED",
      message: "already ticketed at the kitchen",
    });
    await expect(
      updateOrderItemQuantity(VALID_ORDER.holler_order_id, "item-1", 5),
    ).rejects.toSatisfy((err: unknown) => isTauriCommandError(err));
  });
});

describe("updateOrderShape", () => {
  it("invokes update_order_shape with the order id, order type and table id, and parses the response", async () => {
    invokeMock.mockResolvedValue(VALID_ORDER);
    const order = await updateOrderShape(VALID_ORDER.holler_order_id, "TAKEAWAY", null);
    expect(invokeMock).toHaveBeenCalledWith("update_order_shape", {
      orderId: VALID_ORDER.holler_order_id,
      orderType: "TAKEAWAY",
      tableId: null,
    });
    expect(order.holler_order_id).toBe(VALID_ORDER.holler_order_id);
  });

  it("normalizes an ORDER_NOT_DRAFT rejection once the order has left DRAFT", async () => {
    invokeMock.mockRejectedValue({ code: "ORDER_NOT_DRAFT", message: "not draft" });
    await expect(
      updateOrderShape(VALID_ORDER.holler_order_id, "DINE_IN", "table-1"),
    ).rejects.toSatisfy((err: unknown) => isTauriCommandError(err));
  });
});

describe("getActiveDraftOrder", () => {
  it("invokes get_active_draft_order with no arguments and parses the response", async () => {
    invokeMock.mockResolvedValue(VALID_ORDER);
    const order = await getActiveDraftOrder();
    expect(invokeMock).toHaveBeenCalledWith("get_active_draft_order");
    expect(order?.holler_order_id).toBe(VALID_ORDER.holler_order_id);
  });

  it("returns null rather than throwing when there is nothing to recover", async () => {
    invokeMock.mockResolvedValue(null);
    const order = await getActiveDraftOrder();
    expect(order).toBeNull();
  });

  it("rejects a malformed response rather than trusting the cast", async () => {
    invokeMock.mockResolvedValue({ holler_order_id: "not-a-uuid" });
    await expect(getActiveDraftOrder()).rejects.toBeTruthy();
  });
});

const VALID_KOT = {
  id: "00000000-0000-7000-8000-000000000009",
  order_id: VALID_ORDER.holler_order_id,
  station: "MAIN_KITCHEN",
  sequence: 1,
  status: "NEW",
  items: [
    {
      order_item_id: "00000000-0000-7000-8000-00000000000a",
      name: "Paneer Tikka",
      quantity: 2,
      modifiers: [],
      notes: null,
    },
  ],
  created_by_device_id: "00000000-0000-7000-8000-00000000000b",
  created_at: "2026-08-10T10:00:00.000Z",
  updated_at: "2026-08-10T10:00:00.000Z",
  schema_version: 1,
};

describe("sendOrderToKitchen / listKotsForOrder / transitionKotStatus", () => {
  it("sendOrderToKitchen parses the returned KOTs against the frozen contract schema", async () => {
    invokeMock.mockResolvedValue([VALID_KOT]);
    const kots = await sendOrderToKitchen(VALID_ORDER.holler_order_id);
    expect(kots).toHaveLength(1);
    expect(kots[0]?.station).toBe("MAIN_KITCHEN");
  });

  it("listKotsForOrder invokes list_kots_for_order with the order id", async () => {
    invokeMock.mockResolvedValue([VALID_KOT]);
    await listKotsForOrder(VALID_ORDER.holler_order_id);
    expect(invokeMock).toHaveBeenCalledWith("list_kots_for_order", {
      orderId: VALID_ORDER.holler_order_id,
    });
  });

  it("transitionKotStatus invokes transition_kot_status with all three ids/status", async () => {
    invokeMock.mockResolvedValue([{ ...VALID_KOT, status: "ACKNOWLEDGED" }]);
    const kots = await transitionKotStatus(VALID_ORDER.holler_order_id, VALID_KOT.id, "ACKNOWLEDGED");
    expect(invokeMock).toHaveBeenCalledWith("transition_kot_status", {
      orderId: VALID_ORDER.holler_order_id,
      kotId: VALID_KOT.id,
      newStatus: "ACKNOWLEDGED",
    });
    expect(kots[0]?.status).toBe("ACKNOWLEDGED");
  });

  it("rejects a malformed KOT rather than trusting the cast", async () => {
    invokeMock.mockResolvedValue([{ id: "not-a-uuid" }]);
    await expect(listKotsForOrder(VALID_ORDER.holler_order_id)).rejects.toBeTruthy();
  });
});

describe("listStations", () => {
  it("parses stations against the frozen contract schema", async () => {
    invokeMock.mockResolvedValue([
      {
        id: "00000000-0000-7000-8000-00000000000c",
        outlet_id: "00000000-0000-7000-8000-000000000003",
        code: "MAIN_KITCHEN",
        name: "Main Kitchen",
        sort_order: 1,
        is_active: true,
        config_version: 1,
        schema_version: 1,
      },
    ]);
    const stations = await listStations();
    expect(stations[0]?.code).toBe("MAIN_KITCHEN");
  });
});

describe("listFailedPrintJobs / retryFailedPrintJobs", () => {
  const VALID_FAILED_JOB = {
    id: "00000000-0000-7000-8000-00000000000d",
    kot_id: VALID_KOT.id,
    printer_id: "00000000-0000-7000-8000-00000000000e",
    status: "FAILED",
    attempt_count: 2,
    last_error: "connect refused",
    created_at: "2026-08-10T10:00:00.000Z",
    updated_at: "2026-08-10T10:00:05.000Z",
    printer_name: "Kitchen Printer",
    kot_station: "MAIN_KITCHEN",
    schema_version: 1,
  };

  it("listFailedPrintJobs parses the extended PrintJobSchema view", async () => {
    invokeMock.mockResolvedValue([VALID_FAILED_JOB]);
    const failed = await listFailedPrintJobs();
    expect(failed).toHaveLength(1);
    expect(failed[0]?.printer_name).toBe("Kitchen Printer");
    expect(failed[0]?.last_error).toBe("connect refused");
  });

  it("retryFailedPrintJobs invokes retry_failed_print_jobs and returns the still-failing set", async () => {
    invokeMock.mockResolvedValue([]);
    const failed = await retryFailedPrintJobs();
    expect(invokeMock).toHaveBeenCalledWith("retry_failed_print_jobs");
    expect(failed).toEqual([]);
  });
});
