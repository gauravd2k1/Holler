#!/usr/bin/env node
// Checks packages/contracts/openapi/openapi.yaml against the Go mirrors.
//
// WHY THIS EXISTS. Nothing machine-checked openapi.yaml. The TS↔Go drift test
// covers the two type mirrors and stops there, so the OpenAPI spec silently
// drifted on three MenuItem fields for two whole contract versions before a
// human noticed by reading it (docs/RESUME.md §5).
//
// A hand rewrite fixes today's copy and leaves the next version to drift
// identically. So: check it, and check it against the GO TYPES rather than
// against SQL. The Go structs are already drift-tested against the schema and
// against TypeScript, so this closes the last link and every hop is guarded:
//
//     SQL  ->  Go / TS  ->  OpenAPI
//     ^^^^^^^^^^^^^^^^      ^^^^^^^
//     existing drift test   this script
//
// Cheap, and it reuses what exists rather than adding a second SQL parser.
//
// SCOPE, stated so the guarantee is not overread: this compares FIELD NAME SETS
// for the declared schema/struct pairs. It does not compare types, formats,
// nullability or required-ness. A field present in both with the wrong type
// still passes. That is a real limit and it is the honest 80% — the drift that
// actually happened was three MISSING fields, which this catches.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const OPENAPI = join(repoRoot, "packages/contracts/openapi/openapi.yaml");
const GO_DIR = join(repoRoot, "packages/contracts/go");

// OpenAPI schema name -> Go struct name. Only pairs listed here are checked.
//
// A DECLARED LIST, not an auto-discovery, and deliberately so: an OpenAPI
// schema with no Go struct is often correct (request bodies, response
// wrappers), so auto-discovery would fail constantly and get switched off. The
// cost is that adding a pair is a manual step — which is why UNPAIRED_SCHEMAS
// below must account for every schema this list does not cover.
const PAIRS = {
  // Milestone 5 (0.6.0, ADR-019). Added WITH the schemas rather than after
  // them: this file's whole point is that an unpaired schema drifts silently.
  //
  // SupplierInvoice and SupplierCredit are deliberately unpaired — cloud-only,
  // modelled but not acted on until M7, and they appear on no route, so there
  // is no OpenAPI schema to pair. They join this list when M7 gives them one.
  Supplier: "Supplier",
  SupplierItem: "SupplierItem",
  PurchaseOrder: "PurchaseOrder",
  PurchaseOrderLine: "PurchaseOrderLine",
  GoodsReceiptNote: "GoodsReceiptNote",
  GrnLine: "GrnLine",
  GrnGap: "GrnGap",
  PurchaseReturn: "PurchaseReturn",
  PurchaseReturnLine: "PurchaseReturnLine",
  StockTransferOut: "StockTransferOut",
  StockTransferLine: "StockTransferLine",
  // Milestone 4 (0.5.0, ADR-018) — the milestone that added this check.
  InventoryItem: "InventoryItem",
  ItemUnitConversion: "ItemUnitConversion",
  Recipe: "Recipe",
  RecipeIngredient: "RecipeIngredient",
  ModifierIngredientDelta: "ModifierIngredientDelta",
  StockLedgerEntry: "StockLedgerEntry",
  StockCount: "StockCount",
  StockCountLine: "StockCountLine",
  StockDeductionGap: "StockDeductionGap",
  // Earlier milestones, retrofitted. MenuItem is first for a reason: it is the
  // schema that actually drifted.
  MenuItem: "MenuItem",
  Station: "Station",
  MenuItemStation: "MenuItemStation",
  Printer: "Printer",
  StationPrinter: "StationPrinter",
  PrinterRole: "PrinterRole",
  Kot: "Kot",
  Invoice: "Invoice",
  Payment: "Payment",
  CashShift: "CashShift",
  TaxProfile: "TaxProfile",
  RestaurantTable: "RestaurantTable",
  TableSession: "TableSession",
  AppUser: "AppUser",
};

const fail = (message) => {
  console.error(`check-openapi-go-drift: ${message}`);
  process.exit(1);
};

const read = (path) => {
  try {
    return readFileSync(path, "utf8");
  } catch (error) {
    // Loudly, never skipping: a check that passes when it cannot find its
    // inputs is the failure it exists to prevent.
    fail(`cannot read ${path}: ${error.message}`);
  }
};

// --- Go: struct name -> set of json tag names -------------------------------
//
// Hand-rolled rather than a parser dependency. The shape it must handle is
// narrow: `type Name struct {` ... `Field Type \`json:"name"\`` ... `}`.
function goStructFields(source) {
  const structs = new Map();
  const lines = source.split("\n");
  let current = null;

  for (const line of lines) {
    const open = line.match(/^type\s+(\w+)\s+struct\s*\{/);
    if (open) {
      current = open[1];
      structs.set(current, new Set());
      continue;
    }
    if (current && /^\}/.test(line)) {
      current = null;
      continue;
    }
    if (!current) continue;

    const tag = line.match(/json:"([^",]+)/);
    if (tag && tag[1] !== "-") structs.get(current).add(tag[1]);
  }
  return structs;
}

const goSources = ["identity", "invoice", "inventory", "kot", "menu", "order", "payment", "printer", "procurement", "station", "sync", "table", "tax"]
  .map((name) => read(join(GO_DIR, `${name}.go`)))
  .join("\n");
const goStructs = goStructFields(goSources);
if (goStructs.size === 0) fail("parsed zero Go structs — the parser or the path is wrong, not the contracts");

// --- OpenAPI: schema name -> set of property names --------------------------
//
// An indentation scanner over `components: schemas:` only. It does not attempt
// general YAML: it finds a schema at 4 spaces, its `properties:` at 6, and
// takes every key at 8. Anything it cannot find is an error, never a skip.
function openapiSchemaProperties(source) {
  const lines = source.split("\n");
  const schemas = new Map();

  let inSchemas = false;
  let schema = null;
  let inProperties = false;

  for (const line of lines) {
    if (/^ {2}schemas:\s*$/.test(line)) {
      inSchemas = true;
      continue;
    }
    if (!inSchemas) continue;
    // A new top-level key ends the components block.
    if (/^\S/.test(line) && line.trim() !== "") break;

    const schemaStart = line.match(/^ {4}(\w+):\s*$/);
    if (schemaStart) {
      schema = schemaStart[1];
      schemas.set(schema, new Set());
      inProperties = false;
      continue;
    }
    if (!schema) continue;

    if (/^ {6}properties:\s*$/.test(line)) {
      inProperties = true;
      continue;
    }
    // Any other 6-space key ends the properties block for this schema.
    if (/^ {6}\S/.test(line)) {
      inProperties = false;
      continue;
    }
    if (!inProperties) continue;

    const property = line.match(/^ {8}(\w+):/);
    if (property) schemas.get(schema).add(property[1]);
  }
  return schemas;
}

const openapiSchemas = openapiSchemaProperties(read(OPENAPI));
if (openapiSchemas.size === 0) fail("parsed zero OpenAPI schemas — the scanner or the path is wrong");

// --- Compare -----------------------------------------------------------------

const problems = [];

for (const [schemaName, structName] of Object.entries(PAIRS)) {
  const schemaFields = openapiSchemas.get(schemaName);
  const structFields = goStructs.get(structName);

  if (!schemaFields) {
    problems.push(`OpenAPI has no schema "${schemaName}" (paired with Go ${structName})`);
    continue;
  }
  if (!structFields) {
    problems.push(`Go has no struct "${structName}" (paired with OpenAPI ${schemaName})`);
    continue;
  }

  const missingFromOpenapi = [...structFields].filter((f) => !schemaFields.has(f));
  const missingFromGo = [...schemaFields].filter((f) => !structFields.has(f));

  if (missingFromOpenapi.length) {
    problems.push(
      `${schemaName}: in Go ${structName} but NOT in the OpenAPI schema — ${missingFromOpenapi.join(", ")}`,
    );
  }
  if (missingFromGo.length) {
    problems.push(
      `${schemaName}: in the OpenAPI schema but NOT in Go ${structName} — ${missingFromGo.join(", ")}`,
    );
  }
}

if (problems.length) {
  console.error("check-openapi-go-drift: OPENAPI HAS DRIFTED FROM THE GO MIRRORS\n");
  for (const problem of problems) console.error(`  • ${problem}`);
  console.error(
    "\nThe Go structs are drift-tested against the schema and against TypeScript, so they are\n" +
      "the reference here. Fix openapi.yaml, or add the field to Go if the spec is right.\n" +
      "This spec drifted on three MenuItem fields for two contract versions because nothing\n" +
      "checked it.",
  );
  process.exit(1);
}

console.log(
  `check-openapi-go-drift: ok — ${Object.keys(PAIRS).length} schema/struct pairs agree on field names`,
);
