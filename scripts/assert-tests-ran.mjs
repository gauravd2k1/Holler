#!/usr/bin/env node
// A SUITE THAT RUNS NOTHING MUST BE AS LOUD AS A SUITE THAT FAILS.
//
// A test command that executes ZERO tests and exits 0 is indistinguishable,
// from the outside, from a passing suite. Every runner this project uses has
// at least one way to do it, and all three were confirmed by experiment on
// 2026-09-02 rather than assumed:
//
//   cargo test <filter>          -> "0 passed; 2 filtered out", exit 0
//   cargo test --exact <typo>    -> same, exit 0   (this bit us: a truncated
//                                   test name in a repro loop reported 20
//                                   clean runs having run nothing)
//   go test -run <typo> ./...    -> "ok  pkg  [no tests to run]", exit 0
//   vitest run -t <typo>         -> "Tests  230 skipped", exit 0
//
// And two failures that do not even reach a runner exit the same way once a
// pipe is involved: `cargo test -p <name>` outside a workspace, and
// `make check-seams` with no make on PATH, both die — but `cmd | tail` reports
// the exit status of `tail`, which is 0. Two "green" lines were reported in
// this repository from exactly that.
//
// So: run the command, tee its output, and FAIL if the runner cannot be shown
// to have executed at least one test. The count is parsed from the runner's
// own summary; a missing summary is a failure too, because a command that
// never got as far as printing one did not run tests either.
//
//   node scripts/assert-tests-ran.mjs cargo -- cargo test --manifest-path edge/sync/Cargo.toml
//   node scripts/assert-tests-ran.mjs go -- go test -count=1 ./...
//   node scripts/assert-tests-ran.mjs vitest -- pnpm test
//
// Runner is one of: cargo | go | vitest. There is deliberately no "auto":
// guessing which summary to look for is how this guard would come to pass on
// a runner it does not understand.

import { spawn } from "node:child_process";

const RUNNERS = new Set(["cargo", "go", "vitest"]);

const argv = process.argv.slice(2);
const sep = argv.indexOf("--");
if (sep < 1) {
  console.error("usage: assert-tests-ran.mjs <cargo|go|vitest> -- <command...>");
  process.exit(2);
}
const runner = argv[0];
const command = argv.slice(sep + 1);
if (!RUNNERS.has(runner) || command.length === 0) {
  console.error(`usage: assert-tests-ran.mjs <${[...RUNNERS].join("|")}> -- <command...>`);
  process.exit(2);
}

// ---------------------------------------------------------------------------
// Counting, per runner. Each returns the number of tests OBSERVED TO EXECUTE:
// passed + failed. Skipped/ignored/filtered do NOT count — a suite that
// skipped everything ran nothing, which is the M2 defect where an unset
// HOLLER_TEST_DATABASE_URL made every Postgres-backed test t.Skip and
// `go test ./...` printed ok for all twelve packages.
// ---------------------------------------------------------------------------

/** `test result: ok. 56 passed; 0 failed; 0 ignored; ...`, one per target. */
function countCargo(output) {
  let executed = 0;
  let summaries = 0;
  for (const m of output.matchAll(/test result:.*?(\d+) passed; (\d+) failed/g)) {
    summaries += 1;
    executed += Number(m[1]) + Number(m[2]);
  }
  return { executed, summaries };
}

/**
 * `ok  <pkg>  0.3s` counts. `[no tests to run]` and `[no test files]` do not,
 * and neither does `? <pkg> [no test files]`.
 */
function countGo(output) {
  let executed = 0;
  let summaries = 0;
  for (const line of output.split(/\r?\n/)) {
    if (/^(ok|FAIL)\s/.test(line)) {
      summaries += 1;
      if (!/\[no tests? (to run|files)\]/.test(line)) executed += 1;
    }
  }
  // Go prints no per-test count, so "executed" here is PACKAGES that ran at
  // least one test. That is the granularity the runner gives us.
  return { executed, summaries };
}

/** `Tests  230 passed (230)` / `Tests  12 failed | 4 passed (16)`. */
function countVitest(output) {
  const clean = output.replace(/\[[0-9;]*m/g, "");
  if (/No test files found/i.test(clean)) return { executed: 0, summaries: 1 };
  let executed = 0;
  let summaries = 0;
  for (const m of clean.matchAll(/^\s*Tests\s+(.+)$/gm)) {
    summaries += 1;
    for (const part of m[1].matchAll(/(\d+)\s+(passed|failed)/g)) {
      executed += Number(part[1]);
    }
  }
  return { executed, summaries };
}

const COUNTERS = { cargo: countCargo, go: countGo, vitest: countVitest };

// ---------------------------------------------------------------------------

const child = spawn(command[0], command.slice(1), {
  shell: true,
  stdio: ["inherit", "pipe", "pipe"],
});

let captured = "";
child.stdout.on("data", (chunk) => {
  captured += chunk;
  process.stdout.write(chunk);
});
child.stderr.on("data", (chunk) => {
  captured += chunk;
  process.stderr.write(chunk);
});

child.on("error", (err) => {
  console.error(`\nassert-tests-ran: could not run the command at all: ${err.message}`);
  process.exit(1);
});

child.on("close", (code) => {
  const shown = command.join(" ");

  // A non-zero runner exit is already loud. Pass it straight through — this
  // guard exists for the SILENT case, and must never turn a real failure into
  // a different-looking one.
  if (code !== 0) {
    console.error(`\nassert-tests-ran: '${shown}' exited ${code}.`);
    process.exit(code ?? 1);
  }

  const { executed, summaries } = COUNTERS[runner](captured);

  if (summaries === 0) {
    console.error(
      `\nassert-tests-ran: FAIL — '${shown}' exited 0 but printed no ${runner} test summary at all.\n` +
        `A command that never reached a summary did not run tests. Check the manifest path, ` +
        `package name, working directory and that the tool is on PATH.`
    );
    process.exit(1);
  }

  if (executed === 0) {
    console.error(
      `\nassert-tests-ran: FAIL — '${shown}' exited 0 having executed ZERO tests ` +
        `(${summaries} summar${summaries === 1 ? "y" : "ies"}, all empty or skipped).\n` +
        `A suite that runs nothing is indistinguishable from a suite that passes, so this ` +
        `fails the job. Likely causes: a filter matching no test, a feature flag leaving every ` +
        `target gated out, or an unset environment variable turning every test into a skip.`
    );
    process.exit(1);
  }

  const unit = runner === "go" ? "package(s) with tests" : "test(s)";
  console.log(`assert-tests-ran: OK — ${executed} ${unit} executed by '${shown}'.`);
});
