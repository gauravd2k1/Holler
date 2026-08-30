#!/usr/bin/env node
// Every config family the CLOUD SENDS must be one the EDGE APPLIES.
//
// WHY THIS EXISTS, AND WHY THE EXISTING GUARD DID NOT CATCH IT.
//
// `TestSyncConfigGuard_EveryCloudAuthoritativeColumnIsWiredOrExempted`
// (backend/cmd/api/syncconfig_guard_test.go) already holds every column of
// every cloud-authoritative table to account. At 0.6.0 it was extended with
// supplier, supplier_item, purchase_order and purchase_order_line, the four
// arrays were added to `syncConfigResponse`, and the guard PASSED — correctly,
// on its own terms.
//
// The data still never reached a single outlet.
//
// That guard reflects over the Go response struct and asks: "is this column
// somewhere in the cloud's wire shape?" It never asks whether anything at the
// far end reads it. `edge/sync/src/config.rs` had no procurement families at
// all, so the cloud serialised four arrays that serde discarded in silence —
// and silence is what serde does with an unknown field by default.
//
// SO THE GUARD WAS NOT MISSING AND NOT BLIND TO NEW TABLES. It was built, it
// saw the new tables, it fired on exactly what it covered. Its scope defined
// "delivered" as "the cloud emits it" rather than "the outlet stores it", and
// the gap lived one hop past the edge of that definition.
//
// This is contracts 0.5.9 inverted. Then, the EDGE wrote source_stock_count_id
// and the CLOUD had never heard of it, so json.Unmarshal dropped it and the
// column was NULL for every row. Now the CLOUD writes four families and the
// EDGE has never heard of them, so serde drops them and the tables are empty at
// every outlet. Same silence, same lenient decoder, opposite direction.
//
// A column nothing reads is a column that does not exist — and "nothing reads
// it" has to be measured at the END of the wire, not at the start.
//
// SCOPE, stated so the guarantee is not overread: this compares the top-level
// FAMILY NAMES of the config bundle — cloud struct json tags against the edge's
// serde field names. It does not compare the columns inside a family; the Go
// guard does that for the cloud half. A family present on both sides but
// missing a field is not caught here.

import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const CLOUD = join(repoRoot, "backend/cmd/api/syncconfig.go");
const EDGE = join(repoRoot, "edge/sync/src/config.rs");

// Families the cloud sends that the edge deliberately does not apply. Declared
// with a reason, never a placeholder — the SINGLE_STORE_MIGRATIONS discipline.
// A stale entry fails too, so this list cannot quietly accumulate dead rows.
const NOT_APPLIED_AT_EDGE = {
  // EMPTY, AND THAT IS THE POINT. Every family the cloud sends has a field on
  // the edge's ConfigBundle -- including `roles`, `day_start_time` and
  // `fiscal_profile`, which the edge accepts as fields even where it applies
  // them as something other than a collection.
  //
  // The first draft of this file declared those three as "not applied at the
  // edge" from reading the cloud side alone. The stale-exemption check below
  // caught all three immediately: they ARE on the edge struct, so exempting
  // them would have been a lie about coverage written into the guard on the
  // day it was born. An exemption list that starts empty is one nobody has to
  // audit later.
};

const fail = (message) => {
  console.error(`check-config-apply-drift: ${message}`);
  process.exit(1);
};

const read = (path) => {
  if (!existsSync(path)) {
    // Loudly, never skipping: a check that passes when it cannot find its
    // inputs is the failure it exists to prevent.
    fail(`cannot read ${path} — the path is wrong, not the code`);
  }
  return readFileSync(path, "utf8");
};

// --- cloud: the top-level families of syncConfigResponse --------------------
function cloudFamilies(source) {
  const start = source.indexOf("type syncConfigResponse struct {");
  if (start === -1) fail("syncConfigResponse not found in the cloud handler");
  const end = source.indexOf("\n}", start);
  const body = source.slice(start, end);

  const families = new Set();
  for (const line of body.split("\n")) {
    if (line.trimStart().startsWith("//")) continue;
    const tag = line.match(/json:"([^",]+)/);
    if (tag) families.add(tag[1]);
  }
  return families;
}

// --- edge: the fields of ConfigBundle ---------------------------------------
function edgeFamilies(source) {
  const start = source.indexOf("pub struct ConfigBundle {");
  if (start === -1) fail("ConfigBundle not found in the edge config module");
  const end = source.indexOf("\n}", start);
  const body = source.slice(start, end);

  const families = new Set();
  for (const line of body.split("\n")) {
    const trimmed = line.trimStart();
    if (trimmed.startsWith("//")) continue;
    // An explicit serde rename wins over the field name.
    const renamed = trimmed.match(/#\[serde\(rename\s*=\s*"([^"]+)"/);
    if (renamed) {
      families.add(renamed[1]);
      continue;
    }
    const field = trimmed.match(/^pub\s+([a-z0-9_]+)\s*:/);
    if (field) families.add(field[1]);
  }
  return families;
}

const cloud = cloudFamilies(read(CLOUD));
const edge = edgeFamilies(read(EDGE));

if (cloud.size < 5) fail(`parsed only ${cloud.size} cloud families — the parser is wrong, not the schema`);
if (edge.size < 5) fail(`parsed only ${edge.size} edge families — the parser is wrong, not the schema`);

const problems = [];

for (const family of cloud) {
  if (edge.has(family)) continue;
  if (family in NOT_APPLIED_AT_EDGE) continue;
  problems.push(
    `SENT BUT NEVER APPLIED: the cloud puts "${family}" in GET /sync/config, and\n` +
      `        edge/sync/src/config.rs has no such field. serde discards an unknown\n` +
      `        field in silence, so this family is empty at every outlet — the\n` +
      `        screens that read it look built and are unusable.`,
  );
}

// A stale exemption is a lie about coverage: if the edge now applies a family
// declared here, the declaration has to go.
for (const [family, reason] of Object.entries(NOT_APPLIED_AT_EDGE)) {
  if (edge.has(family)) {
    problems.push(
      `STALE EXEMPTION: "${family}" is declared as not-applied (${reason})\n` +
        `        but edge/sync/src/config.rs now has it. Remove the entry.`,
    );
  }
}

if (problems.length > 0) {
  console.error("check-config-apply-drift: THE CLOUD IS SENDING CONFIG THE EDGE THROWS AWAY\n");
  for (const problem of problems) console.error(`  • ${problem}\n`);
  console.error(
    "The Go guard in backend/cmd/api/syncconfig_guard_test.go proves the cloud SENDS a\n" +
      "column. It cannot prove an outlet STORES it. This check is the other half, and it\n" +
      "exists because four procurement families passed that guard at 0.6.0 and reached no\n" +
      "outlet at all.",
  );
  process.exit(1);
}

console.log(
  `check-config-apply-drift: ok — ${cloud.size} cloud families, ` +
    `${Object.keys(NOT_APPLIED_AT_EDGE).length} declared as not-applied, 0 silently discarded`,
);
