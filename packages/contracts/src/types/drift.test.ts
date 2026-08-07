// Contract-drift test (ADR-008): fixtures under packages/contracts/fixtures/
// must parse cleanly through the Zod schemas and round-trip identically.
// The mirrored Go check lives in go/drift_test.go.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { CanonicalOrderSchema } from "./order";
import { KotSchema } from "./kot";
import { SyncEnvelopeSchema, AGGREGATE_AUTHORITY } from "./sync";

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
});
