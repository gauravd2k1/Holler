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
