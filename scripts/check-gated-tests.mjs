#!/usr/bin/env node
// A GATED TARGET NOTHING INVOKES IS A TARGET THAT DOES NOT EXIST.
//
// Cargo targets carrying `required-features` are invisible to a plain
// `cargo test`: they are not built, not run, and — this is the part that
// costs — they are not REPORTED as skipped either. The suite goes green
// having never compiled them.
//
// That is the exact state M4 acceptance criterion 1 spent four pushes in: a
// test existed, CI never ran it (there, because `cargo fmt --check` failed in
// front of it), and the criterion was recorded UNPROVEN because nothing had
// run to prove it. Gating is the right call when a target needs PostgreSQL or
// the Go toolchain — but a gate and a pipeline that opens it are one decision,
// not two, and only one of them is enforced by the compiler.
//
// So this asserts the other half:
//
//   1. every feature that gates ANY target is enabled by some `--features`
//      flag on a ci.yml run line, and
//   2. every gated `[[test]]` target is named by a `--test` flag there.
//
// Rule 1 covers gated `[[bin]]` targets too — `crashpoint` is reachable only
// through a test that enables `crash-points`, so what CI must name is the
// feature. Rule 2 is the stricter check available for test targets, whose
// names CI can address directly.
//
// Run from the repo root: node scripts/check-gated-tests.mjs

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = process.cwd();
const CI_PATH = join(ROOT, ".github", "workflows", "ci.yml");
const SKIP_DIRS = new Set(["target", "node_modules", ".git", "dist", "build", ".vscode"]);

function findCargoTomls(dir, found = []) {
  for (const entry of readdirSync(dir)) {
    if (SKIP_DIRS.has(entry)) continue;
    const full = join(dir, entry);
    let st;
    try {
      st = statSync(full);
    } catch {
      continue; // a symlink or a file that vanished mid-walk is not our problem
    }
    if (st.isDirectory()) findCargoTomls(full, found);
    else if (entry === "Cargo.toml") found.push(full);
  }
  return found;
}

// Line-based rather than a TOML parser, deliberately: this repo has no TOML
// dependency in scripts/, and the shapes it must read — `[[test]]`/`[[bin]]`
// blocks with `name` and `required-features` — are unambiguous line by line.
// A shape it cannot read is a shape it must not silently pass, so anything
// with `required-features` and no name is a hard error below.
function gatedTargets(tomlPath) {
  const text = readFileSync(tomlPath, "utf8");
  const targets = [];
  let kind = null;
  let name = null;
  let features = null;

  const flush = () => {
    if (kind && features) {
      if (!name) {
        throw new Error(
          `${relative(ROOT, tomlPath)}: a [[${kind}]] has required-features but no name`,
        );
      }
      targets.push({ kind, name, features, tomlPath });
    }
    kind = null;
    name = null;
    features = null;
  };

  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (line.startsWith("[")) {
      flush();
      if (line === "[[test]]") kind = "test";
      else if (line === "[[bin]]") kind = "bin";
      continue;
    }
    if (!kind) continue;
    const nameMatch = line.match(/^name\s*=\s*"([^"]+)"/);
    if (nameMatch) name = nameMatch[1];
    const featMatch = line.match(/^required-features\s*=\s*\[([^\]]*)\]/);
    if (featMatch) {
      features = [...featMatch[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
    }
  }
  flush();
  return targets;
}

const ci = readFileSync(CI_PATH, "utf8");
const enabledFeatures = new Set(
  [...ci.matchAll(/--features[= ]+([A-Za-z0-9_,-]+)/g)].flatMap((m) => m[1].split(",")),
);
const namedTests = new Set([...ci.matchAll(/--test[= ]+([A-Za-z0-9_-]+)/g)].map((m) => m[1]));

const problems = [];
let checked = 0;

for (const toml of findCargoTomls(ROOT)) {
  for (const target of gatedTargets(toml)) {
    checked += 1;
    const where = relative(ROOT, target.tomlPath).replace(/\\/g, "/");
    for (const feature of target.features) {
      if (!enabledFeatures.has(feature)) {
        problems.push(
          `${where}: [[${target.kind}]] "${target.name}" is gated behind feature "${feature}", ` +
            `which no ci.yml run line enables.\n` +
            `    Nothing builds this target. Add a job that runs it with --features ${feature}, ` +
            `or delete the target — a gate nobody opens is not coverage.`,
        );
      }
    }
    if (target.kind === "test" && !namedTests.has(target.name)) {
      problems.push(
        `${where}: gated test target "${target.name}" is not named by any --test flag in ci.yml.\n` +
          `    Enabling its feature is not enough: a gated test is only run when it is addressed.\n` +
          `    Add: cargo test --features ${target.features.join(",")} --test ${target.name}`,
      );
    }
  }
}

if (problems.length > 0) {
  console.error("check-gated-tests: gated targets CI does not invoke\n");
  for (const p of problems) console.error(`  - ${p}\n`);
  process.exit(1);
}

console.log(`check-gated-tests: ok — ${checked} gated target(s), every one invoked by ci.yml`);
