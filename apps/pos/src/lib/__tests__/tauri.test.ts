import { describe, expect, it, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

// Imported after the mock so the module under test picks it up.
const { login, listMenuItems, listTables, createOrder, isTauriCommandError } = await import("../tauri");

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
  it("patches in the missing schema_version and parses against the real contract schema", async () => {
    invokeMock.mockResolvedValue([
      {
        id: "00000000-0000-7000-8000-000000000006",
        outlet_id: "00000000-0000-7000-8000-000000000003",
        category_id: "00000000-0000-7000-8000-000000000007",
        name: "Paneer Tikka",
        base_price_paise: 25000,
        is_available: true,
        config_version: 1,
        // schema_version intentionally absent — matches the real Rust DTO.
      },
    ]);
    const items = await listMenuItems();
    expect(items).toHaveLength(1);
    expect(items[0]?.base_price_paise).toBe(25000);
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
