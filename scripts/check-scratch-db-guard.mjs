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

// THE PRODUCTION MIGRATION PATH MUST STAY EXEMPT BY CONSTRUCTION, NEVER BY A
// FLAG. The only thing that applies the contract migrations to a real database
// is the API starting up (backend/cmd/api/main.go -> postgres.Migrate), and it
// is clear of this rule because it cannot reach the guard at all: testdb is
// imported only from _test.go files. If a non-test file in cmd/api ever
// imports it, the production path is one boolean away from being able to turn
// the guard off everywhere, which is the failure this whole rule exists to
// prevent. Filed as pilot-readiness B7.
for (const file of ["backend/cmd/api/main.go", "backend/cmd/api/router.go"]) {
  let source;
  try {
    source = read(file);
  } catch {
    continue; // a file that does not exist cannot import anything
  }
  if (/platform\/testdb/.test(source)) {
    failures.push(
      `${file} imports platform/testdb. The production migration path must be exempt from the scratch rule BY CONSTRUCTION — because it cannot reach the guard — never by a bypass flag.`,
    );
  }
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

// EVERY CI STEP THAT SEEDS OR MIGRATES NAMES ITS DATABASE.
//
// The rule above stops a run naming the WRONG database. This stops one naming
// NONE, which is the same hazard by omission: `cmd/devseed` used to fall back
// to ...localhost:5432/holler, so CI's migrate step went on connecting to a
// database that had just been renamed out from under it, and the backend job
// failed for eleven commits while that failure sat in a wall of red. D13's
// shape again -- a target named in one place that the step doing the work
// never reads.
//
// devseed now refuses without DATABASE_URL, so a workflow step that runs it
// without setting one fails at the step. This check moves that discovery from
// a CI run to a push.
const ciSteps = read(".github/workflows/ci.yml").split(/\n      - name: /).slice(1);
for (const step of ciSteps) {
  // COMMENTS DO NOT RUN. The first version of this matched
  // `cargo run --bin devseed` inside a comment and reported two innocent
  // `cargo check` / `cargo build` steps. A check that reads prose as behaviour
  // produces exactly the noise that teaches people to ignore it.
  const executable = step
    .split(/\r?\n/)
    .filter((line) => !/^\s*#/.test(line))
    .join("\n");
  const runsSeeder = /go run \.\/cmd\/devseed|cargo run [^\n]*--bin devseed/.test(executable);
  if (!runsSeeder) continue;
  // --emit-json writes a file and opens no database.
  if (/--emit-json/.test(executable)) continue;
  const stepName = step.split("\n")[0].trim();
  if (!/DATABASE_URL:/.test(step)) {
    failures.push(
      `.github/workflows/ci.yml step "${stepName}" runs the seeder without setting DATABASE_URL. devseed has no default any more -- the one it had named the live \`holler\` database -- so this step names no database and refuses.`,
    );
  }
}

// EVERY POWERSHELL CALLER OF THE SEEDER NAMES ITS DATABASE TOO.
//
// The CI loop above covered workflow steps and nothing else, so when 29e3ead
// removed devseed's default the PowerShell callers were missed entirely:
// `scripts\dev-bootstrap.ps1` ran `go run ./cmd/devseed` with no database at
// all and died at its step [2/4] with "devseed: no database named" -- on a
// demo morning, in the middle of `demo-up.ps1 -Fresh`. The guard existed, it
// was green, and it could not see the caller that broke.
//
// A caller satisfies this by either passing --database-url or assigning
// $env:DATABASE_URL, and in both cases the value must come from a
// $DatabaseUrl parameter rather than a literal: a second literal is a second
// source, and demo-reset.ps1's DROP SCHEMA target is derived from that same
// parameter. That is the whole point of D13's fix.
const seederCallers = [
  "scripts/dev-bootstrap.ps1",
  "scripts/demo-reset.ps1",
];
for (const file of seederCallers) {
  const text = read(file);
  // Comments do not run -- same rule the CI loop above learned.
  const executable = text
    .split(/\r?\n/)
    .filter((line) => !/^\s*#/.test(line))
    .join("\n");

  for (const raw of executable.split(/\r?\n/)) {
    // QUOTED TEXT DOES NOT RUN, the same way comments do not. demo-reset.ps1's
    // -WhatIf branch prints the words "would run 'go run ./cmd/devseed'", and
    // the first version of this loop read that message as a call and demanded
    // a flag on a Write-Note. A check that reads prose as behaviour produces
    // the noise that teaches people to ignore it -- this file already learned
    // that once, one loop up.
    const line = raw.replace(/"[^"]*"/g, '""').replace(/'[^']*'/g, "''");
    if (!/go run \.\/cmd\/devseed/.test(line)) continue;
    if (!/--database-url/.test(line)) {
      failures.push(
        `${file} runs the Go seeder without --database-url. devseed has no default any more, so this call names no database and refuses at run time -- which is exactly how dev-bootstrap.ps1 broke a demo morning while this guard was green.`,
      );
      continue;
    }
    if (!/--database-url\s+\$DatabaseUrl\b/.test(line)) {
      failures.push(
        `${file} passes --database-url from something other than $DatabaseUrl. It must come from the same parameter demo-reset.ps1 derives its DROP SCHEMA target from -- a second source can drift from the one the destructive step reads (D13).`,
      );
    }
  }

  // The parameter itself must exist, or the line above is passing an empty
  // string and the refusal moves from the caller to the seeder.
  if (/go run \.\/cmd\/devseed/.test(executable) && !/\[string\]\$DatabaseUrl\s*=/.test(text)) {
    failures.push(
      `${file} calls the Go seeder but declares no [string]$DatabaseUrl parameter, so there is nothing for a caller to override and no single source for the database name.`,
    );
  }
}

// demo-up.ps1 must hand its own $DatabaseUrl down to the bootstrap. It already
// passes it to demo-reset.ps1 and to the backend; the bootstrap seeds the same
// cloud, and a run whose steps name two databases is the defect this whole
// file exists for.
const demoUp = read("scripts/demo-up.ps1");
if (!/\$bootstrapArgs\["DatabaseUrl"\]\s*=\s*\$DatabaseUrl/.test(demoUp)) {
  failures.push(
    `scripts/demo-up.ps1 does not pass DatabaseUrl to dev-bootstrap.ps1. It passes one to demo-reset.ps1 and to the backend, so without this the bootstrap falls back to its own default and one run can seed two different databases.`,
  );
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
