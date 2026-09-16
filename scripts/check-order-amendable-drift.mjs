#!/usr/bin/env node
// The set of order statuses in which a LINE may still be added, removed or
// resized is declared in four places that no compiler can compare: a Zod-side
// array in packages/contracts/src/types/order.ts, a Go slice in
// packages/contracts/go/order.go, a Rust array at the edge
// (edge/database/src/repo.rs), and a line of English in openapi.yaml.
//
// THIS CHECK EXISTS BECAUSE THEY DID DISAGREE, AND NOTHING WENT RED. The edge
// has allowed a line to be added through PREPARING since #132-A; the cloud
// enforced DRAFT only. That mismatch was invisible for months because a
// SECOND defect — order transitions rejected or silently swallowed cloud-side,
// contracts 0.8.3 / ADR-028 — left every cloud order in DRAFT, so the cloud's
// rule was never reached. Defect A masked defect B. A drift check is the only
// thing that would have shown it, because every test on either side passed.
//
// What a mismatch costs: the cloud's refusal is a permanent 409 and the edge's
// outbox is at-least-once with per-aggregate ordering, so ONE refused line
// wedges every later row of that order. It surfaces as a blocked-row banner on
// a till hours later, not as a red build.
//
// The set is the EDGE's (§50.1: the edge is the authority for order
// transactions). The cloud replays what the outlet did; it does not get an
// amendable set of its own.
//
// Run: node scripts/check-order-amendable-drift.mjs

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

// The set every surface must declare, in the order src/types/order.ts
// declares it. Order is compared too: four surfaces listing the same four
// statuses in different orders is not drift, but it is how a reader starts
// believing the lists are unrelated.
const EXPECTED = ["DRAFT", "CONFIRMED", "SENT_TO_KITCHEN", "PREPARING"];

const failures = [];

function read(rel) {
  return readFileSync(join(ROOT, rel), "utf8");
}

/** Extracts the quoted upper-case members of a block, or records why it could
 * not. A surface this script cannot PARSE is a failure, never a pass: a
 * silently unmatched regex is how a check goes on reporting green over a
 * region it stopped looking at. */
function members(source, pattern, label, token = /([A-Z][A-Z_]+)/g) {
  const block = source.match(pattern);
  if (!block) {
    failures.push(
      `${label}: the declaration was not found — this check cannot verify what it cannot locate. If it moved, point this script at the new place IN THE SAME COMMIT.`,
    );
    return null;
  }
  const found = [...block[1].matchAll(token)].map((m) => m[1]);
  if (found.length === 0) {
    failures.push(`${label}: the declaration was found but held no statuses`);
    return null;
  }
  return found;
}

const surfaces = {
  "Zod ORDER_ITEM_AMENDABLE_STATUSES (src/types/order.ts)": members(
    read("packages/contracts/src/types/order.ts"),
    /ORDER_ITEM_AMENDABLE_STATUSES\s*=\s*\[([\s\S]*?)\]/,
    "src/types/order.ts",
  ),
  "Go OrderItemAmendableStatuses (go/order.go)": members(
    read("packages/contracts/go/order.go"),
    /OrderItemAmendableStatuses\s*=\s*\[\]OrderStatus\{([\s\S]*?)\}/,
    "go/order.go",
    /(OrderStatus[A-Za-z]+)/g,
  ),
  "Rust ORDER_ITEM_AMENDABLE_STATUSES (edge/database/src/repo.rs)": members(
    read("edge/database/src/repo.rs"),
    /ORDER_ITEM_AMENDABLE_STATUSES:\s*\[&str;\s*\d+\]\s*=\s*\[([\s\S]*?)\]/,
    "edge/database/src/repo.rs",
  ),
  "OpenAPI /orders/{id}/items": members(
    read("packages/contracts/openapi/openapi.yaml"),
    /amendable:\s*\[([^\]]*)\]/,
    "openapi.yaml",
  ),
};

// The Go slice spells its members as Go identifiers (OrderStatusSentToKitchen),
// not as wire strings, so it is compared on the identifiers' suffixes.
const GO_IDENTIFIERS = {
  DRAFT: "OrderStatusDraft",
  CONFIRMED: "OrderStatusConfirmed",
  SENT_TO_KITCHEN: "OrderStatusSentToKitchen",
  PREPARING: "OrderStatusPreparing",
};

for (const [label, found] of Object.entries(surfaces)) {
  if (!found) continue;
  const expected = label.startsWith("Go ")
    ? EXPECTED.map((m) => GO_IDENTIFIERS[m])
    : EXPECTED;
  const missing = expected.filter((m) => !found.includes(m));
  const extra = found.filter((m) => !expected.includes(m));
  if (missing.length > 0) {
    failures.push(
      `${label} is MISSING ${missing.join(", ")} — a status one side treats as amendable and the other refuses is a permanent 409 that wedges an order's whole outbox queue`,
    );
  }
  if (extra.length > 0) {
    failures.push(`${label} declares ${extra.join(", ")}, which no other surface has`);
  }
  const ordered = found.filter((m) => expected.includes(m));
  if (missing.length === 0 && extra.length === 0 && ordered.join(",") !== expected.join(",")) {
    failures.push(
      `${label} lists the set in a different order (${ordered.join(", ")}) — same members, but four lists that look unrelated stop being read as one set`,
    );
  }
}

// The cloud must CONSUME the contracts helper rather than restating the rule.
// A status equality test against a literal is exactly the declaration this
// check was written to remove, and it would pass every comparison above while
// reintroducing the defect.
const cloudService = read("backend/internal/ordering/service.go");
if (!/contracts\.IsOrderItemAmendable\(/.test(cloudService)) {
  failures.push(
    "backend/internal/ordering/service.go does not call contracts.IsOrderItemAmendable — the cloud's line-amendment rule must be READ from contracts, never restated",
  );
}

if (failures.length > 0) {
  console.error("order amendable-status drift check FAILED:\n");
  for (const f of failures) console.error(`  - ${f}`);
  console.error(
    `\n${failures.length} problem(s). The set is declared in four places and they must agree (contracts 0.8.3, ADR-028).`,
  );
  process.exit(1);
}

console.log(
  `order amendable-status drift check OK — ${EXPECTED.join(", ")} declared identically by ${Object.keys(surfaces).length} surfaces, and the cloud reads it from contracts.`,
);
