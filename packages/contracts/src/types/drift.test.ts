// Contract-drift test (ADR-008): fixtures under packages/contracts/fixtures/
// must parse cleanly through the Zod schemas and round-trip identically.
// The mirrored Go check lives in go/drift_test.go.
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import { CanonicalOrderSchema } from "./order";
import { KotSchema } from "./kot";
import { SyncEnvelopeSchema, AGGREGATE_AUTHORITY, AggregateTypeSchema } from "./sync";
import {
  AppUserSchema,
  AuditEventSchema,
  PermissionSchema,
  AUDIT_REDACTED_FIELDS,
  EdgeUserCacheEntrySchema,
  EdgeDeviceCredentialSchema,
} from "./identity";
import { RestaurantTableSchema, TableSessionSchema } from "./table";
import { MenuItemSchema, MenuItemModifierSchema } from "./menu";
import { StationSchema, MenuItemStationSchema } from "./station";
import {
  PrinterSchema,
  StationPrinterSchema,
  PrinterRoleSchema,
  PrintJobSchema,
} from "./printer";
import { OUTBOX_EVENT_TYPES } from "./events";
import { TaxProfileSchema } from "./tax";
import { InvoiceSchema } from "./invoice";
import { PaymentSchema, CashShiftSchema } from "./payment";
// Milestone 4 additions (0.5.0, ADR-018).
import {
  InventoryItemSchema,
  ItemUnitConversionSchema,
  RecipeSchema,
  RecipeIngredientSchema,
  ModifierIngredientDeltaSchema,
  StockLedgerEntrySchema,
  StockCountSchema,
  StockCountLineSchema,
  StockDeductionGapSchema,
  DIMENSIONAL_CONVERSIONS,
  YIELD_FACTOR_PPM_IDENTITY,
} from "./inventory";
// Milestone 5 additions (0.6.0, ADR-019).
import {
  SupplierSchema,
  SupplierItemSchema,
  PurchaseOrderSchema,
  PurchaseOrderStatusSchema,
  GoodsReceiptNoteSchema,
  GrnGapSchema,
  PurchaseReturnSchema,
  StockTransferOutSchema,
} from "./procurement";

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

  // 0.4.7. BILL deliberately, not KITCHEN: KITCHEN would round-trip even if the
  // enum were mis-mirrored as a plain string, since station_printer already
  // pins the join-row shape. BILL is the member that exists only because an
  // invoice needs a print target.
  it("printer_role.json round-trips through PrinterRoleSchema", () => {
    const raw = loadFixture("printer_role.json");
    const parsed = PrinterRoleSchema.parse(raw);
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
      "ItemQuantityChanged",
      "OrderConfirmed",
      "KOTCreated",
      "KOTStatusChanged",
      "OrderReady",
      "SentToKitchen",
      "OrderCancelled",
      "TableSessionOpened",
      "TableSessionUpdated",
      "InvoiceCreated",
      "PaymentReceived",
      "PaymentRefunded",
      "CashShiftOpened",
      "CashShiftClosed",
      // Milestone 4 (0.5.5): a completed stocktake is an individually
      // meaningful, low-volume fact, so it rides the outbox while the ledger
      // rides the entry_seq cursor.
      "StockCountOpened",
      "StockCountCompleted",
      // Milestone 5 (0.6.1): the four procurement facts. Same cut as the
      // stocktake above -- discrete, individually meaningful, low-volume.
      "GoodsReceived",
      "GrnGapRecorded",
      "PurchaseReturned",
      "StockDispatched",
    ]);
  });

  // The device credential hash is the second deliberate credential carrier,
  // after EdgeUserCacheEntry. Both hash states are pinned because nullable
  // handling is exactly where a mirror silently drops a field.
  it("edge_device_credential.json round-trips with its hash intact", () => {
    const raw = loadFixture("edge_device_credential.json") as Record<string, unknown>;
    const parsed = EdgeDeviceCredentialSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
    expect(parsed.token_hash).toBe(raw.token_hash);
  });

  // A revoked credential must still be representable: the edge learns a
  // credential is dead by syncing it, not by its absence. Absence is
  // indistinguishable from "not yet synced" while the uplink is down.
  it("represents a revoked device credential rather than omitting it", () => {
    const raw = loadFixture("edge_device_credential.json") as Record<string, unknown>;
    const revoked = EdgeDeviceCredentialSchema.parse({
      ...raw,
      revoked_at: "2026-08-13T09:00:00Z",
    });
    expect(revoked.revoked_at).toBe("2026-08-13T09:00:00Z");
  });

  it("invoice.json round-trips through InvoiceSchema", () => {
    const raw = loadFixture("invoice.json");
    const parsed = InvoiceSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  it("payment.json round-trips through PaymentSchema", () => {
    const raw = loadFixture("payment.json");
    const parsed = PaymentSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  it("cash_shift.json round-trips through CashShiftSchema", () => {
    const raw = loadFixture("cash_shift.json");
    const parsed = CashShiftSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  it("tax_profile.json round-trips through TaxProfileSchema", () => {
    const raw = loadFixture("tax_profile.json");
    const parsed = TaxProfileSchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
  });

  // Milestone 3 (ADR-016). The outlet issues bills and takes money offline, so
  // both are edge-authoritative; the rules governing them are management
  // decisions, so those are cloud config. Same cut as station/kot.
  it("Milestone 3 billing authority follows §50.1", () => {
    expect(AGGREGATE_AUTHORITY.invoice).toBe("EDGE_TO_CLOUD");
    expect(AGGREGATE_AUTHORITY.cash_shift).toBe("EDGE_TO_CLOUD");
    expect(AGGREGATE_AUTHORITY.payment).toBe("EDGE_TO_CLOUD");
    expect(AGGREGATE_AUTHORITY.tax_profile).toBe("CLOUD_TO_EDGE");
    expect(AGGREGATE_AUTHORITY.compliance_version).toBe("CLOUD_TO_EDGE");
    expect(AGGREGATE_AUTHORITY.invoice_series).toBe("CLOUD_TO_EDGE");
    expect(AGGREGATE_AUTHORITY.discount_definition).toBe("CLOUD_TO_EDGE");
  });

  // invoice_sequence is the counter behind a series. Giving it a sync
  // direction would make the cloud a second writer of invoice numbers, which
  // is exactly what §33's "never generate duplicate invoice numbers" forbids.
  // The print_job precedent, applied to numbering. Child rows are absent for
  // the ordinary reason: they travel inside their parent's payload.
  it("keeps the invoice counter and billing child rows out of AggregateType", () => {
    expect(AggregateTypeSchema.options).not.toContain("invoice_sequence");
    expect(AggregateTypeSchema.options).not.toContain("invoice_line");
    expect(AggregateTypeSchema.options).not.toContain("payment_allocation");
    expect(AggregateTypeSchema.options).not.toContain("cash_movement");
    expect(AggregateTypeSchema.options).not.toContain("tax_rule");
  });

  // The ADR-016 rounding policy, asserted at the type layer. It is also a
  // CHECK in sqlite/0006 and postgres/0007 — three layers, because a bill that
  // does not add up must be unrepresentable everywhere, not merely untested.
  it("rejects a bill whose grand total does not equal its parts", () => {
    const bill = loadFixture("invoice.json") as Record<string, unknown>;
    expect(() =>
      InvoiceSchema.parse({ ...bill, grand_total_paise: 106000 }),
    ).toThrow();
  });

  it("rejects a round-off larger than half a rupee", () => {
    const bill = loadFixture("invoice.json") as Record<string, unknown>;
    // Keep the sum self-consistent so ONLY the round-off bound can fail.
    expect(() =>
      InvoiceSchema.parse({
        ...bill,
        taxable_value_paise: 99940,
        round_off_paise: 60,
        grand_total_paise: 105000,
      }),
    ).toThrow();
  });

  it("rejects a grand total that does not settle in whole rupees", () => {
    const bill = loadFixture("invoice.json") as Record<string, unknown>;
    expect(() =>
      InvoiceSchema.parse({
        ...bill,
        taxable_value_paise: 99999,
        round_off_paise: 0,
        grand_total_paise: 104999,
      }),
    ).toThrow();
  });

  it("rejects cash-drawer fields on a non-cash tender", () => {
    const pay = loadFixture("payment.json") as Record<string, unknown>;
    expect(() => PaymentSchema.parse({ ...pay, method: "UPI" })).toThrow();
  });

  it("rejects a closed shift with no counted cash", () => {
    const shift = loadFixture("cash_shift.json") as Record<string, unknown>;
    expect(() =>
      CashShiftSchema.parse({ ...shift, status: "CLOSED", actual_cash_paise: null }),
    ).toThrow();
  });

  it("redacts exactly the credential fields Go redacts (ADR-011)", () => {
    expect([...AUDIT_REDACTED_FIELDS]).toEqual([
      "password_hash",
      "pin_hash",
      "token_hash",
      "device_token_hash",
      "credential_hash",
    ]);
  });

  // EdgeUserCacheEntry (0.3.1, ADR-015) — the one deliberate credential
  // carrier. Both hash states are pinned because nullable handling is exactly
  // where a mirror silently drops a field, and until now that was a
  // read-verified claim rather than an executed one.
  it("edge_user_cache_entry.json round-trips with both hashes intact", () => {
    const raw = loadFixture("edge_user_cache_entry.json") as Record<string, unknown>;
    const parsed = EdgeUserCacheEntrySchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
    expect(parsed.password_hash).toBe(raw.password_hash);
    expect(parsed.pin_hash).toBe(raw.pin_hash);
    expect(parsed.pin_hash).toMatch(/^\$argon2id\$/);
  });

  // A PIN pad is the primary offline login at a POS, so the null case is not
  // an edge case — it is every user who has not set one.
  it("edge_user_cache_entry_no_pin.json round-trips with pin_hash null", () => {
    const raw = loadFixture("edge_user_cache_entry_no_pin.json") as Record<string, unknown>;
    const parsed = EdgeUserCacheEntrySchema.parse(raw);
    expect(JSON.parse(JSON.stringify(parsed))).toEqual(raw);
    expect(parsed.pin_hash).toBeNull();
    // Present-and-null, never dropped: a mirror that omits the key round-trips
    // to a different object, and this is the assertion that catches it.
    expect(Object.hasOwn(JSON.parse(JSON.stringify(parsed)), "pin_hash")).toBe(true);
  });

  // The cache entry never becomes an aggregate: no sync direction, never
  // edge→cloud. Same reasoning as print_job and refresh_token.
  it("keeps the edge user cache out of AggregateType", () => {
    expect(AggregateTypeSchema.options).not.toContain("edge_user_cache_entry");
    expect(AggregateTypeSchema.options).not.toContain("app_user_credential");
  });

  it("no wire fixture carries credential material", () => {
    // Every fixture EXCEPT the deliberate carriers. Naming them as exceptions
    // rather than skipping the check keeps the rule enforceable: a second
    // credential-bearing fixture fails here until someone justifies it in an
    // ADR. Sweeping the whole directory also means a NEW fixture is covered
    // automatically, which the previous hard-coded four-name list did not do.
    const CREDENTIAL_BEARING = new Set([
      "edge_user_cache_entry.json",
      "edge_user_cache_entry_no_pin.json",
      // Added at 0.4.3 and justified in the ADR-017 amendment, exactly as this
      // guard demands. The device credential hash syncs to an enrolled edge so
      // a LAN handshake can be verified with the uplink down — the ADR-011
      // pattern applied to devices, for the same offline-first reason.
      "edge_device_credential.json",
    ]);
    const all = readdirSync(resolve(__dirname, "../../fixtures")).filter((f) => f.endsWith(".json"));
    expect(all.length).toBeGreaterThan(CREDENTIAL_BEARING.size); // the sweep is not vacuous
    for (const name of all.filter((f) => !CREDENTIAL_BEARING.has(f))) {
      const serialized = JSON.stringify(loadFixture(name));
      for (const field of AUDIT_REDACTED_FIELDS) {
        expect(serialized).not.toContain(field);
      }
    }
  });

  // The exception is exactly as wide as it claims to be: the user carriers hold
  // password_hash and pin_hash, and never token_hash or any bearer material.
  it("the credential carriers hold verifiers only, never a token", () => {
    for (const name of ["edge_user_cache_entry.json", "edge_user_cache_entry_no_pin.json"]) {
      const serialized = JSON.stringify(loadFixture(name));
      expect(serialized).not.toContain("token_hash");
      expect(serialized).not.toContain("refresh_token");
      expect(serialized).not.toContain("access_token");
      expect(serialized).not.toContain("session");
    }
  });

  // The device carrier is held to the same standard, stated for its own field
  // names. Its column is spelled token_hash, but the VALUE must be an Argon2id
  // verifier — something you check a presented token against — never a bearer
  // token you could replay. If a plaintext token ever landed in this field the
  // hash prefix would be the first thing to go.
  it("the device credential carrier holds an Argon2id verifier, not a bearer token", () => {
    const cred = loadFixture("edge_device_credential.json") as Record<string, unknown>;
    expect(String(cred.credential_hash)).toMatch(/^\$argon2id\$/);
    const serialized = JSON.stringify(cred);
    expect(serialized).not.toContain("refresh_token");
    expect(serialized).not.toContain("access_token");
    expect(serialized).not.toContain("plaintext");
  });
});

// ---------------------------------------------------------------------------
// Milestone 4 — inventory and recipes (0.5.0, ADR-018)
// ---------------------------------------------------------------------------

describe("Milestone 4 inventory contracts", () => {
  const fixture = (name: string) => loadFixture(name) as Record<string, unknown>;

  it("every fixture parses against its schema", () => {
    expect(() => InventoryItemSchema.parse(fixture("inventory_item.json"))).not.toThrow();
    expect(() => ItemUnitConversionSchema.parse(fixture("item_unit_conversion.json"))).not.toThrow();
    expect(() => RecipeSchema.parse(fixture("recipe.json"))).not.toThrow();
    expect(() => RecipeIngredientSchema.parse(fixture("recipe_ingredient.json"))).not.toThrow();
    expect(() =>
      ModifierIngredientDeltaSchema.parse(fixture("modifier_ingredient_delta.json")),
    ).not.toThrow();
    expect(() => StockLedgerEntrySchema.parse(fixture("stock_ledger_entry.json"))).not.toThrow();
    expect(() =>
      StockLedgerEntrySchema.parse(fixture("stock_ledger_entry_count_adjustment.json")),
    ).not.toThrow();
    // 0.6.3: the COSTED group. Both other ledger fixtures are null in both
    // money columns, and a null round-trips through a dropped field perfectly.
    expect(() =>
      StockLedgerEntrySchema.parse(fixture("stock_ledger_entry_goods_receipt.json")),
    ).not.toThrow();
    expect(() => StockCountSchema.parse(fixture("stock_count.json"))).not.toThrow();
    expect(() => StockCountLineSchema.parse(fixture("stock_count_line.json"))).not.toThrow();
    expect(() =>
      StockDeductionGapSchema.parse(fixture("stock_deduction_gap.json")),
    ).not.toThrow();
  });

  // A COUNT_ADJUSTMENT's link to the count that produced it, PARSED rather
  // than merely accepted: Zod strips unknown keys silently, so a field missing
  // from the schema makes `.parse` pass and the value vanish -- the same shape
  // as the cloud's lenient json.Unmarshal, which dropped this field for two
  // milestones. The recipe fixture cannot catch it: null round-trips through
  // an absent field perfectly (contracts 0.5.9).
  it("keeps a count-sourced entry's provenance through a parse", () => {
    const parsed = StockLedgerEntrySchema.parse(
      fixture("stock_ledger_entry_count_adjustment.json"),
    );
    expect(parsed.source_stock_count_id).toBe(
      fixture("stock_ledger_entry_count_adjustment.json").source_stock_count_id,
    );
  });

  it("assigns §50.1 authority the same way every milestone before it did", () => {
    expect(AGGREGATE_AUTHORITY.inventory_item).toBe("CLOUD_TO_EDGE");
    expect(AGGREGATE_AUTHORITY.recipe).toBe("CLOUD_TO_EDGE");
    expect(AGGREGATE_AUTHORITY.stock_ledger_entry).toBe("EDGE_TO_CLOUD");
    expect(AGGREGATE_AUTHORITY.stock_count).toBe("EDGE_TO_CLOUD");
    expect(AGGREGATE_AUTHORITY.stock_deduction_gap).toBe("EDGE_TO_CLOUD");
  });

  // The cloud may re-derive its own stock view by summing the ingested ledger.
  // Mirroring the edge's projection would make it a second authority on stock —
  // the mistake mirroring invoice_sequence would make about invoice numbers.
  it("keeps the edge-local projection and the child rows out of AggregateType", () => {
    for (const forbidden of [
      "stock_balance_snapshot",
      "item_unit_conversion",
      "recipe_ingredient",
      "modifier_ingredient_delta",
      "stock_count_line",
    ]) {
      expect(AggregateTypeSchema.options).not.toContain(forbidden);
    }
  });

  // Rule 1: stock never blocks a sale. A negative balance is a variance signal,
  // and no schema may quietly enforce non-negative.
  it("accepts a negative applied quantity", () => {
    const entry = fixture("stock_ledger_entry.json");
    expect(() =>
      StockLedgerEntrySchema.parse({ ...entry, quantity_applied_micro: -999_999_999 }),
    ).not.toThrow();
  });

  it("rejects a micro-quantity beyond Number.MAX_SAFE_INTEGER", () => {
    const entry = fixture("stock_ledger_entry.json");
    expect(() =>
      StockLedgerEntrySchema.parse({
        ...entry,
        quantity_applied_micro: Number.MAX_SAFE_INTEGER + 2,
      }),
    ).toThrow();
  });

  it("requires exactly one provenance group, keyed on origin", () => {
    const entry = fixture("stock_ledger_entry.json");
    // RECIPE row carrying a modifier delta as well.
    expect(() =>
      StockLedgerEntrySchema.parse({ ...entry, modifier_delta_id: entry.recipe_id }),
    ).toThrow();
    // RECIPE row carrying no recipe.
    expect(() =>
      StockLedgerEntrySchema.parse({ ...entry, recipe_id: null }),
    ).toThrow();
    // WASTAGE row must carry neither.
    expect(() =>
      StockLedgerEntrySchema.parse({ ...entry, origin: "WASTAGE" }),
    ).toThrow();
    // MODIFIER_DELTA row with its delta and no recipe.
    expect(() =>
      StockLedgerEntrySchema.parse({
        ...entry,
        origin: "MODIFIER_DELTA",
        recipe_id: null,
        recipe_version: null,
        recipe_name: null,
        modifier_delta_id: entry.source_order_id,
        modifier_name: "Extra Paneer",
        modifier_delta_version: 7,
      }),
    ).not.toThrow();
  });

  it("requires exactly one recipe component, and refuses self-reference", () => {
    const ing = fixture("recipe_ingredient.json");
    expect(() =>
      RecipeIngredientSchema.parse({ ...ing, sub_recipe_id: ing.recipe_id }),
    ).toThrow();
    expect(() =>
      RecipeIngredientSchema.parse({ ...ing, inventory_item_id: null }),
    ).toThrow();
    expect(() =>
      RecipeIngredientSchema.parse({
        ...ing,
        component_kind: "SUB_RECIPE",
        inventory_item_id: null,
        sub_recipe_id: ing.recipe_id,
      }),
    ).toThrow();
  });

  // Two sources of truth for kg→g would need a silent precedence rule between
  // disagreeing numbers, which is how a deduction becomes quietly wrong.
  it("refuses a pack label that collides with the frozen dimensional map", () => {
    const conv = fixture("item_unit_conversion.json");
    for (const reserved of ["kg", "KG", "g", "ml", "litre", "dozen"]) {
      expect(() =>
        ItemUnitConversionSchema.parse({ ...conv, pack_unit_label: reserved }),
      ).toThrow();
    }
    expect(() =>
      ItemUnitConversionSchema.parse({ ...conv, pack_unit_label: "crate" }),
    ).not.toThrow();
  });

  // These are physical constants and must match the Go mirror exactly.
  it("holds within-dimension conversions only", () => {
    expect(DIMENSIONAL_CONVERSIONS.kg).toEqual({ dimension: "MASS", micro: 1_000_000_000 });
    expect(DIMENSIONAL_CONVERSIONS.ml).toEqual({ dimension: "VOLUME", micro: 1_000 });
    expect(DIMENSIONAL_CONVERSIONS.dozen).toEqual({ dimension: "COUNT", micro: 12_000_000 });
    expect(Object.keys(DIMENSIONAL_CONVERSIONS)).toHaveLength(7);
  });

  // Deferred columns stay inert in M4, pinned by exact assertion.
  it("keeps the deferred fields inert", () => {
    expect(fixture("inventory_item.json").yield_factor_ppm).toBe(YIELD_FACTOR_PPM_IDENTITY);
    expect(fixture("recipe_ingredient.json").yield_factor_ppm).toBe(YIELD_FACTOR_PPM_IDENTITY);
    expect(fixture("stock_ledger_entry.json").unit_cost_paise).toBeNull();
  });

  // wastage.approve is deliberately absent: an unused permission is a
  // documented obligation dressed as structural enforcement.
  it("adds the M4 permissions and not wastage.approve", () => {
    expect(PermissionSchema.options).toContain("inventory.manage");
    expect(PermissionSchema.options).toContain("inventory.count");
    expect(PermissionSchema.options).toContain("recipe.manage");
    expect(PermissionSchema.options).toContain("billing.manage");
    expect(PermissionSchema.options).not.toContain("wastage.approve");
  });

  // -------------------------------------------------------------------------
  // Milestone 5 — procurement (0.6.0, ADR-019)
  // -------------------------------------------------------------------------

  it("supplier.json round-trips through SupplierSchema", () => {
    const raw = loadFixture("supplier.json");
    expect(JSON.parse(JSON.stringify(SupplierSchema.parse(raw)))).toEqual(raw);
  });

  it("supplier_item.json round-trips through SupplierItemSchema", () => {
    const raw = loadFixture("supplier_item.json");
    expect(JSON.parse(JSON.stringify(SupplierItemSchema.parse(raw)))).toEqual(raw);
  });

  it("purchase_order.json round-trips through PurchaseOrderSchema", () => {
    const raw = loadFixture("purchase_order.json");
    expect(JSON.parse(JSON.stringify(PurchaseOrderSchema.parse(raw)))).toEqual(raw);
  });

  it("goods_receipt_note.json round-trips through GoodsReceiptNoteSchema", () => {
    const raw = loadFixture("goods_receipt_note.json");
    expect(JSON.parse(JSON.stringify(GoodsReceiptNoteSchema.parse(raw)))).toEqual(raw);
  });

  it("goods_receipt_note_no_po.json round-trips through GoodsReceiptNoteSchema", () => {
    const raw = loadFixture("goods_receipt_note_no_po.json");
    expect(JSON.parse(JSON.stringify(GoodsReceiptNoteSchema.parse(raw)))).toEqual(raw);
  });

  it("grn_gap.json round-trips through GrnGapSchema", () => {
    const raw = loadFixture("grn_gap.json");
    expect(JSON.parse(JSON.stringify(GrnGapSchema.parse(raw)))).toEqual(raw);
  });

  it("purchase_return.json round-trips through PurchaseReturnSchema", () => {
    const raw = loadFixture("purchase_return.json");
    expect(JSON.parse(JSON.stringify(PurchaseReturnSchema.parse(raw)))).toEqual(raw);
  });

  it("stock_transfer_out.json round-trips through StockTransferOutSchema", () => {
    const raw = loadFixture("stock_transfer_out.json");
    expect(JSON.parse(JSON.stringify(StockTransferOutSchema.parse(raw)))).toEqual(raw);
  });

  // A GRN NEVER BLOCKS ON A PO. This nullability is load-bearing, not laxity:
  // goods arrive against an unsynced PO, against one amended after dispatch,
  // and with no PO at all, and the receipt is accepted every time. If someone
  // tidies the schema up by requiring a link, this is what stops them.
  it("accepts a receipt with no purchase order, no PO line and no supplier", () => {
    const grn = GoodsReceiptNoteSchema.parse(loadFixture("goods_receipt_note_no_po.json"));
    expect(grn.purchase_order_id).toBeNull();
    expect(grn.supplier_id).toBeNull();
    expect(grn.lines[0].purchase_order_line_id).toBeNull();
  });

  // Contracts 0.5.9's lesson, applied before the hole exists rather than after:
  // a fidelity test proves fidelity only for the fields its fixture POPULATES,
  // and a null round-trips through a nonexistent field perfectly. Every
  // provenance field is therefore non-null in the linked fixture, and the null
  // case lives in its own fixture above rather than hiding inside this one.
  it("populates every provenance field on the linked GRN fixture", () => {
    const line = GoodsReceiptNoteSchema.parse(loadFixture("goods_receipt_note.json")).lines[0];
    const provenance = [
      "purchase_order_line_id",
      "entered_purchase_unit",
      "entered_quantity_micro",
      "quantity_dimension",
      "base_quantity_micro",
      "pack_size_micro_applied",
      "unit_cost_paise",
      "batch_code",
      "expiry_date",
    ] as const;
    for (const field of provenance) {
      expect(line[field], field + " must be populated, or it is untested").not.toBeNull();
    }
  });

  // The conversion happens exactly once, at the edge, and BOTH sides are
  // stored. When a receipt turns out 1000x wrong, "what did they actually
  // type?" must be answerable from the row, not reconstructed from a pack size
  // that may since have been edited.
  it("stores both sides of the purchase-unit conversion, consistently", () => {
    const line = GoodsReceiptNoteSchema.parse(loadFixture("goods_receipt_note.json")).lines[0];
    expect(line.base_quantity_micro).toBe(
      (line.entered_quantity_micro * line.pack_size_micro_applied) / 1_000_000,
    );
    // The binding range limit is JavaScript's 2^53, not i64 (0.5.0's rule).
    expect(line.base_quantity_micro).toBeLessThan(Number.MAX_SAFE_INTEGER);
  });

  // PLAIN OUTBOX. A grn_gap is a discrete event a buyer acts on, not a
  // per-sale row arriving all day — so it gets none of stock_deduction_gap's
  // 0.5.8 ranged-stream machinery, and an entry_seq here would be transport
  // cost cargo-culted onto a shape that does not pay it.
  it("keeps grn_gap a plain outbox with no sequence field", () => {
    const raw = loadFixture("grn_gap.json") as Record<string, unknown>;
    expect(raw).not.toHaveProperty("entry_seq");
    expect(Object.keys(GrnGapSchema.parse(raw))).not.toContain("entry_seq");
    // stock_deduction_gap, by contrast, still carries one.
    expect(loadFixture("stock_deduction_gap.json")).toHaveProperty("entry_seq");
  });

  // NO RECEIPT STATE on the PO. Receipt progress is derived on both sides and
  // the two derivations LEGITIMATELY DIFFER — the edge sees only its own GRN
  // lines, the cloud sees every outlet's. A status member here would make the
  // outlet a second writer of a cloud-owned row (§50.1).
  it("keeps receipt state off purchase_order", () => {
    expect(PurchaseOrderStatusSchema.options).not.toContain("PARTIALLY_RECEIVED");
    expect(PurchaseOrderStatusSchema.options).not.toContain("RECEIVED");
    const po = loadFixture("purchase_order.json") as Record<string, unknown>;
    expect(po).not.toHaveProperty("received_quantity_micro");
    expect(po).not.toHaveProperty("receipt_status");
  });

  it("gives the M5 aggregates their §50.1 authority, and no others", () => {
    expect(AGGREGATE_AUTHORITY.supplier).toBe("CLOUD_TO_EDGE");
    expect(AGGREGATE_AUTHORITY.purchase_order).toBe("CLOUD_TO_EDGE");
    expect(AGGREGATE_AUTHORITY.goods_receipt_note).toBe("EDGE_TO_CLOUD");
    expect(AGGREGATE_AUTHORITY.grn_gap).toBe("EDGE_TO_CLOUD");
    expect(AGGREGATE_AUTHORITY.purchase_return).toBe("EDGE_TO_CLOUD");
    expect(AGGREGATE_AUTHORITY.stock_transfer_out).toBe("EDGE_TO_CLOUD");

    // Child rows travel inside their parent. Counters never leave the outlet
    // (invoice_sequence). Supplier accounts are cloud-only (refresh_token).
    const absent = [
      "supplier_item",
      "purchase_order_line",
      "grn_line",
      "purchase_return_line",
      "stock_transfer_line",
      "grn_sequence",
      "supplier_invoice",
      "supplier_credit",
    ];
    for (const name of absent) {
      expect(AggregateTypeSchema.options).not.toContain(name);
    }
  });

  // Both land WITH their enforced checks in this milestone. wastage.approve
  // does not, for the second milestone running — adding it here would repeat
  // the billing.manage defect verbatim in the change that fixes it.
  it("adds the M5 permissions and still not wastage.approve", () => {
    expect(PermissionSchema.options).toContain("procurement.manage");
    expect(PermissionSchema.options).toContain("procurement.approve");
    expect(PermissionSchema.options).not.toContain("wastage.approve");
  });
});
