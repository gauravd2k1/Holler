// Contract-drift test (ADR-008): fixtures under packages/contracts/fixtures/
// must parse cleanly through the Zod schemas and round-trip identically.
// The mirrored Go check lives in go/drift_test.go.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { CanonicalOrderSchema } from "./order";
import { KotSchema } from "./kot";
import { SyncEnvelopeSchema, AGGREGATE_AUTHORITY, AggregateTypeSchema } from "./sync";
import { AppUserSchema, AuditEventSchema, AUDIT_REDACTED_FIELDS } from "./identity";
import { RestaurantTableSchema, TableSessionSchema } from "./table";
import { MenuItemSchema, MenuItemModifierSchema } from "./menu";
import { StationSchema, MenuItemStationSchema } from "./station";
import { PrinterSchema, StationPrinterSchema, PrintJobSchema } from "./printer";
import { OUTBOX_EVENT_TYPES } from "./events";

function loadFixture(name: string): unknown {
  return JSON.parse(readFileSync(resolve(__dirname, "../../fixtures", name), "utf-8"));
}

describe("contract drift", () => {
  it("order.json round-trips through CanonicalOrderSchema", () => {
    const raw = loadFixture("order.json");
    const parsed = CanonicalOrderSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  it("kot.json round-trips through KotSchema", () => {
    const raw = loadFixture("kot.json");
    const parsed = KotSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  it("sync_envelope.json satisfies the §50.1 authority rule", () => {
    const raw = loadFixture("sync_envelope.json");
    const parsed = SyncEnvelopeSchema.parse(raw);
    expect(AGGREGATE_AUTHORITY[parsed.aggregate_type]).toBe(parsed.direction);
  });

  it("app_user.json round-trips through AppUserSchema", () => {
    const raw = loadFixture("app_user.json");
    const parsed = AppUserSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  it("restaurant_table.json round-trips through RestaurantTableSchema", () => {
    const raw = loadFixture("restaurant_table.json");
    const parsed = RestaurantTableSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  it("table_session.json round-trips through TableSessionSchema", () => {
    const raw = loadFixture("table_session.json");
    const parsed = TableSessionSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  it("every aggregate type has an authority direction (§50.1)", () => {
    for (const aggregate of AggregateTypeSchema.options) {
      expect(AGGREGATE_AUTHORITY[aggregate]).toBeDefined();
    }
  });

  it("Milestone 1 aggregates carry the ADR-011 authority directions", () => {
    expect(AGGREGATE_AUTHORITY.table_session).toBe("EDGE_TO_CLOUD");
    expect(AGGREGATE_AUTHORITY.app_user).toBe("CLOUD_TO_EDGE");
    expect(AGGREGATE_AUTHORITY.role).toBe("CLOUD_TO_EDGE");
    expect(AGGREGATE_AUTHORITY.restaurant_table).toBe("CLOUD_TO_EDGE");
  });

  it("audit_event.json round-trips through AuditEventSchema", () => {
    const raw = loadFixture("audit_event.json");
    const parsed = AuditEventSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  it("menu_item.json round-trips through MenuItemSchema", () => {
    const raw = loadFixture("menu_item.json");
    const parsed = MenuItemSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  it("menu_item_modifier.json round-trips through MenuItemModifierSchema", () => {
    const raw = loadFixture("menu_item_modifier.json");
    const parsed = MenuItemModifierSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  // Milestone 2 boundary-crossing tables (0.3.0, ADR-014). Every one of these
  // has a row in both stores, so a shape that drifts breaks replay silently —
  // the same failure the order-level round-trip test was added to catch.
  it("station.json round-trips through StationSchema", () => {
    const raw = loadFixture("station.json");
    const parsed = StationSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  it("menu_item_station.json round-trips through MenuItemStationSchema", () => {
    const raw = loadFixture("menu_item_station.json");
    const parsed = MenuItemStationSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  it("printer.json round-trips through PrinterSchema", () => {
    const raw = loadFixture("printer.json");
    const parsed = PrinterSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  it("station_printer.json round-trips through StationPrinterSchema", () => {
    const raw = loadFixture("station_printer.json");
    const parsed = StationPrinterSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  // Edge-local: SQLite only, no Postgres mirror, no wire route. It still gets a
  // round-trip because the POS reads it across the Tauri boundary to show staff
  // a failed print.
  it("print_job.json round-trips through PrintJobSchema", () => {
    const raw = loadFixture("print_job.json");
    const parsed = PrintJobSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  // print_job is deliberately not an aggregate — see the note on PrintJobSchema
  // and the refresh_token precedent. If someone adds it to AggregateTypeSchema,
  // they have given the spool a sync direction, and this fails.
  it("keeps edge-local and cloud-only tables out of AggregateType", () => {
    expect(AggregateTypeSchema.options).not.toContain("print_job");
    expect(AggregateTypeSchema.options).not.toContain("refresh_token");
    expect(AggregateTypeSchema.options).not.toContain("kot_status_history");
  });

  // Stations and printers are config; the ticket at the station is not. This is
  // the ADR-011 restaurant_table/table_session split applied to the kitchen.
  it("Milestone 2 config aggregates never become edge-authoritative", () => {
    expect(AGGREGATE_AUTHORITY.station).toBe("CLOUD_TO_EDGE");
    expect(AGGREGATE_AUTHORITY.printer).toBe("CLOUD_TO_EDGE");
    expect(AGGREGATE_AUTHORITY.kot).toBe("EDGE_TO_CLOUD");
  });

  it("event type list matches Go's OutboxEventTypes, in order", () => {
    expect([...OUTBOX_EVENT_TYPES]).toEqual([
      "OrderCreated",
      "ItemAdded",
      "ItemRemoved",
      "OrderConfirmed",
      "KOTCreated",
      "KOTStatusChanged",
      "OrderReady",
      "SentToKitchen",
      "OrderCancelled",
      "TableSessionOpened",
      "TableSessionUpdated",
    ]);
  });

  it("redacts exactly the credential fields Go redacts (ADR-011)", () => {
    expect([...AUDIT_REDACTED_FIELDS]).toEqual(["password_hash", "pin_hash", "token_hash"]);
  });

  it("no wire fixture carries credential material", () => {
    for (const name of ["app_user.json", "order.json", "table_session.json", "audit_event.json"]) {
      const serialized = JSON.stringify(loadFixture(name));
      for (const field of AUDIT_REDACTED_FIELDS) {
        expect(serialized).not.toContain(field);
      }
    }
  });
});
