#!/usr/bin/env node
// ONE RULE, TWO RUNTIMES: a run that drops, migrates or seeds a schema may only
// name a database whose name starts with the scratch prefix.
//
// It is spelled twice because the two callers share no language —
// scripts/agent-guard.ps1 guards demo-reset.ps1, and
// backend/internal/platform/testdb guards every Postgres-backed Go test — and
// neither can import the other. This check is the joint, exactly as
// check-order-amendable-drift.mjs is for the line-amendment set (ADR-028).
//
// The two incidents behind the rule:
//   - demo-reset.ps1's destructive DROP SCHEMA read -PostgresDb while every
//     seeder read -DatabaseUrl, so a scratch -DatabaseUrl on its own left the
//     drop aimed at 'holler'.
//   - The Go suite pointed at the shared dev database overwrote
//     owner@holler.test and cashier@holler.test with fixture hashes, and the
//     till then refused a correct password with a 401 indistinguishable from a
//     wrong one.
//
// It also checks the two things that would make the rule decorative: that
// demo-reset derives its drop target from the URL the seeders use, and that
// CI does not point HOLLER_TEST_DATABASE_URL at a non-scratch database — a
// green CI run against 'holler' is how a rule like this gets quietly reverted.
//
// Run: node scripts/check-scratch-db-guard.mjs

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const read = (rel) => readFileSync(join(ROOT, rel), "utf8");
const failures = [];

function prefix(source, pattern, label) {
  const m = source.match(pattern);
  if (!m) {
    failures.push(
      `${label}: the scratch-prefix declaration was not found — this check cannot verify what it cannot locate. If it moved, point this script at it IN THE SAME COMMIT.`,
    );
    return null;
  }
  return m[1];
}

const powershell = prefix(
  read("scripts/agent-guard.ps1"),
  /ScratchDatabasePrefix\s*=\s*"([a-z0-9_]+)"/,
  "scripts/agent-guard.ps1",
);
const go = prefix(
  read("backend/internal/platform/testdb/testdb.go"),
  /ScratchDatabasePrefix\s*=\s*"([a-z0-9_]+)"/,
  "backend/internal/platform/testdb/testdb.go",
);

if (powershell && go && powershell !== go) {
  failures.push(
    `the two spellings disagree: PowerShell says "${powershell}", Go says "${go}". A database name one guard clears and the other refuses is a guard that only holds on whichever side runs first.`,
  );
}

// The drop target must be DERIVED, never an independent parameter again.
const reset = read("scripts/demo-reset.ps1");
if (!/\$PostgresDb\s*=\s*\$urlDatabase/.test(reset)) {
  failures.push(
    "scripts/demo-reset.ps1 no longer derives -PostgresDb from -DatabaseUrl. Two independent database parameters is the defect this rule was written for: the drop hits one and the seeders the other.",
  );
}
if (!/Assert-ScratchDatabaseTarget/.test(reset)) {
  failures.push("scripts/demo-reset.ps1 does not call Assert-ScratchDatabaseTarget");
}

// CI must not point the suite at a non-scratch database.
const ci = read(".github/workflows/ci.yml");
for (const m of ci.matchAll(/HOLLER_TEST_DATABASE_URL:\s*(\S+)/g)) {
  const name = m[1].split("?")[0].split("/").pop();
  if (powershell && !name.toLowerCase().startsWith(powershell)) {
    failures.push(
      `.github/workflows/ci.yml points HOLLER_TEST_DATABASE_URL at database "${name}", which is not a scratch database. CI green against a working database name is how this rule gets reverted without anyone deciding to.`,
    );
  }
}

if (failures.length > 0) {
  console.error("scratch-database guard check FAILED:\n");
  for (const f of failures) console.error(`  - ${f}`);
  console.error(`\n${failures.length} problem(s).`);
  process.exit(1);
}

console.log(
  `scratch-database guard OK — PowerShell and Go both require "${powershell}", demo-reset derives its drop target from -DatabaseUrl, and CI names a scratch database.`,
);
