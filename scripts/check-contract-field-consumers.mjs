#!/usr/bin/env node
// Fails the build when a field in the frozen contracts is read or written by
// NO consumer outside packages/contracts.
//
// WHY THIS EXISTS. Five instances in Milestone 4 alone, each found by hand,
// each after the fact:
//
//   menu_item_variant.is_default   0.5.0  added, read by nobody -- the POS
//                                         could not resolve a variant, so no
//                                         sale ever deducted stock
//   GetLedgerEntryBySeq                   declared, implemented, called by
//                                         nobody
//   printer_role                   0.4.7  shape landed, no delivery path
//   source_stock_count_id          0.5.5  sent by the edge, discarded by the
//                                         cloud, NULL in Postgres for every row
//   outlet.day_start_time          0.5.0  read, never written
//
// CLAUDE.md already states the rule -- "an ADDITIVE contract change has a
// consumer list too", "a column nothing reads is a column that does not
// exist". A documented obligation is not binding; this is the structural
// version, same shape as SINGLE_STORE_MIGRATIONS in
// edge/database/src/migrations.rs: a machine check with a declared exemption
// list, each exemption carrying a reason.
//
// WHAT IT DOES NOT CATCH -- MEASURED, NOT ASSUMED.
//
// This is a grep over identifiers, not a type-aware analysis. It catches the
// "declared in contracts, absent from every consumer" class, falsified against
// a probe field on 2026-08-27.
//
// It would NOT have caught any of the five above. Checked, not guessed:
// `is_default` appeared 3 times in edge/database/src/model.rs before the POS
// fix, so this check was green while the field was unread by resolution and no
// sale deducted stock. The other four are the same shape -- each existed in
// some Rust or Go struct while the path that mattered ignored it.
//
// The distinction it cannot make is DECLARED versus ACTED ON. A field
// mentioned only in a struct definition, a DTO and a seeder looks identical to
// one a resolver branches on. Closing that needs a per-surface check -- the
// field must appear in each required surface AND in at least one file that is
// not a model, DTO or fixture -- which is filed as follow-up in
// docs/RESUME.md §6 rather than half-built here.
//
// So: a floor, not the guard the five instances argue for. It makes the
// cheapest failure impossible and leaves the expensive one open.
//
// Usage: node scripts/check-contract-field-consumers.mjs [--report]
//   --report lists every unconsumed field without failing, for triage.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, extname } from "node:path";

const REPO = new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const CONTRACTS_TYPES = join(REPO, "packages/contracts/src/types");

// Directories searched for consumers. packages/contracts is deliberately
// excluded: a field referenced only by its own schema and its own fixtures is
// precisely the thing being hunted.
const CONSUMER_ROOTS = [
  "edge",
  "backend",
  "apps/pos/src",
  "apps/pos/src-tauri/src",
  "apps/kds/src",
  "tests",
];
const CONSUMER_EXTS = new Set([".rs", ".go", ".ts", ".tsx"]);

// ---------------------------------------------------------------- exemptions
// Every entry needs a reason. An exemption without one is not an exemption,
// it is a silenced failure.
const EXEMPT = new Map([
  [
    "unit_cost_paise",
    "Deferred to M5 procurement (ADR-018). Modelled now so the ledger shape " +
      "does not change when purchasing lands; nothing costs stock until then.",
  ],
  [
    "yield_factor_ppm",
    "Deferred to M5 (ADR-018). Trim/yield loss is authored with procurement; " +
      "YIELD_FACTOR_PPM_IDENTITY is the only value any current path uses.",
  ],
  [
    "schema_version",
    "Envelope discriminator present on every aggregate. Read by the drift " +
      "suites and pinned by Zod literals rather than branched on in product code.",
  ],
]);

// --------------------------------------------------------------------- scan
function walk(dir, out = []) {
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    return out;
  }
  for (const e of entries) {
    if (e === "node_modules" || e === "target" || e === ".git" || e === "dist") continue;
    const p = join(dir, e);
    let st;
    try {
      st = statSync(p);
    } catch {
      continue;
    }
    if (st.isDirectory()) walk(p, out);
    else out.push(p);
  }
  return out;
}

// Field names declared in the Zod schemas. Matches `  field_name: z.` at the
// two-space indent an object member sits at, which is how every schema in
// packages/contracts/src/types is written.
function contractFields() {
  const fields = new Map(); // name -> Set(file)
  for (const file of walk(CONTRACTS_TYPES)) {
    if (extname(file) !== ".ts") continue;
    const src = readFileSync(file, "utf8");
    for (const m of src.matchAll(/^ {2}([a-z][a-z0-9_]*):\s*z\./gm)) {
      const name = m[1];
      if (!fields.has(name)) fields.set(name, new Set());
      fields.get(name).add(file.slice(REPO.length).replace(/\\/g, "/"));
    }
  }
  return fields;
}

function consumerCorpus() {
  let blob = "";
  for (const root of CONSUMER_ROOTS) {
    for (const file of walk(join(REPO, root))) {
      if (!CONSUMER_EXTS.has(extname(file))) continue;
      blob += readFileSync(file, "utf8");
      blob += "\n";
    }
  }
  return blob;
}

const reportOnly = process.argv.includes("--report");
const fields = contractFields();
const corpus = consumerCorpus();

const unconsumed = [];
for (const [name, declaredIn] of fields) {
  if (EXEMPT.has(name)) continue;
  // Word-boundary match: `is_default` must not be satisfied by
  // `is_default_something`.
  if (new RegExp(`\\b${name}\\b`).test(corpus)) continue;
  unconsumed.push({ name, declaredIn: [...declaredIn] });
}

if (unconsumed.length === 0) {
  console.log(
    `contract field consumers: OK — ${fields.size} fields, ${EXEMPT.size} exempt, 0 unconsumed`,
  );
  process.exit(0);
}

console.error(
  `\ncontract field consumers: ${unconsumed.length} field(s) declared in packages/contracts ` +
    `and read or written by NO consumer:\n`,
);
for (const { name, declaredIn } of unconsumed) {
  console.error(`  ${name}`);
  for (const d of declaredIn) console.error(`      declared in ${d}`);
}
console.error(
  `\nA column nothing reads is a column that does not exist (CLAUDE.md).\n` +
    `Either wire a consumer, or add the field to EXEMPT in\n` +
    `scripts/check-contract-field-consumers.mjs WITH A REASON — a deliberate\n` +
    `deferral is fine, an undeclared one is the defect this check exists for.\n`,
);
process.exit(reportOnly ? 0 : 1);
