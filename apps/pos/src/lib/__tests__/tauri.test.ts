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
  recordPayment,
  issueSplitInvoices,
  listInvoicesForSplitGroup,
  findOpenCashShift,
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
        tax_profile_id: null,
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
        tax_profile_id: null,
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
  const VALID_FAILED_KOT_JOB = {
    id: "00000000-0000-7000-8000-00000000000d",
    target: "KOT",
    kot_id: VALID_KOT.id,
    kot_station: "MAIN_KITCHEN",
    invoice_id: null,
    invoice_number: null,
    printer_id: "00000000-0000-7000-8000-00000000000e",
    status: "FAILED",
    attempt_count: 2,
    last_error: "connect refused",
    created_at: "2026-08-10T10:00:00.000Z",
    updated_at: "2026-08-10T10:00:05.000Z",
    printer_name: "Kitchen Printer",
    schema_version: 1,
  };

  const VALID_FAILED_INVOICE_JOB = {
    id: "00000000-0000-7000-8000-00000000000f",
    target: "INVOICE",
    kot_id: null,
    kot_station: null,
    invoice_id: "00000000-0000-7000-8000-000000000010",
    invoice_number: "FY26/PNQ/001423",
    printer_id: "00000000-0000-7000-8000-00000000000e",
    status: "FAILED",
    attempt_count: 3,
    last_error: "connect refused",
    created_at: "2026-08-10T10:01:00.000Z",
    updated_at: "2026-08-10T10:01:05.000Z",
    printer_name: "Bill Printer",
    schema_version: 1,
  };

  it("listFailedPrintJobs parses a failed KOT job with its target and station", async () => {
    invokeMock.mockResolvedValue([VALID_FAILED_KOT_JOB]);
    const failed = await listFailedPrintJobs();
    expect(failed).toHaveLength(1);
    expect(failed[0]?.target).toBe("KOT");
    expect(failed[0]?.kot_station).toBe("MAIN_KITCHEN");
    expect(failed[0]?.invoice_number).toBeNull();
    expect(failed[0]?.printer_name).toBe("Kitchen Printer");
    expect(failed[0]?.last_error).toBe("connect refused");
  });

  it("listFailedPrintJobs parses a failed invoice job with its target and invoice number", async () => {
    invokeMock.mockResolvedValue([VALID_FAILED_INVOICE_JOB]);
    const failed = await listFailedPrintJobs();
    expect(failed).toHaveLength(1);
    expect(failed[0]?.target).toBe("INVOICE");
    expect(failed[0]?.invoice_number).toBe("FY26/PNQ/001423");
    expect(failed[0]?.kot_station).toBeNull();
  });

  it("listFailedPrintJobs returns both kinds distinctly in one call", async () => {
    invokeMock.mockResolvedValue([VALID_FAILED_KOT_JOB, VALID_FAILED_INVOICE_JOB]);
    const failed = await listFailedPrintJobs();
    expect(failed.map((j) => j.target)).toEqual(["KOT", "INVOICE"]);
  });

  it("retryFailedPrintJobs invokes retry_failed_print_jobs and returns the still-failing set", async () => {
    invokeMock.mockResolvedValue([]);
    const failed = await retryFailedPrintJobs();
    expect(invokeMock).toHaveBeenCalledWith("retry_failed_print_jobs");
    expect(failed).toEqual([]);
  });
});

describe("recordPayment", () => {
  const VALID_PAYMENT = {
    id: "018e5a2e-a001-7c3d-9f4e-1234567890ab",
    outlet_id: "018e5a2e-1a09-7c3d-9f4e-1234567890ab",
    order_id: "018e5a2e-2b10-7c3d-9f4e-1234567890ab",
    cash_shift_id: null,
    method: "CASH",
    status: "CAPTURED",
    amount_paise: 105000,
    tendered_paise: 105000,
    change_paise: 0,
    reference: null,
    external_id: null,
    reverses_payment_id: null,
    captured_at: "2026-08-12T14:32:00Z",
    allocations: [],
    created_by_user_id: "018e5a2e-3d12-7c3d-9f4e-1234567890ab",
    created_at: "2026-08-12T14:32:00Z",
    updated_at: "2026-08-12T14:32:00Z",
    version: 1,
    schema_version: 1,
  };

  // T9 retry, Defect 1: the caller must be able to name which invoice a
  // forward tender settles, so the edge can reject one that would exceed
  // its remaining due — verifies the wire shape actually carries `invoiceId`
  // through to the `record_payment` command, not just that the response
  // parses.
  it("passes invoiceId through to the record_payment command", async () => {
    invokeMock.mockResolvedValue(VALID_PAYMENT);
    await recordPayment({
      orderId: VALID_PAYMENT.order_id,
      method: "CASH",
      amountPaise: 105000,
      tenderedPaise: 105000,
      changePaise: 0,
      reference: null,
      cashShiftId: null,
      reversesPaymentId: null,
      invoiceId: "018e5a2e-9001-7c3d-9f4e-1234567890ab",
      createdByUserId: VALID_PAYMENT.created_by_user_id,
    });
    expect(invokeMock).toHaveBeenCalledWith(
      "record_payment",
      expect.objectContaining({ invoiceId: "018e5a2e-9001-7c3d-9f4e-1234567890ab" }),
    );
  });

  it("throws a normalized TauriCommandError on a double-settlement rejection", async () => {
    invokeMock.mockRejectedValue({
      code: "FORWARD_PAYMENT_EXCEEDS_REMAINING_DUE",
      message: "payment of 100 paise exceeds invoice inv-1's remaining due of 0 paise",
    });
    await expect(
      recordPayment({
        orderId: VALID_PAYMENT.order_id,
        method: "CASH",
        amountPaise: 100,
        tenderedPaise: 100,
        changePaise: 0,
        reference: null,
        cashShiftId: null,
        reversesPaymentId: null,
        invoiceId: "018e5a2e-9001-7c3d-9f4e-1234567890ab",
        createdByUserId: VALID_PAYMENT.created_by_user_id,
      }),
    ).rejects.toSatisfy((err: unknown) => isTauriCommandError(err));
  });
});

describe("issueSplitInvoices / listInvoicesForSplitGroup", () => {
  const VALID_INVOICE_PART_1 = {
    id: "018e5a2e-9001-7c3d-9f4e-1234567890ab",
    outlet_id: "018e5a2e-1a09-7c3d-9f4e-1234567890ab",
    order_id: "018e5a2e-2b10-7c3d-9f4e-1234567890ab",
    split_group_id: "018e5a2e-9500-7c3d-9f4e-1234567890ab",
    split_index: 1,
    split_count: 2,
    series_id: "018e5a2e-9101-7c3d-9f4e-1234567890ab",
    invoice_number: "FY26/PNQ/001423",
    invoice_date: "2026-08-12T14:30:00Z",
    business_date: "2026-08-12",
    status: "ISSUED",
    cancelled_reason: null,
    cancelled_at: null,
    customer_name: null,
    customer_phone: null,
    customer_gstin: null,
    place_of_supply_state_code: "27",
    lines: [],
    subtotal_paise: 20000,
    discount_paise: 0,
    taxable_value_paise: 20000,
    cgst_paise: 500,
    sgst_paise: 500,
    igst_paise: 0,
    cess_paise: 0,
    round_off_paise: 0,
    grand_total_paise: 21000,
    compliance_version_id: "018e5a2e-9401-7c3d-9f4e-1234567890ab",
    tax_snapshot: {},
    fiscal_profile: {},
    channel: "POS",
    tax_liability_party: "RESTAURANT",
    eco_operator_name: null,
    eco_operator_gstin: null,
    supply_classification: null,
    created_by_user_id: "018e5a2e-3d12-7c3d-9f4e-1234567890ab",
    created_at: "2026-08-12T14:30:00Z",
    updated_at: "2026-08-12T14:30:00Z",
    version: 1,
    schema_version: 1,
  };
  const VALID_INVOICE_PART_2 = { ...VALID_INVOICE_PART_1, split_index: 2 };

  // The wire shape actually carries per-part order_item_id/quantity through
  // to `issue_split_invoices` — not merely that the response parses. This is
  // what lets a mismatched split reach the edge's own §66 conservation
  // check at all.
  it("passes parts' order_item_id/quantity through to the issue_split_invoices command", async () => {
    invokeMock.mockResolvedValue([VALID_INVOICE_PART_1, VALID_INVOICE_PART_2]);
    const result = await issueSplitInvoices(
      VALID_INVOICE_PART_1.order_id,
      VALID_INVOICE_PART_1.created_by_user_id,
      [
        { lines: [{ orderItemId: "item-1", quantity: 1 }] },
        { lines: [{ orderItemId: "item-1", quantity: 1 }] },
      ],
    );
    expect(invokeMock).toHaveBeenCalledWith(
      "issue_split_invoices",
      expect.objectContaining({
        parts: [
          { lines: [{ order_item_id: "item-1", quantity: 1 }] },
          { lines: [{ order_item_id: "item-1", quantity: 1 }] },
        ],
      }),
    );
    expect(result).toHaveLength(2);
    expect(result[0]?.split_index).toBe(1);
    expect(result[1]?.split_index).toBe(2);
  });

  it("throws a normalized TauriCommandError when the edge rejects an over/under-billed split", async () => {
    invokeMock.mockRejectedValue({
      code: "INVALID_INPUT",
      message:
        "split conservation violated for order_item item-1: order line has quantity 1 but the supplied shares total 2",
    });
    await expect(
      issueSplitInvoices(VALID_INVOICE_PART_1.order_id, VALID_INVOICE_PART_1.created_by_user_id, [
        { lines: [{ orderItemId: "item-1", quantity: 1 }] },
        { lines: [{ orderItemId: "item-1", quantity: 1 }] },
      ]),
    ).rejects.toSatisfy((err: unknown) => isTauriCommandError(err));
  });

  it("lists every invoice sharing a split_group_id", async () => {
    invokeMock.mockResolvedValue([VALID_INVOICE_PART_1, VALID_INVOICE_PART_2]);
    const result = await listInvoicesForSplitGroup(VALID_INVOICE_PART_1.split_group_id);
    expect(invokeMock).toHaveBeenCalledWith(
      "list_invoices_for_split_group",
      expect.objectContaining({ splitGroupId: VALID_INVOICE_PART_1.split_group_id }),
    );
    expect(result).toHaveLength(2);
  });
});

describe("findOpenCashShift", () => {
  const VALID_SHIFT = {
    id: "018e5a2e-b001-7c3d-9f4e-1234567890ab",
    outlet_id: "018e5a2e-1a09-7c3d-9f4e-1234567890ab",
    device_id: "018e5a2e-1b0a-7c3d-9f4e-1234567890ab",
    cashier_user_id: "018e5a2e-3d12-7c3d-9f4e-1234567890ab",
    status: "OPEN",
    opened_at: "2026-08-12T04:30:00Z",
    opening_cash_paise: 200000,
    closed_at: null,
    expected_cash_paise: null,
    actual_cash_paise: null,
    variance_paise: null,
    variance_reason: null,
    business_date: "2026-08-12",
    movements: [],
    created_at: "2026-08-12T04:30:00Z",
    updated_at: "2026-08-12T04:30:00Z",
    version: 1,
    schema_version: 1,
  };

  // T9 retry, Defect 2: recovery needs no shift id — only the cashier.
  it("invokes find_open_cash_shift with only the cashier id and parses the recovered shift", async () => {
    invokeMock.mockResolvedValue(VALID_SHIFT);
    const shift = await findOpenCashShift(VALID_SHIFT.cashier_user_id);
    expect(invokeMock).toHaveBeenCalledWith("find_open_cash_shift", {
      cashierUserId: VALID_SHIFT.cashier_user_id,
    });
    expect(shift?.id).toBe(VALID_SHIFT.id);
    expect(shift?.status).toBe("OPEN");
  });

  it("returns null rather than throwing when nothing is open", async () => {
    invokeMock.mockResolvedValue(null);
    const shift = await findOpenCashShift(VALID_SHIFT.cashier_user_id);
    expect(shift).toBeNull();
  });
});
