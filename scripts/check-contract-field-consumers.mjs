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
  // REMOVED at 0.6.0, and the removal is the point:
  //
  //   unit_cost_paise   -- now written by GRN receipt (grn_line.unit_cost_paise
  //                        feeds it) and read by weighted-average cost.
  //   yield_factor_ppm  -- now applied during receipt conversion.
  //
  // Both were "deferred to M5 procurement (ADR-018)". M5 landed them, so the
  // exemptions came out in the same change. AN EXEMPTION THAT OUTLIVES ITS
  // REASON IS A SILENCED FAILURE: left in place, these two would have gone on
  // suppressing a real finding the moment a later refactor dropped the
  // consumer, and nothing would have failed.
  //
  // If you are re-adding either of these, the question to answer first is
  // whether the consumer genuinely went away or whether you broke it.
  [
    "batch_code",
    "Deferred to M6 batch/expiry alerting (ADR-019). Modelled at 0.6.0 " +
      "because BATCH IDENTITY IS CAPTURED AT RECEIPT OR NEVER -- you cannot " +
      "retrofit which crate an ingredient came out of, so unlike most " +
      "deferred fields this one cannot wait for its consumer without losing " +
      "the data permanently. Written by GRN receipt, read by nothing until M6.",
  ],
  [
    "expiry_date",
    "Deferred to M6 batch/expiry alerting (ADR-019). Same argument as " +
      "batch_code: captured at receipt or never. Both exemptions come out " +
      "together when M6's expiry alerting lands.",
  ],
  [
    "schema_version",
    "Envelope discriminator present on every aggregate. Read by the drift " +
      "suites and pinned by Zod literals rather than branched on in product code.",
  ],
]);

// ------------------------------------------------------- in-flight, expiring
// A contract version lands BEFORE the builders that consume it — that is what
// contracts-first means (ADR-008), and it means every field of the milestone
// currently in flight is legitimately unconsumed for a few days.
//
// EXEMPT is the wrong tool for that. An EXEMPT entry never expires, so using
// it here would convert a whole milestone's fields into permanent silence and
// the one that never got wired would look exactly like the fourteen that did.
// That is the failure this script was written about, committed by the script.
//
// So: IN_FLIGHT is keyed by MILESTONE NUMBER and checked against
// .claude/current-milestone — the same marker CLAUDE.md's block is gated on.
//
//   marker === milestone   -> tolerated, listed as in-flight, exit 0
//   marker >  milestone    -> HARD FAILURE, named as an unfinished deliverable
//
// IT CANNOT OUTLIVE ITS REASON. Closing the milestone moves the marker, and
// the next CI run fails on anything still unwired. Nobody has to remember.
const IN_FLIGHT = new Map([
  [
    5,
    {
      reason:
        "ADR-019 contracts v0.6.0 procurement shapes. The contract lands " +
        "before T1-T6 build against it. Consumers arrive with the tracks; " +
        "this tolerance expires the moment .claude/current-milestone leaves 5.",
      fields: [
        // supplier / supplier_item (T1 cloud, T3 config push, T5 admin)
        "payment_terms_days", "pack_size_micro", "last_price_paise", "is_preferred",
        // purchase_order / line (T1, T5)
        "po_number", "expected_date", "total_paise", "approved_by_user_id",
        "approved_at", "ordered_quantity_micro", "unit_price_paise",
        "purchase_unit",
        // goods_receipt_note / grn_line (T2 edge, T4 POS)
        "purchase_order_id", "grn_number", "delivery_note_ref", "received_at",
        "received_by_user_id", "purchase_order_line_id",
        "entered_purchase_unit", "entered_quantity_micro",
        "base_quantity_micro", "pack_size_micro_applied",
        // grn_gap (T2, T4 — criterion 3 surfaces it)
        "grn_id", "grn_line_id",
        // Shared across several of the above; no prior contract shape uses
        // either name, so they surface here rather than under one table.
        "supplier_id", "line_number",
        // purchase_return (T2, T4)
        "purchase_return_id", "return_number", "returned_at",
        "returned_by_user_id",
        // stock_transfer_out (T2, T3)
        "stock_transfer_out_id", "destination_outlet_id", "transfer_number",
        "dispatched_at", "dispatched_by_user_id",
        // supplier_invoice / supplier_credit (T1 create+list only; M7 acts)
        "supplier_invoice_no", "due_date", "credit_note_no", "credit_date",
        // stock_ledger_entry provenance (T2 writes, T1 stores — 0.5.9's lesson)
        "source_grn_id", "source_purchase_return_id",
        "source_stock_transfer_out_id",
      ],
    },
  ],
]);

function currentMilestone() {
  try {
    const raw = readFileSync(join(REPO, ".claude/current-milestone"), "utf8").trim();
    if (!/^\d+$/.test(raw)) return null;
    return Number(raw);
  } catch {
    return null;
  }
}

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

const marker = currentMilestone();

// Flatten IN_FLIGHT into field -> milestone, so an entry left behind by a
// closed milestone is a hard failure rather than a quiet pass.
const inFlightMilestone = new Map();
for (const [milestone, { fields: names }] of IN_FLIGHT) {
  for (const n of names) inFlightMilestone.set(n, milestone);
}

const unconsumed = [];
const tolerated = [];
const expired = [];
for (const [name, declaredIn] of fields) {
  if (EXEMPT.has(name)) continue;
  // Word-boundary match: `is_default` must not be satisfied by
  // `is_default_something`.
  if (new RegExp(`\\b${name}\\b`).test(corpus)) continue;

  const milestone = inFlightMilestone.get(name);
  if (milestone !== undefined && marker !== null) {
    if (marker === milestone) {
      tolerated.push({ name, milestone });
      continue;
    }
    if (marker > milestone) {
      expired.push({ name, milestone, declaredIn: [...declaredIn] });
      continue;
    }
  }
  unconsumed.push({ name, declaredIn: [...declaredIn] });
}

// An IN_FLIGHT declaration whose milestone has CLOSED is the worst case this
// script handles: the field was promised a consumer, the milestone shipped,
// and nothing wired it. Report it separately and loudly — it is an unfinished
// deliverable, not a deferral.
if (expired.length > 0) {
  console.error(
    `\ncontract field consumers: ${expired.length} field(s) were declared IN_FLIGHT for a\n` +
      `milestone that has since CLOSED (.claude/current-milestone is now ${marker}):\n`,
  );
  for (const { name, milestone, declaredIn } of expired) {
    console.error(`  ${name}   (promised a consumer in M${milestone})`);
    for (const d of declaredIn) console.error(`      declared in ${d}`);
  }
  console.error(
    `\nThe milestone shipped and these were never wired. That is an UNFINISHED\n` +
      `DELIVERABLE, not a deferral: "not wired into X yet" is a gate failure.\n` +
      `Either wire the consumer, or move the field to EXEMPT with the NEW\n` +
      `milestone named and a reason that is true today.\n`,
  );
  process.exit(reportOnly ? 0 : 1);
}

if (unconsumed.length === 0) {
  const flight =
    tolerated.length > 0
      ? `, ${tolerated.length} in flight for M${marker}`
      : "";
  console.log(
    `contract field consumers: OK — ${fields.size} fields, ${EXEMPT.size} exempt${flight}, 0 unconsumed`,
  );
  if (tolerated.length > 0) {
    console.log(
      `  in flight (M${marker}, expires when the marker moves): ` +
        tolerated.map((t) => t.name).join(", "),
    );
  }
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
