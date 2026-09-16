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
// AND IT COMPARES THE KDS's OWN FORWARD FLOW (D25). The kitchen screen cannot
// call a Tauri command — it is a browser page on a phone talking WebSocket to
// the LAN hub — so deleting its copy the way the till's was deleted is not
// available without a protocol change. What IS available is comparing it, so
// the copy stops being unchecked. `apps/kds/src/domain/kotTransitions.ts`
// declares a FORWARD_FLOW chain rather than a table, so this is not a literal
// equality check: the chain is compared against the edge's table in BOTH
// directions, so a state added, removed or reordered on either side fails.
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

// ---- D25: the KDS's forward flow against the edge's table ----------------
//
// The edge's table is the authority. The KDS offers ONE forward move per
// status and never CANCELLED (that is an order-level action, not a cook's
// button), so its chain is a derived view of the table and is checked as one.

const repoRs = read("edge/database/src/repo.rs");
const tableBlock = repoRs.match(
  /LEGAL_KOT_TRANSITIONS:\s*&\[\(&str,\s*&\[&str\]\)\]\s*=\s*&\[([\s\S]*?)\n\];/,
);
let edgeTable = null;
if (!tableBlock) {
  failures.push(
    "edge/database/src/repo.rs: LEGAL_KOT_TRANSITIONS was not found — this check cannot compare against a table it cannot locate.",
  );
} else {
  edgeTable = new Map();
  for (const row of tableBlock[1].matchAll(/\("([A-Z_]+)",\s*&\[([^\]]*)\]\)/g)) {
    const tos = [...row[2].matchAll(/"([A-Z_]+)"/g)].map((m) => m[1]);
    edgeTable.set(row[1], tos);
  }
  if (edgeTable.size === 0) {
    failures.push("edge/database/src/repo.rs: LEGAL_KOT_TRANSITIONS parsed to nothing");
    edgeTable = null;
  }
}

const kdsSource = read("apps/kds/src/domain/kotTransitions.ts");
const flowBlock = kdsSource.match(/FORWARD_FLOW:\s*readonly KotStatus\[\]\s*=\s*\[([\s\S]*?)\]/);
let flow = null;
if (!flowBlock) {
  failures.push(
    "apps/kds/src/domain/kotTransitions.ts: FORWARD_FLOW was not found — this check cannot verify what it cannot locate.",
  );
} else {
  flow = [...flowBlock[1].matchAll(/"([A-Z_]+)"/g)].map((m) => m[1]);
  if (flow.length === 0) {
    failures.push("apps/kds/src/domain/kotTransitions.ts: FORWARD_FLOW parsed to nothing");
    flow = null;
  }
}

/** The edge's forward successor for a status: its legal moves minus
 * CANCELLED, which no KDS button offers. */
function forwardSuccessor(status) {
  const tos = (edgeTable.get(status) ?? []).filter((t) => t !== "CANCELLED");
  return tos;
}

if (edgeTable && flow) {
  // DIRECTION 1: every step the KDS offers must be legal at the edge. A step
  // that is not is a button whose press the edge refuses — the D14 defect,
  // arriving on the kitchen screen instead of the till.
  for (let i = 0; i < flow.length - 1; i += 1) {
    const [from, to] = [flow[i], flow[i + 1]];
    if (!(edgeTable.get(from) ?? []).includes(to)) {
      failures.push(
        `apps/kds: FORWARD_FLOW steps ${from} -> ${to}, which the edge's LEGAL_KOT_TRANSITIONS does not allow. The cook would press a button the edge refuses.`,
      );
    }
  }
  // DIRECTION 2: every forward move the EDGE allows must be the one the KDS
  // offers. Without this, a state inserted into the edge's table is simply
  // skipped by the kitchen screen — no error, no button, tickets that stop
  // advancing — and nothing says so.
  for (const [from] of edgeTable) {
    const successors = forwardSuccessor(from);
    if (successors.length === 0) continue;
    if (successors.length > 1) {
      failures.push(
        `edge: ${from} has more than one non-CANCELLED successor (${successors.join(", ")}), which a single-next-step FORWARD_FLOW cannot express. The KDS needs a real table before this lands.`,
      );
      continue;
    }
    const idx = flow.indexOf(from);
    if (idx === -1) {
      failures.push(
        `apps/kds: FORWARD_FLOW never mentions ${from}, which the edge can advance to ${successors[0]}. A ticket in that state would sit with no button.`,
      );
    } else if (flow[idx + 1] !== successors[0]) {
      failures.push(
        `apps/kds: after ${from} the KDS offers ${flow[idx + 1] ?? "nothing"}, the edge allows ${successors[0]}.`,
      );
    }
  }
  // The end of the chain must really be the end at the edge.
  const last = flow[flow.length - 1];
  if (forwardSuccessor(last).length > 0) {
    failures.push(
      `apps/kds: FORWARD_FLOW ends at ${last}, but the edge can still advance it to ${forwardSuccessor(last).join(", ")}.`,
    );
  }
}

if (failures.length > 0) {
  console.error("kitchen live-update drift check FAILED:\n");
  for (const f of failures) console.error(`  - ${f}`);
  console.error(`\n${failures.length} problem(s).`);
  process.exit(1);
}

console.log(
  `kitchen live-update drift check OK — both sides name "${rustName}", the till reads the table from the edge, ` +
    `and the KDS's forward flow (${flow ? flow.join(" -> ") : "?"}) agrees with it in both directions.`,
);
