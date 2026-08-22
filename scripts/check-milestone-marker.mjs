#!/usr/bin/env node
// Fails when CLAUDE.md's "Current milestone" block disagrees with the
// authoritative marker in .claude/current-milestone.
//
// WHY THIS EXISTS. CLAUDE.md said "## Current milestone: MILESTONE 2 — Kitchen"
// for the entire Milestone 3 build. Builder agents load CLAUDE.md as primary
// context, so every M3 builder read M2's scope and M2's EXCLUDES as the
// authoritative statement of what it was allowed to touch — a list that bars
// aggregator KOTs and the waiter app while saying nothing about billing at all.
// Nothing noticed for a whole milestone.
//
// A careful edit does not fix that: the next milestone forgets in exactly the
// same way. So the number lives in one authoritative file, /milestone updates
// the block as its FIRST act, and this check fails the build when the two
// disagree.
//
// Third check of its kind in this repo, and the shape is deliberate: a claim in
// prose is worth nothing unless something fails when it goes false
// (docs/retro.md, 2026-08-20).

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

const read = (relative) => {
  try {
    return readFileSync(join(repoRoot, relative), "utf8");
  } catch (error) {
    // Fail loudly rather than skip. A check that silently passes when it cannot
    // find its inputs is the same failure one level up, and this repo has
    // recorded three instances of it.
    console.error(`check-milestone-marker: cannot read ${relative}: ${error.message}`);
    process.exit(1);
  }
};

const markerRaw = read(".claude/current-milestone").trim();
if (!/^\d+$/.test(markerRaw)) {
  console.error(
    `check-milestone-marker: .claude/current-milestone must contain a bare milestone number, got ${JSON.stringify(markerRaw)}`,
  );
  process.exit(1);
}
const marker = Number(markerRaw);

const claudeMd = read("CLAUDE.md");

const headingMatch = claudeMd.match(/^## Current milestone:\s*MILESTONE\s+(\d+)\b/m);
if (!headingMatch) {
  console.error(
    'check-milestone-marker: CLAUDE.md has no "## Current milestone: MILESTONE <n>" heading. ' +
      "Builder agents read that block as the authoritative scope; it must exist and name a number.",
  );
  process.exit(1);
}

const commentMatch = claudeMd.match(/<!--\s*MILESTONE-MARKER:\s*(\d+)\s*-->/);
if (!commentMatch) {
  console.error(
    "check-milestone-marker: CLAUDE.md's milestone block is missing its " +
      "<!-- MILESTONE-MARKER: n --> comment. It is what makes the heading machine-checkable.",
  );
  process.exit(1);
}

const heading = Number(headingMatch[1]);
const comment = Number(commentMatch[1]);

if (heading !== marker || comment !== marker) {
  console.error(
    `check-milestone-marker: MILESTONE NUMBER DISAGREEMENT\n` +
      `  .claude/current-milestone : ${marker}\n` +
      `  CLAUDE.md heading         : ${heading}\n` +
      `  CLAUDE.md marker comment  : ${comment}\n\n` +
      `Builder agents load CLAUDE.md as primary context, so a stale block hands every\n` +
      `builder the wrong scope and the wrong EXCLUDES list. This exact drift went\n` +
      `unnoticed for the whole of Milestone 3. Update the CLAUDE.md block (scope,\n` +
      `track graph, acceptance, EXCLUDES) — not just the number — then re-run.`,
  );
  process.exit(1);
}

// A milestone's planning doc is written when the milestone is planned, so it is
// an independent signal that the marker is not merely self-consistent. Absence
// is a warning rather than an error: a milestone may legitimately start before
// its planning doc lands.
const planningDoc = `docs/m${marker}-planning.md`;
try {
  readFileSync(join(repoRoot, planningDoc), "utf8");
} catch {
  console.warn(
    `check-milestone-marker: note — ${planningDoc} does not exist. ` +
      `The marker says milestone ${marker}; nothing else corroborates that yet.`,
  );
}

// ---------------------------------------------------------------------------
// Contracts version: the same drift, fourth instance. CLAUDE.md said 0.5.3 and
// RESUME.md said 0.4.7 while package.json said 0.5.7 (2026-08-22) — the third
// time the prose copy of the version had gone stale. The version is machine-
// readable in exactly one place (packages/contracts/package.json); every prose
// copy must either match it or fail the build.

const pkg = JSON.parse(read("packages/contracts/package.json"));
const pkgVersion = pkg.version;
if (!/^\d+\.\d+\.\d+$/.test(pkgVersion ?? "")) {
  console.error(
    `check-milestone-marker: packages/contracts/package.json has no semver "version", got ${JSON.stringify(pkgVersion)}`,
  );
  process.exit(1);
}

// CLAUDE.md's contracts heading is REQUIRED: builders read it as the frozen
// baseline. Mismatch or absence both fail.
const contractsHeading = claudeMd.match(/^## Contracts status: FROZEN at v(\d+\.\d+\.\d+)\b/m);
if (!contractsHeading) {
  console.error(
    'check-milestone-marker: CLAUDE.md has no "## Contracts status: FROZEN at v<x.y.z>" heading. ' +
      "Builders read that line as the frozen baseline; it must exist and name a version.",
  );
  process.exit(1);
}
if (contractsHeading[1] !== pkgVersion) {
  console.error(
    `check-milestone-marker: CONTRACTS VERSION DISAGREEMENT\n` +
      `  packages/contracts/package.json : ${pkgVersion}\n` +
      `  CLAUDE.md contracts heading     : ${contractsHeading[1]}\n\n` +
      `Builders read CLAUDE.md's line as the frozen baseline. Update the heading\n` +
      `(and the addendum prose beneath it, not just the number), then re-run.`,
  );
  process.exit(1);
}

// The migration high-water marks on the same heading line, against the actual
// files on disk — the runner, not the prose.
const { readdirSync } = await import("node:fs");
const highest = (dir) =>
  Math.max(
    0,
    ...readdirSync(join(repoRoot, dir))
      .map((f) => f.match(/^(\d{4})_/))
      .filter(Boolean)
      .map((m) => Number(m[1])),
  );
const migrations = claudeMd.match(/migrations through sqlite (\d{4}) \/ postgres (\d{4})/);
if (!migrations) {
  console.error(
    'check-milestone-marker: CLAUDE.md\'s contracts heading must carry "migrations through sqlite NNNN / postgres NNNN".',
  );
  process.exit(1);
}
const sqliteHigh = highest("packages/contracts/sqlite");
const postgresHigh = highest("packages/contracts/postgres");
if (Number(migrations[1]) !== sqliteHigh || Number(migrations[2]) !== postgresHigh) {
  console.error(
    `check-milestone-marker: MIGRATION HIGH-WATER DISAGREEMENT\n` +
      `  on disk   : sqlite ${String(sqliteHigh).padStart(4, "0")} / postgres ${String(postgresHigh).padStart(4, "0")}\n` +
      `  CLAUDE.md : sqlite ${migrations[1]} / postgres ${migrations[2]}`,
  );
  process.exit(1);
}

// RESUME.md is a point-in-time snapshot, so the claim may legitimately be
// absent (warn) — but a PRESENT claim that disagrees is exactly the stale copy
// this check exists for (fail).
const resumeMd = read("docs/RESUME.md");
const resumeClaim = resumeMd.match(/Contracts are FROZEN at v(\d+\.\d+\.\d+)\b/);
if (resumeClaim && resumeClaim[1] !== pkgVersion) {
  console.error(
    `check-milestone-marker: docs/RESUME.md claims contracts v${resumeClaim[1]} but ` +
      `packages/contracts/package.json says ${pkgVersion}. A fresh session reads RESUME.md ` +
      `first; update the claim or drop it.`,
  );
  process.exit(1);
}
if (!resumeClaim) {
  console.warn(
    "check-milestone-marker: note — docs/RESUME.md carries no 'Contracts are FROZEN at v…' claim; nothing to cross-check there.",
  );
}

console.log(
  `check-milestone-marker: ok — milestone ${marker}; contracts v${pkgVersion}; migrations sqlite ${String(sqliteHigh).padStart(4, "0")} / postgres ${String(postgresHigh).padStart(4, "0")}`,
);
