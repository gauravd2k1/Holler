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

console.log(`check-milestone-marker: ok — CLAUDE.md and .claude/current-milestone both say milestone ${marker}`);
