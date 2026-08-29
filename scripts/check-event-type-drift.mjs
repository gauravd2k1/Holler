#!/usr/bin/env node
// Event-type drift check between the frozen contract and the Rust edge crates.
//
// WHY THIS EXISTS: packages/contracts has TypeScript and Go bindings, and a
// drift test asserts those two agree. The Rust crates (edge/sync,
// apps/pos/src-tauri) have no generated binding — deferred until a fourth Rust
// consumer justifies one (ADR-011 addendum, 0.2.2) — so they carry event_type
// values as bare string literals. Nothing links those literals to the contract
// at compile time, and a mismatch fails SILENTLY at replay: the POS writes an
// outbox row the cloud never recognises, with no error anywhere.
//
// The check runs in BOTH directions:
//   forward  — every event-type-shaped literal in Rust must exist in the
//              frozen list, catching an invented or misspelled string.
//   backward — every frozen event type must either appear in Rust or be listed
//              in NOT_YET_EMITTED below, catching a contract addition the edge
//              silently never adopted.
//
// Run: node scripts/check-event-type-drift.mjs

import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { join, relative } from "node:path";

const REPO_ROOT = new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const EVENTS_TS = join(REPO_ROOT, "packages/contracts/src/types/events.ts");
// Every Rust crate that names an event type. edge/database was missing from
// this list until 0.2.3 — the crate that BUILDS the outbox payloads was the one
// the check never scanned, so its literals were unguarded in both directions
// while the check reported green. If a new Rust crate touches event types, it
// belongs here.
// Milestone 2 adds edge/printer (the spool) and edge/device (the KDS LAN
// server, which relays KOT state and names event types in its messages).
// Directories that do not exist yet are skipped rather than fatal, so this list
// can name a crate before its track lands.
const RUST_ROOTS = [
  join(REPO_ROOT, "edge/database/src"),
  join(REPO_ROOT, "edge/sync/src"),
  join(REPO_ROOT, "edge/printer/src"),
  join(REPO_ROOT, "edge/device/src"),
  join(REPO_ROOT, "apps/pos/src-tauri/src"),
];

// Frozen event types the Rust edge legitimately does not emit yet. Each needs a
// reason — an empty justification here defeats the backward check.
// Emptied at Milestone 2: KOTCreated, KOTStatusChanged and OrderReady were the
// last deferred types, and the edge now emits all three. Every frozen event
// type is live, so the backward check no longer has an exemption to grant —
// which is the state this map exists to drive toward, not a reason to delete it.
// A future contract addition lands here with a reason, or it fails the check.
// Milestone 3 (contracts 0.4.0, ADR-016) added the five billing events ahead of
// the tracks that emit them. Each entry named the track that must remove it —
// an entry that outlives its track is the signal this map exists to produce.
//
// Emptied again at M4/T5. Two removals, for the same reason and on the same
// day: StockCountOpened/StockCountCompleted, now emitted by
// `Db::open_stock_count_with_outbox`/`complete_stock_count_with_outbox`; and
// the five M3 billing entries, which the edge had already been emitting since
// M3 went code-complete. Those five had gone stale silently, because an entry
// whose type IS seen in Rust is skipped before the exemption is consulted — so
// the map can rot without ever failing the check it belongs to. Read that as a
// standing instruction: when a track lands, delete its entry here in the same
// change, because nothing else will tell you.
const NOT_YET_EMITTED = {};

function frozenEventTypes() {
  const source = readFileSync(EVENTS_TS, "utf-8");
  const block = source.match(/OUTBOX_EVENT_TYPES\s*=\s*\[([\s\S]*?)\]\s*as const/);
  if (!block) {
    throw new Error(`could not find OUTBOX_EVENT_TYPES in ${EVENTS_TS}`);
  }
  return [...block[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

function rustFiles(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      out.push(...rustFiles(full));
    } else if (entry.endsWith(".rs")) {
      out.push(full);
    }
  }
  return out;
}

// An event-type-shaped literal: PascalCase, two or more capitalised words, so
// "OrderCreated" and "KOTCreated" match while "order_id" and "Db" do not.
// Each word needs a lowercase tail, so an ALL-CAPS domain constant like
// "TAKEAWAY" is not read as eight one-letter words. KOT is the one acronym the
// contract froze, so it stays an explicit alternative.
const EVENT_SHAPED = /"((?:[A-Z][a-z0-9]+|KOT){2,})"/g;

// The line looks like it is writing an outbox event. Deliberately broad: a
// false positive costs one FROZEN entry or one NOT_YET_EMITTED entry with a
// reason, while a false negative is a type that replays nowhere and says
// nothing -- and this file exists because that already happened.
// Literals that are DELIBERATELY not frozen: negative test fixtures asserting
// that an unknown event type is rejected. Declared with a reason rather than
// filtered by a pattern, so a real miss can never hide behind "it looked like a
// test" -- the SINGLE_STORE_MIGRATIONS / NOT_YET_EMITTED discipline.
const NEGATIVE_TEST_LITERALS = {
  NotAFrozenEvent:
    "edge/sync route test: asserts an unrecognised event type is refused, so it must never be frozen.",
  NotAFrozenOrderEvent:
    "edge/sync route test: the same assertion with an Order-prefixed name, which the old prefix filter did flag.",
};

const EVENT_CONTEXT = /outbox|event_type|EventType|enqueue|emit|OutboxEvent/i;

function main() {
  const frozen = frozenEventTypes();
  const frozenSet = new Set(frozen);
  const seenInRust = new Map(); // event type -> [locations]
  const unknown = []; // { literal, location }

  for (const root of RUST_ROOTS) {
    if (!existsSync(root)) continue; // crate not created yet — see RUST_ROOTS
    for (const file of rustFiles(root)) {
      const source = readFileSync(file, "utf-8");
      source.split("\n").forEach((line, i) => {
        if (line.trimStart().startsWith("//")) return; // comments are not contracts
        for (const [, literal] of line.matchAll(EVENT_SHAPED)) {
          const location = `${relative(REPO_ROOT, file)}:${i + 1}`;
          if (frozenSet.has(literal)) {
            seenInRust.set(literal, [...(seenInRust.get(literal) ?? []), location]);
          } else if (EVENT_CONTEXT.test(line) && !(literal in NEGATIVE_TEST_LITERALS)) {
            // WHAT COUNTS AS "TRYING TO BE AN EVENT TYPE" IS THE WHOLE
            // QUESTION, and the first answer was wrong in a way that hid three
            // real misses.
            //
            // This used to test the LITERAL against eight hard-coded prefixes
            // (Order|Item|KOT|Table|Sent|Payment|Stock|Invoice). At 0.6.1 the
            // edge emitted four unfrozen procurement types and the check
            // reported exactly one: "StockDispatched", by the accident of
            // starting with "Stock". GoodsReceived, GrnGapRecorded and
            // PurchaseReturned passed in silence -- a guard reporting 1 of 4
            // reads as "one typo to fix", not "the contract is three types
            // short", which is the worse failure because it looks like a pass.
            //
            // A prefix list can only ever recognise the vocabulary already
            // frozen, so it is blind to exactly the case it exists for: a NEW
            // milestone's NEW nouns. Procurement brought Goods, Grn, Purchase;
            // M6 will bring its own, and hard-coding those now would rebuild
            // the same hole one milestone later.
            //
            // So classify by CONTEXT, not by vocabulary: flag a PascalCase
            // literal on a line that also mentions an outbox/event API. That
            // is a property of how the literal is USED, which does not go stale
            // when the domain grows a noun.
            unknown.push({ literal, location });
          }
        }
      });
    }
  }

  const problems = [];

  for (const { literal, location } of unknown) {
    problems.push(
      `FORWARD: ${location} uses event type "${literal}", which is not in OUTBOX_EVENT_TYPES.\n` +
        `         Either it is a typo, or the contract needs it frozen (orchestrator-only change).`,
    );
  }

  for (const eventType of frozen) {
    if (seenInRust.has(eventType)) continue;
    if (eventType in NOT_YET_EMITTED) continue;
    problems.push(
      `BACKWARD: "${eventType}" is frozen in the contract but appears in no Rust edge crate.\n` +
        `          Either the edge never adopted it, or it belongs in NOT_YET_EMITTED with a reason.`,
    );
  }

  if (problems.length > 0) {
    console.error("Event-type drift detected between contracts and the Rust edge crates:\n");
    for (const problem of problems) console.error(problem + "\n");
    process.exit(1);
  }

  // Count deferred types EXCLUSIVE of ones Rust already references, so the
  // figures sum to the frozen total. A deferred type can still appear in Rust
  // (KOTCreated shows up in a test asserting it is unrouted), and the earlier
  // phrasing double-counted those.
  const deferredAndUnseen = Object.keys(NOT_YET_EMITTED).filter((t) => !seenInRust.has(t));
  console.log(
    `Event-type drift check passed: ${frozen.length} frozen types — ` +
      `${seenInRust.size} referenced by Rust, ` +
      `${deferredAndUnseen.length} deferred with a reason.`,
  );
}

main();
