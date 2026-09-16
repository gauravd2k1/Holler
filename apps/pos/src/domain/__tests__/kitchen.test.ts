import { describe, expect, it } from "vitest";
import type { AuthenticatedPrincipal, Kot, OrderStatus } from "@holler/contracts";
import {
  buildKotTransitionTable,
  canOfferKotTransition,
  canOfferSendToKitchen,
  kitchenErrorMessage,
  kotStatusLabel,
  kotStatusToneClass,
  kotTransitionActionLabel,
  legalNextKotStatuses,
  orderStatusLabel,
  stationsForKots,
} from "../kitchen";

function principal(permissions: AuthenticatedPrincipal["permissions"]): AuthenticatedPrincipal {
  return {
    user_id: "00000000-0000-7000-8000-000000000001",
    tenant_id: "00000000-0000-7000-8000-000000000002",
    outlet_id: "00000000-0000-7000-8000-000000000003",
    full_name: "Test Cashier",
    permissions,
    authenticated_offline: true,
    schema_version: 1,
  };
}

function kot(overrides: Partial<Kot> = {}): Kot {
  return {
    id: "00000000-0000-7000-8000-000000000009",
    order_id: "00000000-0000-7000-8000-00000000000a",
    station: "MAIN_KITCHEN",
    sequence: 1,
    status: "NEW",
    items: [],
    created_by_device_id: "00000000-0000-7000-8000-00000000000b",
    created_at: "2026-08-10T10:00:00.000Z",
    updated_at: "2026-08-10T10:00:00.000Z",
    schema_version: 1,
    ...overrides,
  };
}

describe("legalNextKotStatuses", () => {
  // The edge's own table, as `list_kot_status_transitions` returns it. This
  // fixture stands in for the command; it is NOT a second copy of the rule —
  // nothing in `src/` declares these pairs any more, and the screen renders
  // whatever the edge sends.
  const fromTheEdge = buildKotTransitionTable([
    ["NEW", ["ACKNOWLEDGED", "CANCELLED"]],
    ["ACKNOWLEDGED", ["PREPARING", "CANCELLED"]],
    ["PREPARING", ["READY", "CANCELLED"]],
    ["READY", ["SERVED"]],
  ]);

  it("offers exactly what the edge sent", () => {
    expect(legalNextKotStatuses(fromTheEdge, "NEW")).toEqual(["ACKNOWLEDGED", "CANCELLED"]);
    expect(legalNextKotStatuses(fromTheEdge, "ACKNOWLEDGED")).toEqual(["PREPARING", "CANCELLED"]);
    expect(legalNextKotStatuses(fromTheEdge, "PREPARING")).toEqual(["READY", "CANCELLED"]);
    expect(legalNextKotStatuses(fromTheEdge, "READY")).toEqual(["SERVED"]);
  });

  it("offers NOTHING for a status the edge did not mention", () => {
    // Terminal statuses are absent from the edge's table rather than present
    // and empty, and a screen must not invent a move for them.
    expect(legalNextKotStatuses(fromTheEdge, "SERVED")).toEqual([]);
    expect(legalNextKotStatuses(fromTheEdge, "CANCELLED")).toEqual([]);
  });

  it("offers NOTHING at all before the edge has answered", () => {
    // THE POINT OF THE WHOLE CHANGE. A button rendered from a guess is what
    // VV-009 caught: the till offered "Acknowledged" for a ticket the kitchen
    // had already acknowledged, and the edge refused it. No table, no buttons.
    for (const status of ["NEW", "ACKNOWLEDGED", "PREPARING", "READY"] as const) {
      expect(legalNextKotStatuses(undefined, status)).toEqual([]);
    }
  });
});

describe("kotTransitionActionLabel", () => {
  it("labels every action as a verb, never as the status it lands in", () => {
    // "Acknowledged" describes a state; "Acknowledge" describes what pressing
    // the button does. The operator pressed one labelled as a state in VV-009.
    expect(kotTransitionActionLabel("ACKNOWLEDGED")).toBe("Acknowledge");
    expect(kotTransitionActionLabel("PREPARING")).toBe("Start preparing");
    expect(kotTransitionActionLabel("READY")).toBe("Mark ready");
    expect(kotTransitionActionLabel("SERVED")).toBe("Mark served");
    expect(kotTransitionActionLabel("CANCELLED")).toBe("Cancel ticket");
  });

  it("never returns the status label itself for a forward move", () => {
    for (const status of ["ACKNOWLEDGED", "PREPARING", "READY", "SERVED"] as const) {
      expect(kotTransitionActionLabel(status)).not.toBe(kotStatusLabel(status));
    }
  });
});

describe("kotStatusToneClass", () => {
  it("gives every status its own class, and none of them stands alone", () => {
    const seen = new Set<string>();
    for (const status of ["NEW", "ACKNOWLEDGED", "PREPARING", "READY", "SERVED", "CANCELLED"] as const) {
      const cls = kotStatusToneClass(status);
      // Colour is ADDED to the words, never substituted for them
      // (docs/spec/kitchen.md §KDS), so the base class that carries the text
      // styling must always be present alongside the per-status modifier.
      expect(cls.startsWith("kot-status ")).toBe(true);
      expect(seen.has(cls)).toBe(false);
      seen.add(cls);
    }
    expect(seen.size).toBe(6);
  });
});

describe("kotStatusLabel / orderStatusLabel", () => {
  it("never returns an empty label for any KotStatus", () => {
    for (const status of ["NEW", "ACKNOWLEDGED", "PREPARING", "READY", "SERVED", "CANCELLED"] as const) {
      expect(kotStatusLabel(status).length).toBeGreaterThan(0);
    }
  });

  it("renders a readable order status", () => {
    expect(orderStatusLabel("SENT_TO_KITCHEN")).toBe("Sent To Kitchen");
    expect(orderStatusLabel("DRAFT")).toBe("Draft");
  });
});

describe("canOfferSendToKitchen", () => {
  const SENDABLE: OrderStatus[] = ["CONFIRMED", "SENT_TO_KITCHEN", "PREPARING"];
  const NOT_SENDABLE: OrderStatus[] = ["DRAFT", "READY", "SERVED", "BILLED", "PAID", "CLOSED", "CANCELLED"];

  it.each(SENDABLE)("offers send-to-kitchen for %s with permission", (status) => {
    expect(canOfferSendToKitchen(status, principal(["order.modify"]))).toBe(true);
  });

  it.each(NOT_SENDABLE)("does not offer send-to-kitchen for %s", (status) => {
    expect(canOfferSendToKitchen(status, principal(["order.modify"]))).toBe(false);
  });

  it("does not offer send-to-kitchen without permission", () => {
    expect(canOfferSendToKitchen("CONFIRMED", principal(["order.create"]))).toBe(false);
  });

  it("does not offer send-to-kitchen with no principal", () => {
    expect(canOfferSendToKitchen("CONFIRMED", null)).toBe(false);
  });
});

describe("canOfferKotTransition", () => {
  it("requires order.modify", () => {
    expect(canOfferKotTransition(principal(["order.modify"]))).toBe(true);
    expect(canOfferKotTransition(principal(["order.create"]))).toBe(false);
    expect(canOfferKotTransition(null)).toBe(false);
  });
});

describe("kitchenErrorMessage", () => {
  it("gives a plain-language message for each documented failure code", () => {
    expect(kitchenErrorMessage({ code: "ORDER_NOT_SENDABLE_TO_KITCHEN" }).length).toBeGreaterThan(0);
    expect(kitchenErrorMessage({ code: "NOTHING_TO_SEND_TO_KITCHEN" }).length).toBeGreaterThan(0);
    expect(kitchenErrorMessage({ code: "ILLEGAL_KOT_STATUS_TRANSITION" }).length).toBeGreaterThan(0);
    expect(kitchenErrorMessage({ code: "NO_PRINTER_ROUTED" }).length).toBeGreaterThan(0);
  });

  it("falls back to a generic message for any other error", () => {
    expect(kitchenErrorMessage({ code: "STORAGE_ERROR" })).toContain("try again");
    expect(kitchenErrorMessage(new Error("boom"))).toContain("try again");
  });

  // Regression for docs/backlog-m2.md Track A / docs/m3-planning.md §2 Track
  // A: a mixed order used to send silently, dropping any unrouted line with
  // no signal anywhere. The edge now rejects the whole call and names the
  // dropped item(s) (apps/pos/src-tauri/src/error.rs); this asserts the POS
  // renders that name rather than a generic message.
  it("surfaces the edge's item-naming message for UNROUTED_KITCHEN_ITEMS verbatim", () => {
    const message = kitchenErrorMessage({
      code: "UNROUTED_KITCHEN_ITEMS",
      message: "2 items have no kitchen station — not sent: Mystery Side, Cold Coffee",
    });
    expect(message).toBe("2 items have no kitchen station — not sent: Mystery Side, Cold Coffee");
    expect(message).toContain("Mystery Side");
    expect(message).toContain("Cold Coffee");
  });

  it("falls back to a non-generic-looking message for UNROUTED_KITCHEN_ITEMS if the edge ever sent no message", () => {
    const message = kitchenErrorMessage({ code: "UNROUTED_KITCHEN_ITEMS" });
    expect(message.length).toBeGreaterThan(0);
    expect(message).not.toContain("try again");
  });
});

describe("stationsForKots", () => {
  it("de-duplicates and sorts station codes", () => {
    const kots = [
      kot({ station: "BAR" }),
      kot({ station: "MAIN_KITCHEN" }),
      kot({ station: "BAR" }),
    ];
    expect(stationsForKots(kots)).toEqual(["BAR", "MAIN_KITCHEN"]);
  });

  it("returns an empty list for no tickets", () => {
    expect(stationsForKots([])).toEqual([]);
  });
});
