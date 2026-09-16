#!/usr/bin/env node
// A TEST MAY ONLY DEPEND ON FILES THE REPOSITORY CONTAINS.
//
// Third occurrence of one class, which is why it is a check rather than a
// third fix:
//
//   1. A "scratch" test run overwrote the operator's real apps\pos\.env.dev
//      and zeroed the edge database key, because the code it invoked
//      recomputed its own paths (CLAUDE.md, scripts/agent-guard.ps1).
//   2. edge/database's seed_offline_sale defaulted to seed/outlet.toml, so it
//      failed on every machine whose outlet file differs from the committed
//      example — read as a seed defect for an afternoon (2026-09-16).
//   3. edge/database's crash_durability did the same, and had been failing in
//      CI on a clean checkout the whole time with "outlet identity file ...
//      could not be read (os error 2)". Nobody looked at CI.
//
// The shape: a file that exists on the author's machine and nowhere else. The
// test passes locally for whoever wrote it and fails for everyone else, and on
// a red CI nobody reads it fails invisibly.
//
// WHAT THIS CHECKS. A test that runs `devseed` must name the COMMITTED outlet
// identity (seed/outlet.example.toml, via HOLLER_OUTLET_FILE or --outlet-file).
// Scripts are deliberately NOT checked: scripts/demo-up.ps1 and friends are
// operator tools for a real installation, and their refusal to fall back to
// the example is the rule seed/README.md sets, not a defect.
//
// Run: node scripts/check-tests-need-no-gitignored-files.mjs

import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const failures = [];

// Paths git deliberately does not carry, that a test could reach for.
const GITIGNORED = [
  { needle: "seed/outlet.toml", why: "gitignored and per-installation (seed/README.md)" },
  { needle: "seed\outlet.toml", why: "gitignored and per-installation (seed/README.md)" },
  { needle: ".env.dev", why: "gitignored, and carries the edge database key" },
];

const SEARCH_DIRS = ["edge", "backend", "apps", "tests"];
const TEST_FILE = /(^|\/)(tests?)\/|_test\.(go|rs|ts|mjs)$|\.test\.(ts|tsx|mjs)$|(^|\/)__tests__\//;

function walk(dir, out) {
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    return out;
  }
  for (const entry of entries) {
    if (["target", "node_modules", "dist", ".vite"].includes(entry)) continue;
    const full = join(dir, entry);
    let st;
    try {
      st = statSync(full);
    } catch {
      continue;
    }
    if (st.isDirectory()) walk(full, out);
    else if (/\.(rs|go|ts|tsx|mjs)$/.test(entry)) out.push(full);
  }
  return out;
}

const files = [];
for (const dir of SEARCH_DIRS) walk(join(ROOT, dir), files);

for (const file of files) {
  const rel = relative(ROOT, file).split("\\").join("/");
  if (!TEST_FILE.test(rel)) continue;
  const source = readFileSync(file, "utf8");

  // A test that runs devseed must name the committed identity.
  // ONLY THE RUST devseed. `backend/cmd/devseed` reads seed/demo-outlet.json
  // and never opens an identity file, so a test that runs the Go one needs no
  // HOLLER_OUTLET_FILE and must not be nagged into setting a variable it does
  // not read — a check that demands a meaningless line teaches people to add
  // meaningless lines.
  //
  // TWO WAYS TO RUN IT, and the first version knew only one. The e2e harness
  // spawns `cargo run --bin devseed` rather than using CARGO_BIN_EXE, so it
  // sailed through this check while failing in CI for exactly the reason the
  // check exists. Proved by removing its identity line and watching this pass.
  // A check that covers one of two call shapes is a check that reports on the
  // shape its author happened to have in mind.
  const runsRustDevseed =
    /CARGO_BIN_EXE_devseed/.test(source) ||
    /--bin[\s\S]{0,40}devseed/.test(source);
  if (runsRustDevseed) {
    // ONLY THE MECHANISMS DEVSEED ACTUALLY READS (outlet_identity::resolve_path):
    // the env var or the flag. Merely MENTIONING outlet.example.toml is not
    // enough and used to be accepted — deleting the `.env(HOLLER_OUTLET_FILE)`
    // line while leaving a helper that names the file passed this check, which
    // is a check satisfied by a comment.
    const namesIt = /HOLLER_OUTLET_FILE/.test(source) || /--outlet-file/.test(source);
    if (!namesIt) {
      failures.push(
        `${rel} runs devseed without naming an outlet identity, so it falls back to seed/outlet.toml — gitignored, per-installation, and absent on a clean checkout. It will pass for whoever wrote it and fail for everyone else. Pass HOLLER_OUTLET_FILE=seed/outlet.example.toml.`,
      );
    }
  }

  for (const { needle, why } of GITIGNORED) {
    // Comments explaining the rule are fine; a string literal reaching for the
    // file is not.
    const lines = source.split(/\r?\n/);
    lines.forEach((line, i) => {
      if (!line.includes(needle)) return;
      const trimmed = line.trim();
      if (trimmed.startsWith("//") || trimmed.startsWith("*") || trimmed.startsWith("#")) return;
      if (!/["'`]/.test(line)) return;
      // A MESSAGE that names the file is not a dependency on it. The rule is
      // about opening the path, so the line must look like path handling:
      // backend/cmd/devseed/seedfile_test.go names seed/outlet.toml inside a
      // failure string and depends on nothing.
      if (!/Path|path|join|open|read|env|PathBuf|filepath|Join-Path/.test(line)) return;
      failures.push(`${rel}:${i + 1} reaches for ${needle}, which is ${why}.`);
    });
  }
}

if (failures.length > 0) {
  console.error("tests-depend-on-committed-files check FAILED:\n");
  for (const f of failures) console.error(`  - ${f}`);
  console.error(`\n${failures.length} problem(s). A test may only depend on files the repository contains.`);
  process.exit(1);
}

console.log(
  `tests-depend-on-committed-files check OK — ${files.length} source file(s) scanned; no test reaches for a gitignored path.`,
);
