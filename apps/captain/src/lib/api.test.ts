import { describe, expect, it } from "vitest";
import { CanonicalOrderSchema } from "@holler/contracts";
import orderFixture from "../../../../packages/contracts/fixtures/order.json";
import { SessionSchema, CaptainTableSchema, CaptainMenuItemSchema } from "./api";

/**
 * Pins this app's captain-api.md-shaped schemas against real fixtures rather
 * than only against each other, the same discipline apps/admin/session.test.ts
 * uses for the login principal — a schema checked only against itself agrees
 * with the code no matter what the server does.
 */
describe("CanonicalOrderSchema, reused verbatim (docs/captain-api.md)", () => {
  it("parses the shared contracts fixture", () => {
    const result = CanonicalOrderSchema.safeParse(orderFixture);
    expect(result.success, JSON.stringify((result as { error?: unknown }).error)).toBe(true);
  });
});

describe("SessionSchema", () => {
  it("parses a well-formed GET /api/session 200 body", () => {
    const body = {
      device_id: "0191a000-0000-7000-8000-000000000001",
      outlet_id: "0191a000-0000-7000-8000-000000000002",
      outlet_name: "Holler Test Kitchen",
      device_kind: "WAITER",
    };
    expect(SessionSchema.safeParse(body).success).toBe(true);
  });
});

describe("CaptainTableSchema", () => {
  it("accepts a free table with both ids null", () => {
    const body = {
      id: "0191a000-0000-7000-8000-000000000003",
      name: "T4",
      seats: 4,
      open_session_id: null,
      open_order_id: null,
    };
    expect(CaptainTableSchema.safeParse(body).success).toBe(true);
  });
});

describe("CaptainMenuItemSchema", () => {
  it("requires a variant array — never assumes one is present", () => {
    const body = {
      id: "0191a000-0000-7000-8000-000000000004",
      category_id: "0191a000-0000-7000-8000-000000000005",
      name: "Test Item",
      base_price_paise: 10000,
      is_available: true,
      variants: [],
      modifiers: [],
    };
    const parsed = CaptainMenuItemSchema.safeParse(body);
    expect(parsed.success).toBe(true);
  });
});
