#!/usr/bin/env node
// Fails the build when a bounded context grows its own SQLSTATE knowledge
// instead of using backend/internal/platform/storage.
//
// WHY THIS EXISTS. Seven contexts each carried a private copy of the same
// helper -- `const pgUniqueViolation = "23505"` plus an `isUniqueViolation`
// -- and every one of them handled 23505 while NONE handled 23503,
// foreign_key_violation. So a replayed order item referencing a menu_item the
// cloud had never held came back 500 "internal_error": a permanent
// client-data fault reported as a transient server fault. The edge retried it
// forever and, because the general outbox drains in order, 114 rows behind it
// were never attempted at all (docs/m6-a1-sink-audit.md).
//
// Seven copies did not disagree by accident: nobody edits seven files when
// they learn a new SQLSTATE. Consolidating them fixes today's hole; this
// check is what stops the eighth copy appearing next milestone, when the
// reason has been forgotten and copying the neighbouring context is the
// obvious move.
//
// Same shape as every other guard here: a claim in prose is worth nothing
// unless something fails when it goes false (docs/retro.md, 2026-08-20).

import { readdirSync, readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative } from "node:path";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const backendInternal = join(repoRoot, "backend", "internal");

// The one package allowed to know a SQLSTATE literal.
const CLASSIFIER_DIR = join(backendInternal, "platform", "storage");

// A five-character SQLSTATE literal in Go source. Class 23 (integrity
// constraint violation) is what this check is really about, but any hard
// coded SQLSTATE outside the classifier is the same mistake.
const SQLSTATE_LITERAL = /"(2[0-9A-Z]{4}|40001|55P03)"/g;
const LOCAL_HELPER = /func\s+(isUniqueViolation|isForeignKeyViolation|isCheckViolation)\s*\(/g;

function goFilesUnder(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      out.push(...goFilesUnder(full));
    } else if (entry.endsWith(".go")) {
      out.push(full);
    }
  }
  return out;
}

let files;
try {
  files = goFilesUnder(backendInternal);
} catch (error) {
  // Fail loudly rather than skip. A check that passes when it cannot find its
  // inputs is the defect it exists to catch, one level up.
  console.error(`check-sqlstate-classifier: cannot read ${backendInternal}: ${error.message}`);
  process.exit(1);
}

if (files.length === 0) {
  console.error("check-sqlstate-classifier: found no Go files under backend/internal — refusing to pass.");
  process.exit(1);
}

const violations = [];
let scanned = 0;

for (const file of files) {
  // The classifier itself is the point of the exercise, and its own test
  // constructs pgconn.PgError values with real codes.
  if (file.startsWith(CLASSIFIER_DIR)) continue;
  scanned += 1;

  const source = readFileSync(file, "utf8");
  const rel = relative(repoRoot, file).replaceAll("\\", "/");
  const lines = source.split("\n");

  lines.forEach((line, index) => {
    // A test may legitimately assert on a status or a constraint name; what
    // it may not do is re-implement the mapping. So the literal check skips
    // test files while the helper check does not.
    if (!file.endsWith("_test.go")) {
      for (const match of line.matchAll(SQLSTATE_LITERAL)) {
        violations.push({
          file: rel,
          line: index + 1,
          text: line.trim(),
          why: `SQLSTATE literal ${match[1]} outside internal/platform/storage`,
        });
      }
    }
    for (const match of line.matchAll(LOCAL_HELPER)) {
      violations.push({
        file: rel,
        line: index + 1,
        text: line.trim(),
        why: `local ${match[1]} — use storage.Classify / storage.IsUniqueViolation`,
      });
    }
  });
}

if (violations.length > 0) {
  console.error("check-sqlstate-classifier: SQLSTATE KNOWLEDGE OUTSIDE THE CLASSIFIER\n");
  for (const v of violations) {
    console.error(`  ${v.file}:${v.line}  ${v.why}`);
    console.error(`      ${v.text}`);
  }
  console.error(
    "\nbackend/internal/platform/storage is the only place in this module that may know a\n" +
      "SQLSTATE. Seven contexts once carried private copies of the same unique-violation\n" +
      "helper and not one of them handled 23503, so a foreign-key violation on a replayed\n" +
      "row came back 500 and wedged the outbox behind it. Call storage.Classify (or\n" +
      "storage.Wrap in a repository write path) instead of adding an eighth copy.",
  );
  process.exit(1);
}

console.log(
  `check-sqlstate-classifier: ok — ${scanned} Go file(s) under backend/internal carry no SQLSTATE ` +
    "literal or local integrity-error helper; internal/platform/storage is the sole classifier.",
);
