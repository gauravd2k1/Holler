#!/usr/bin/env node
// The till listens to its own kitchen hub over a Tauri event (D14). The event
// NAME is spelled once in Rust and once in TypeScript, and the two must agree.
//
// WHY THIS NEEDS A CHECK. A mismatched event name does not throw, does not
// warn, and does not fail a build: `listen` simply never fires. The symptom is
// the exact defect this mechanism was built to fix — a till showing a stale
// kitchen status — so the failure mode of the fix is indistinguishable from
// the defect, and it would be diagnosed as "the live update does not work"
// rather than "the string is different".
//
// It also pins the SECOND half of D14: the KOT transition table must be read
// from the edge and must not reappear as a literal in the till's TypeScript.
// It lived there for a milestone with a comment promising it mirrored the Rust
// one, which is a promise nothing could check.
//
// Run: node scripts/check-kitchen-event-drift.mjs

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const read = (rel) => readFileSync(join(ROOT, rel), "utf8");
const failures = [];

function one(source, pattern, label) {
  const m = source.match(pattern);
  if (!m) {
    failures.push(
      `${label}: the event-name declaration was not found — this check cannot verify what it cannot locate. If it moved, point this script at it IN THE SAME COMMIT.`,
    );
    return null;
  }
  return m[1];
}

const rustName = one(
  read("apps/pos/src-tauri/src/lib.rs"),
  /KITCHEN_CHANGED_EVENT:\s*&str\s*=\s*"([^"]+)"/,
  "apps/pos/src-tauri/src/lib.rs",
);
const tsName = one(
  read("apps/pos/src/lib/kitchenEvents.ts"),
  /KITCHEN_CHANGED_EVENT\s*=\s*"([^"]+)"/,
  "apps/pos/src/lib/kitchenEvents.ts",
);

if (rustName && tsName && rustName !== tsName) {
  failures.push(
    `the event name differs: Rust emits "${rustName}", the till listens for "${tsName}". Nothing throws on a mismatch — the listener just never fires, and the symptom is the stale kitchen status this event exists to fix.`,
  );
}

// The transition table must come from the edge, not from a literal here.
const kitchenTs = read("apps/pos/src/domain/kitchen.ts");
if (/["']ACKNOWLEDGED["']\s*:\s*\[/.test(kitchenTs) || /NEW:\s*\[\s*["']ACKNOWLEDGED["']/.test(kitchenTs)) {
  failures.push(
    "apps/pos/src/domain/kitchen.ts declares a KOT transition table again. It must be read from the edge via list_kot_status_transitions — a UI that keeps its own copy offers moves the edge refuses, which is what VV-009 observed.",
  );
}
if (!/list_kot_status_transitions/.test(read("apps/pos/src/lib/tauri.ts"))) {
  failures.push("apps/pos/src/lib/tauri.ts no longer calls list_kot_status_transitions");
}
if (!/legal_kot_transitions/.test(read("apps/pos/src-tauri/src/commands/kitchen.rs"))) {
  failures.push(
    "apps/pos/src-tauri/src/commands/kitchen.rs no longer reads repo::legal_kot_transitions — the command must SERVE the edge's table, never restate it",
  );
}

if (failures.length > 0) {
  console.error("kitchen live-update drift check FAILED:\n");
  for (const f of failures) console.error(`  - ${f}`);
  console.error(`\n${failures.length} problem(s).`);
  process.exit(1);
}

console.log(
  `kitchen live-update drift check OK — both sides name "${rustName}", and the transition table is read from the edge.`,
);
