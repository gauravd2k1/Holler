#!/usr/bin/env node
// order.source has FIVE declarations of one closed set: a CHECK in each store,
// a Zod enum, a Go const block and an OpenAPI enum. Nothing made them agree.
//
// The failure this prevents is not hypothetical — it is the shape of contracts
// 0.5.2 and 0.5.9 both. A member added to one store and not the other is a row
// the edge accepts and the cloud refuses (or the reverse), and the refusal
// arrives as a replay failure hours later on a till, not as a red build. A
// member added to the schemas and not to the wire types is a value nothing can
// construct, which is the "a column nothing reads is a column that does not
// exist" rule pointed at an enum.
//
// It also pins the two members that are DELIBERATELY UNWRITTEN. AGGREGATOR and
// TABLE_TAB (contracts 0.8.1, ADR-026) exist in the schema ahead of their
// writers, so "declared but never emitted" is the correct state and must be
// asserted rather than assumed: without the assertion, the day something starts
// writing TABLE_TAB is a day nobody notices.
//
// Run: node scripts/check-order-source-drift.mjs

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

// The set every surface must declare, in the order the schema declares it.
const EXPECTED = [
  "POS",
  "QR",
  "AGGREGATOR_ZOMATO",
  "AGGREGATOR_SWIGGY",
  "DIRECT",
  "AGGREGATOR",
  "TABLE_TAB",
];

// Members that exist in the schema but must have no writer yet (ADR-026). A
// writer for either lands in its own change, and that change removes the member
// from this list in the same commit — forced removal, not remembered removal.
const MUST_HAVE_NO_WRITER = ["AGGREGATOR", "TABLE_TAB"];

// Where a writer would live. Deliberately the SINKS, not the screens: the edge
// is the only thing that writes `order.source`, and the Tauri command layer is
// the only caller that chooses a value.
const WRITER_ROOTS = ["edge", join("apps", "pos", "src-tauri", "src")];

const failures = [];

function read(rel) {
  return readFileSync(join(ROOT, rel), "utf8");
}

// ---- 1. The two stores -------------------------------------------------

// The SQLite declaration lives in the 0035 rebuild; the postgres one in the
// 0035 ADD CONSTRAINT. Both are matched by the same shape so a reformatting of
// either cannot quietly stop this check from finding anything — an empty match
// is a failure below, never a pass.
function checkMembers(sql, label, after) {
  // `after` anchors the match past any earlier `source IN (...)` in the file --
  // the postgres guard block contains one, and matching it would compare the
  // deprecated-member list against the full set and report nonsense.
  const body = after ? sql.slice(sql.indexOf(after)) : sql;
  if (after && !sql.includes(after)) {
    failures.push(`${label}: anchor ${JSON.stringify(after)} not found -- this check cannot verify what it cannot locate`);
    return null;
  }
  const m = body.match(/source\s+IN\s*\(([^)]*)\)/i);
  if (!m) {
    failures.push(`${label}: no \`source IN (...)\` CHECK found — this check cannot verify what it cannot parse`);
    return null;
  }
  return m[1]
    .split(",")
    .map((v) => v.trim().replace(/^'/, "").replace(/'$/, ""))
    .filter((v) => v.length > 0);
}

const sqliteMembers = checkMembers(read("packages/contracts/sqlite/0035_order_source_widened.sql"), "sqlite/0035");
const postgresMembers = checkMembers(
  read("packages/contracts/postgres/0035_order_source_widened.sql"),
  "postgres/0035",
  "ADD CONSTRAINT order_source_check",
);

// ---- 2. Zod ------------------------------------------------------------

const tsSource = read("packages/contracts/src/types/order.ts");
const zodBlock = tsSource.match(/OrderSourceSchema\s*=\s*z\.enum\(\[([\s\S]*?)\]\)/);
const zodMembers = zodBlock
  ? [...zodBlock[1].matchAll(/"([A-Z_]+)"/g)].map((m) => m[1])
  : (failures.push("order.ts: OrderSourceSchema z.enum not found"), null);

// ---- 3. Go -------------------------------------------------------------

const goSource = read("packages/contracts/go/order.go");
const goBlock = goSource.match(/type OrderSource string[\s\S]*?const \(([\s\S]*?)\n\)/);
const goMembers = goBlock
  ? [...goBlock[1].matchAll(/OrderSource\s*=\s*"([A-Z_]+)"/g)].map((m) => m[1])
  : (failures.push("order.go: OrderSource const block not found"), null);

// ---- 4. OpenAPI --------------------------------------------------------

const openapi = read("packages/contracts/openapi/openapi.yaml");
const openapiLine = openapi.match(/enum:\s*\[POS,[^\]]*AGGREGATOR_ZOMATO[^\]]*\]/);
const openapiMembers = openapiLine
  ? openapiLine[0]
      .replace(/^enum:\s*\[/, "")
      .replace(/\]$/, "")
      .split(",")
      .map((v) => v.trim())
  : (failures.push("openapi.yaml: the order source enum was not found"), null);

// ---- Compare -----------------------------------------------------------

const surfaces = {
  "sqlite CHECK": sqliteMembers,
  "postgres CHECK": postgresMembers,
  "Zod OrderSourceSchema": zodMembers,
  "Go OrderSource": goMembers,
  "OpenAPI enum": openapiMembers,
};

for (const [label, members] of Object.entries(surfaces)) {
  if (!members) continue;
  const missing = EXPECTED.filter((m) => !members.includes(m));
  const extra = members.filter((m) => !EXPECTED.includes(m));
  if (missing.length > 0) {
    failures.push(`${label} is MISSING ${missing.join(", ")} — one surface short of the set is a value that writes on one side and is refused on the other`);
  }
  if (extra.length > 0) {
    failures.push(`${label} declares ${extra.join(", ")}, which no other surface has`);
  }
}

// ---- The unwritten members --------------------------------------------

import { readdirSync, statSync } from "node:fs";

function walk(dir, out) {
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    return out;
  }
  for (const entry of entries) {
    if (entry === "target" || entry === "node_modules" || entry === "dist") continue;
    const full = join(dir, entry);
    let st;
    try {
      st = statSync(full);
    } catch {
      continue;
    }
    if (st.isDirectory()) walk(full, out);
    else if (entry.endsWith(".rs")) out.push(full);
  }
  return out;
}

const writerFiles = [];
for (const root of WRITER_ROOTS) walk(join(ROOT, root), writerFiles);

for (const member of MUST_HAVE_NO_WRITER) {
  const literal = `"${member}"`;
  for (const file of writerFiles) {
    const whole = readFileSync(file, "utf8");
    // Everything from `#[cfg(test)]` onward is a test module, and a test that
    // INSERTS a member to prove the widened CHECK accepts it is not a writer --
    // it is the assertion that the member exists. Scanning it would make the
    // only honest way to test the widening indistinguishable from shipping a
    // writer, which is how a check teaches people to work around it.
    const testModule = whole.search(/^\s*#\[cfg\(test\)\]/m);
    const text = testModule === -1 ? whole : whole.slice(0, testModule);
    text.split(/\r?\n/).forEach((line, i) => {
      if (!line.includes(literal)) return;
      // A test asserting the absence is not a writer.
      if (/assert|expect|must not|never/i.test(line)) return;
      // AGGREGATOR is ALSO a legitimate `order_type` member (contracts 0001),
      // and ACCEPTED_ORDER_TYPE writes it. This check is about `source` only;
      // without this the two collide and the check reports a writer that is
      // writing a different column.
      if (/order_type|ORDER_TYPE/.test(line)) return;
      failures.push(
        `${file.slice(ROOT.length + 1)}:${i + 1} emits ${literal}, which ADR-026 records as having no writer. If that is now intended, remove ${member} from MUST_HAVE_NO_WRITER in this script IN THE SAME COMMIT.`,
      );
    });
  }
}

// ---- Report ------------------------------------------------------------

if (failures.length > 0) {
  console.error("order.source drift check FAILED:\n");
  for (const f of failures) console.error(`  - ${f}`);
  console.error(`\n${failures.length} problem(s). The set is declared in five places and they must agree (ADR-026).`);
  process.exit(1);
}

console.log(
  `order.source drift check passed: ${EXPECTED.length} members agree across sqlite, postgres, Zod, Go and OpenAPI; ${MUST_HAVE_NO_WRITER.join(" and ")} have no writer.`,
);
