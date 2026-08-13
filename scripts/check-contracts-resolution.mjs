// Fails if any workspace package resolves a @holler/contracts different from
// the version in packages/contracts/package.json.
//
// Why this exists: on 2026-08-13, merging the POS cart track into main left
// apps/pos resolving @holler/contracts 0.3.1 while the workspace was at 0.4.1.
// `pnpm test` passed all 114 tests; only `tsc --noEmit` caught it, because the
// stale package still satisfied every runtime shape the tests exercised. A
// suite that passes against a FROZEN CONTRACT FROM TWO VERSIONS AGO defeats the
// entire point of freezing it.
//
// docs/backlog-m2.md flagged this as "worth confirming CI cannot hit the same
// state" after it appeared during Milestone 2. It then happened again. This
// check is that confirmation, made mechanical.
//
// Run: node scripts/check-contracts-resolution.mjs

import { readFileSync, existsSync, readdirSync } from "node:fs";
import { join } from "node:path";

const REPO_ROOT = new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const CONTRACTS_PKG = join(REPO_ROOT, "packages/contracts/package.json");

// Every workspace root that may depend on contracts. A directory that does not
// exist is skipped rather than fatal, so this list can name an app before its
// track lands.
const CONSUMER_ROOTS = ["apps", "packages"];

function readJSON(path) {
  return JSON.parse(readFileSync(path, "utf-8"));
}

const expected = readJSON(CONTRACTS_PKG).version;
const problems = [];
let checked = 0;

for (const root of CONSUMER_ROOTS) {
  const dir = join(REPO_ROOT, root);
  if (!existsSync(dir)) continue;

  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const pkgDir = join(dir, entry.name);
    if (!existsSync(join(pkgDir, "package.json"))) continue;

    const pkg = readJSON(join(pkgDir, "package.json"));
    const deps = { ...(pkg.dependencies ?? {}), ...(pkg.devDependencies ?? {}) };
    if (!("@holler/contracts" in deps)) continue;
    if (pkg.name === "@holler/contracts") continue;

    const resolved = join(pkgDir, "node_modules/@holler/contracts/package.json");
    if (!existsSync(resolved)) {
      problems.push(`${pkg.name}: declares @holler/contracts but nothing is resolved — run pnpm install`);
      continue;
    }

    checked += 1;
    const actual = readJSON(resolved).version;
    if (actual !== expected) {
      problems.push(
        `${pkg.name}: resolves @holler/contracts ${actual}, workspace is ${expected}. ` +
          `Fix with: pnpm install --filter ${pkg.name}`,
      );
    }
  }
}

if (problems.length > 0) {
  console.error("Contracts resolution drift:\n");
  for (const p of problems) console.error(`  - ${p}`);
  console.error(
    "\nA stale resolution passes tests while breaking typecheck, because the old\n" +
      "package still satisfies the shapes the tests exercise. That is the exact\n" +
      "failure mode freezing contracts is meant to prevent.",
  );
  process.exit(1);
}

console.log(
  `Contracts resolution check passed: ${checked} consumer(s) all on @holler/contracts ${expected}.`,
);
