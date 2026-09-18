#!/usr/bin/env node
// B2-T3 — every react-query key in the POS carries a RULING about staleness.
//
// THE DEFECT THIS EXISTS FOR. The till is no longer the only writer of its own
// state. The KDS writes KOT status over the LAN socket, the captain writes
// orders over HTTP, and the config pull writes the whole menu — all of them
// inside the POS process, none of them through a Tauri mutation in the
// webview, so nothing invalidates anything. `docs/m7-b2-sinks.md` enumerates
// the four sinks and rules on every key against them.
//
// It has already happened twice under two different names: the kitchen panel
// that "only updates when you collapse and re-expand it", and a waiter's order
// that never reached the order list. `docs/backlog.md` predicts a third.
//
// WHAT THIS GUARD ACTUALLY CHECKS, STATED HONESTLY. It cannot prove a screen
// is fresh — a listener's mount point decides that and no static check can see
// it. What it can do, and the reason it has teeth, is refuse a key that nobody
// has ruled on:
//
//   1. Every key in `queryKeys` must appear in RULINGS below. A key added
//      without a ruling fails the build. THIS IS THE ONE THAT CATCHES THE
//      NEXT INSTANCE — the two defects above were both new surfaces reading an
//      existing key, and both would have been caught here at review time.
//   2. A key ruled `poll` must have a `refetchInterval` in its hook.
//   3. A key ruled `event` must appear inside a kitchen-event listener.
//   4. A key ruled `exempt` must carry a reason, so "nothing writes this from
//      outside" is a claim someone made, not an omission.
//
// It does NOT check that an `event` key's listener is mounted where it needs
// to be. That is the live defect as of this commit and is deliberately left
// visible in the rulings rather than papered over.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const QUERIES = join(repoRoot, "apps/pos/src/lib/queries.ts");
const ORDER_LIST = join(repoRoot, "apps/pos/src/components/OrderListScreen.tsx");

// The ruling for every key, from docs/m7-b2-sinks.md. `sinks` names the
// non-webview writers that can reach it, so a reader can check the ruling
// rather than trust it.
const RULINGS = {
  // --- reachable, covered by polling -------------------------------------
  blockedOutboxRows: { mechanism: "poll", sinks: ["outbox pump"] },
  unroutableOutboxRows: { mechanism: "poll", sinks: ["outbox pump"] },
  persistentlyFailingOutboxRows: { mechanism: "poll", sinks: ["outbox pump"] },
  failedPrintJobs: { mechanism: "poll", sinks: ["print spool (async completion)"] },
  currentStock: { mechanism: "poll", sinks: ["none -- polled for the till's own async deduction"] },

  // --- reachable, covered by the kitchen event ---------------------------
  // NOTE THE KNOWN GAP: the listener lives in KotsPanel, which is mounted only
  // while a Kitchen panel is expanded. This guard cannot see mount points; the
  // gap is recorded in docs/m7-b2-sinks.md and is B2-T1's work.
  kots: { mechanism: "event", sinks: ["LAN set_kot_status", "captain send"] },

  // --- reachable and NOT yet covered -------------------------------------
  // Declared with `mechanism: "uncovered"` rather than omitted: an omission
  // reads as an oversight, a declaration reads as a debt with an owner.
  orders: {
    mechanism: "uncovered",
    sinks: ["LAN set_kot_status", "captain create/add/send"],
    reason: "B2-T1. Covered only via the KotsPanel listener, which is unmounted when no panel is expanded.",
  },
  order: {
    mechanism: "uncovered",
    sinks: ["captain create/add/send"],
    reason: "B2-T1.",
  },
  tables: {
    mechanism: "uncovered",
    sinks: ["captain create (opens a table session)"],
    reason: "B2-T1.",
  },
  blockedReplays: {
    mechanism: "uncovered",
    sinks: ["outbox pump"],
    reason:
      "B2-T1. Its three siblings are on a 15s poll and this one is not -- found by this guard on its first run, " +
      "against a ruling the author had written as 'poll' from memory.",
  },
  // SINK 5, FOUND BY THIS GUARD ON ITS FIRST RUN. `pull_and_apply_aggregator_orders`
  // (edge/sync/src/aggregator.rs:75) runs in the A5 worker loop
  // (edge/sync/src/worker.rs:313) and writes `aggregator_order` and `order`
  // rows. The hand-written enumeration in docs/m7-b2-sinks.md missed it
  // entirely; refusing an unruled key is what surfaced it.
  unacceptedAggregatorOrders: {
    mechanism: "uncovered",
    sinks: ["aggregator pull (sink 5)"],
    reason:
      "B2-T1. An aggregator order lands in the edge database from the worker loop and reaches no screen until " +
      "something else refetches. This is demo step 6's path.",
  },
  purchaseOrderReceiptProgress: {
    mechanism: "uncovered",
    sinks: ["config apply (purchase_order travels on GET /sync/config since 0.6.0)"],
    reason: "B2-T1 step 3.",
  },
  menuItems: { mechanism: "uncovered", sinks: ["config apply"], reason: "B2-T1 step 3." },
  menuCategories: { mechanism: "uncovered", sinks: ["config apply"], reason: "B2-T1 step 3." },
  menuItemVariants: { mechanism: "uncovered", sinks: ["config apply"], reason: "B2-T1 step 3." },
  stations: { mechanism: "uncovered", sinks: ["config apply"], reason: "B2-T1 step 3." },
  discountDefinitions: { mechanism: "uncovered", sinks: ["config apply"], reason: "B2-T1 step 3." },
  outletIdentity: { mechanism: "uncovered", sinks: ["config apply"], reason: "B2-T1 step 3." },

  // --- not reachable from any sink ---------------------------------------
  grnGaps: { mechanism: "exempt", reason: "A GRN gap is recorded by the till's own receiving flow, which invalidates at its call site." },
  invoices: { mechanism: "exempt", reason: "Written only by till-side billing mutations, which invalidate at their call sites." },
  payments: { mechanism: "exempt", reason: "As invoices." },
  cashShift: { mechanism: "exempt", reason: "As invoices." },
  stockDeductionGaps: { mechanism: "exempt", reason: "As currentStock." },
  kotStatusTransitions: { mechanism: "exempt", reason: "A pure derivation of a KOT already on screen." },
  stockCount: { mechanism: "exempt", reason: "Stock counts are authored on the till alone." },
  stockCountLines: { mechanism: "exempt", reason: "As stockCount." },
  stockCountVarianceReport: { mechanism: "exempt", reason: "As stockCount." },
};

const fail = [];

const queriesSrc = readFileSync(QUERIES, "utf8");
const orderListSrc = readFileSync(ORDER_LIST, "utf8");

// ---- parse the queryKeys object ------------------------------------------
const keysBlock = queriesSrc.match(/export const queryKeys = \{([\s\S]*?)\n\};/);
if (!keysBlock) {
  console.error("check-query-key-freshness: could not find `export const queryKeys = {` in apps/pos/src/lib/queries.ts.");
  console.error("The guard reads that object to know which keys exist. If it moved, point this script at it -- do not delete the check.");
  process.exit(1);
}
const declaredKeys = [...keysBlock[1].matchAll(/^\s{2}([A-Za-z0-9_]+)\s*:/gm)].map((m) => m[1]);

if (declaredKeys.length === 0) {
  console.error("check-query-key-freshness: parsed ZERO keys out of queryKeys. A guard that checks nothing is worse than no guard.");
  process.exit(1);
}

// ---- 1. every key carries a ruling ---------------------------------------
for (const key of declaredKeys) {
  if (!(key in RULINGS)) {
    fail.push(
      `queryKeys.${key} has no ruling in scripts/check-query-key-freshness.mjs.\n` +
        `      Decide which of the four sinks in docs/m7-b2-sinks.md can write the rows behind it,\n` +
        `      then add a ruling: "poll", "event", "uncovered" (with a reason naming the track), or\n` +
        `      "exempt" (with a reason saying why no external writer reaches it).`,
    );
  }
}
for (const key of Object.keys(RULINGS)) {
  if (!declaredKeys.includes(key)) {
    fail.push(`RULINGS names "${key}", which no longer exists in queryKeys. Remove the stale ruling -- an exemption that outlives its reason is a silenced failure.`);
  }
}

// ---- 2. a `poll` ruling needs a real refetchInterval ----------------------
// The hook for key `foo` is the block using `queryKeys.foo`; a refetchInterval
// must appear in the same useQuery call.
function hookBodyFor(key) {
  const idx = queriesSrc.indexOf(`queryKeys.${key}`);
  if (idx === -1) return null;
  const end = queriesSrc.indexOf("\n}", idx);
  return queriesSrc.slice(idx, end === -1 ? queriesSrc.length : end);
}

for (const [key, ruling] of Object.entries(RULINGS)) {
  if (ruling.mechanism !== "poll") continue;
  const body = hookBodyFor(key);
  if (body === null) {
    fail.push(`queryKeys.${key} is ruled "poll" but is never used in a hook in queries.ts.`);
    continue;
  }
  if (!/refetchInterval\s*:/.test(body)) {
    fail.push(
      `queryKeys.${key} is ruled "poll" but its hook has no refetchInterval.\n` +
        `      Sinks that can write it: ${(ruling.sinks ?? []).join(", ")}.\n` +
        `      Either restore the poll or change the ruling -- do not leave the ruling saying one thing and the code another.`,
    );
  }
}

// ---- 3. an `event` ruling needs the key inside a kitchen-event listener ---
for (const [key, ruling] of Object.entries(RULINGS)) {
  if (ruling.mechanism !== "event") continue;
  const listener = orderListSrc.match(/onKitchenChanged\(\(\) => \{([\s\S]*?)\}\)/);
  if (!listener || !listener[1].includes(`queryKeys.${key}`)) {
    fail.push(
      `queryKeys.${key} is ruled "event" but is not invalidated inside an onKitchenChanged callback in OrderListScreen.tsx.\n` +
        `      A key whose only freshness mechanism is an event nobody fires for it is stale by construction.`,
    );
  }
}

// ---- 4. `uncovered` and `exempt` must carry a reason ----------------------
for (const [key, ruling] of Object.entries(RULINGS)) {
  if (ruling.mechanism !== "uncovered" && ruling.mechanism !== "exempt") continue;
  if (!ruling.reason || ruling.reason.trim() === "") {
    fail.push(`queryKeys.${key} is ruled "${ruling.mechanism}" with no reason. The reason IS the ruling.`);
  }
}

if (fail.length > 0) {
  console.error("query-key freshness check FAILED:\n");
  for (const f of fail) console.error(`  - ${f}\n`);
  console.error("See docs/m7-b2-sinks.md for the sink enumeration these rulings are made against.");
  process.exit(1);
}

const counts = Object.values(RULINGS).reduce((acc, r) => {
  acc[r.mechanism] = (acc[r.mechanism] ?? 0) + 1;
  return acc;
}, {});
console.log(
  `query-key freshness check OK — ${declaredKeys.length} keys, all ruled: ` +
    `${counts.poll ?? 0} polled, ${counts.event ?? 0} event-driven, ` +
    `${counts.uncovered ?? 0} UNCOVERED (B2-T1), ${counts.exempt ?? 0} exempt.`,
);
