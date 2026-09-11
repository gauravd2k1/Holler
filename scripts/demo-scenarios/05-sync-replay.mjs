/**
 * Stage 5 — replay and the known sync gaps, asserted against the live
 * Postgres rather than assumed from the plan documents.
 *
 * Two of these are KNOWN-ABSENT and are recorded as FAIL / NOT BUILT on
 * purpose: gap A7 (only `order` and `table_session` have an edge route, so
 * KOTs, invoices, payments and stock counts never reach the cloud), and the
 * periodic uplink. The task's instruction is to test them and record them,
 * never to skip them.
 */
import { loadState, record, sql } from "./lib.mjs";

const one = (stmt) => sql(stmt).trim();
const rows = (stmt) => one(stmt).split("\n").filter((r) => r !== "");

const run = async () => {
  const state = loadState();
  const orderId = state.captainOrderId ?? null;

  // --------------------------------------------------------------- S-SYNC-01
  const orderCount = one('select count(*) from "order";');
  const itemCount = one("select count(*) from order_item;");
  record({
    id: "S-SYNC-01",
    demoStep: "1a",
    scenario: "Orders replay from the edge to the cloud at all",
    surface: "postgres 5432",
    precondition: "The outbox pump has run at least once since the POS started",
    steps: 'select count(*) from "order" and order_item',
    expected: "Non-zero — `order` is one of the two aggregates with an edge route (gap A7)",
    actual: `${orderCount} order rows, ${itemCount} order_item rows in the cloud`,
    status: Number(orderCount) > 0 ? "PASS" : "FAIL",
    evidence: `SQL count(order)=${orderCount}, count(order_item)=${itemCount}`,
  });

  // --------------------------------------------------------------- S-SYNC-02
  let captainLanded = "n/a";
  if (orderId) {
    captainLanded = one(`select count(*) from "order" where id='${orderId}';`);
  }
  record({
    id: "S-SYNC-02",
    demoStep: "1a / 6",
    scenario: "The order this run created from the captain page reaches Postgres",
    surface: "captain 9320 → backend 8080 → postgres 5432",
    precondition: orderId ? `Captain created order ${orderId}` : "No captain order was created (S-CAP-10 did not run)",
    steps: 'select count(*) from "order" where id = <the captain order id>',
    expected: "1 row",
    actual: orderId ? `count=${captainLanded} for order ${orderId}` : "No captain order id available — S-CAP-10 was blocked",
    status: !orderId ? "BLOCKED" : captainLanded === "1" ? "PASS" : "FAIL",
    evidence: orderId ? `SQL count=${captainLanded}` : "S-CAP-10 blocked",
  });

  // --------------------------------------------------------------- S-SYNC-03
  // The known trap, asserted where CanonicalOrder cannot answer it.
  const attribution = orderId
    ? one(`select coalesce(d.kind,'<no device row>')||'|'||o.device_id from "order" o left join device d on d.id=o.device_id where o.id='${orderId}';`)
    : "";
  const anyAttribution = rows(
    `select k||' x'||n::text from (select coalesce(d.kind,'<no device row>') as k, count(*) as n from "order" o left join device d on d.id=o.device_id group by 1) t;`,
  ).join("; ");
  record({
    id: "S-SYNC-03",
    demoStep: "1a",
    scenario: "A replayed order's device_id resolves to a real device row in the cloud",
    surface: "postgres 5432",
    precondition: "Orders present in the cloud",
    steps: 'Left join "order".device_id to device.id and report the kind',
    expected: "Every order's device_id joins to a device row, so attribution is readable in the back office",
    actual: `Across all orders: ${anyAttribution.replace(/\n/g, "; ")}${orderId ? `. Captain order ${orderId}: ${attribution}` : ""}`,
    status: anyAttribution.includes("<no device row>") ? "FAIL" : "PASS",
    evidence: `SQL: ${anyAttribution.replace(/\n/g, " ; ")}`,
    notes:
      "Every order currently replays with device_id 0191a000-0000-7000-8000-00000000000b, a seed constant with NO row " +
      "in the cloud `device` table (device ids there are 01a0908b-…). There is no FK from \"order\" to device, so the " +
      "orphan replays silently. Waiter-versus-till attribution — the whole point of demo step 1a — is therefore not " +
      "answerable from the cloud copy at all: both devices resolve to the same non-existent id.",
  });

  // --------------------------------------------------------------- S-SYNC-04
  // GAP A7, asserted rather than assumed.
  const a7 = {};
  for (const t of ["kot", "invoice", "payment", "stock_count", "stock_ledger_entry", "cash_shift", "table_session"]) {
    a7[t] = one(`select count(*) from ${t};`);
  }
  const a7Summary = Object.entries(a7).map(([t, n]) => `${t}=${n}`).join(", ");
  record({
    id: "S-SYNC-04",
    demoStep: "4 / 5 / 6",
    scenario: "KNOWN GAP A7 — aggregates other than `order` never reach the cloud",
    surface: "postgres 5432",
    precondition: "The POS has been cutting KOTs all session; S-CAP-14 cut more this run",
    steps: "Count each edge-authoritative aggregate in Postgres",
    expected:
      "If A7 were closed: non-zero kot / invoice / payment / stock_count. As shipped: zero, because edge/sync/src/route.rs maps only `order` and `table_session`",
    actual: a7Summary,
    status: Number(a7.kot) === 0 ? "FAIL" : "PASS",
    evidence: `SQL counts — ${a7Summary}`,
    notes:
      "KNOWN AND FILED. Gap A7 is carried out of M6 Phase A in docs/backlog.md with the trigger 'before the first " +
      "pilot'. The cloud ingest routes exist; the EDGE RESOLVER does not, so the rows sit in the local outbox with " +
      "no route. Recorded as FAIL, not skipped: the back-office demo steps 4 and 5 are built on data that cannot " +
      "arrive. The KOT the KDS displayed in S-KDS-02 is real and LAN-local; it is simply invisible to the cloud.",
  });

  // --------------------------------------------------------------- S-SYNC-05
  // Blocked-row surfacing. `sync_replay_block` is the ranged-stream table;
  // `ledger_replay_gap` is the cloud-side hole record.
  let blockTable = "";
  try {
    blockTable = one("select count(*) from ledger_replay_gap;");
  } catch {
    blockTable = "<table absent>";
  }
  record({
    id: "S-SYNC-05",
    demoStep: "6",
    scenario: "No unresolved cloud-side replay holes are recorded",
    surface: "postgres 5432",
    precondition: "ledger_replay_gap records a ranged-stream hole; resolved_at clears it",
    steps: "select count(*) from ledger_replay_gap",
    expected: "0, or every row carrying a resolved_at",
    actual: `ledger_replay_gap rows = ${blockTable}`,
    status: blockTable === "0" ? "PASS" : blockTable === "<table absent>" ? "FAIL" : "FAIL",
    evidence: `SQL count(ledger_replay_gap) = ${blockTable}`,
    notes: "Vacuously clean while A7 keeps the stock streams from replaying at all — a zero here is not evidence the mechanism works.",
  });

  // --------------------------------------------------------------- S-SYNC-06
  record({
    id: "S-SYNC-06",
    demoStep: "6",
    scenario: "Inspect the edge-local outbox (local_outbox, sync_replay_block) for stranded rows",
    surface: "edge SQLite",
    precondition: "The edge database file is encrypted at rest (ADR-011) and this suite must not copy or decrypt it",
    steps: "Would be: select from local_outbox / sync_replay_block on the edge SQLite file",
    expected: "A count of pending and permanently-blocked rows",
    actual:
      "NOT TESTABLE from outside the POS process. The edge SQLite file is encrypted at rest and there is no read " +
      "surface for the outbox — no Tauri command, no captain route and no LAN frame exposes it",
    status: "NOT TESTABLE",
    evidence: "ADR-011: 'the edge SQLite file is encrypted at rest — never copy it or its backups anywhere unencrypted'",
    notes:
      "This is gap A3's surfacing half seen from the outside: a stranded row is visible only on the POS window, which " +
      "cannot be driven here. The cloud-side consequence is measurable and IS measured in S-SYNC-04.",
  });

  // --------------------------------------------------------------- S-SYNC-07
  const menuCloud = one("select count(*) from menu_item;");
  record({
    id: "S-SYNC-07",
    demoStep: "2",
    scenario: "The cloud menu is seeded, so an admin price edit has something to edit",
    surface: "postgres 5432",
    precondition: "M6 C7 closed the cloud/edge seed drift as no longer needed",
    steps: "select count(*) from menu_item",
    expected: "Non-zero and comparable to the edge's 43",
    actual: `${menuCloud} menu_item rows in the cloud`,
    status: Number(menuCloud) > 0 ? "PASS" : "FAIL",
    evidence: `SQL count(menu_item) = ${menuCloud}`,
  });

  // --------------------------------------------------------------- S-SYNC-08
  const variantsPerItem = rows(
    "select c, count(*) from (select mi.id, count(v.id) as c from menu_item mi left join menu_item_variant v on v.menu_item_id=mi.id group by mi.id) t group by c order by c;",
  );
  record({
    id: "S-SYNC-08",
    demoStep: "1a",
    scenario: "Cloud variant coverage matches the edge, so a replayed order item does not violate its FK",
    surface: "postgres 5432",
    precondition: "M6 C3 found order_item_variant_id_fkey, not the menu_item seed, was the real refusal key",
    steps: "Histogram of variants per menu_item in the cloud",
    expected: "Every item carrying at least the variants the edge offers",
    actual: `variants-per-item histogram (variants|items): ${variantsPerItem.join(", ")}`,
    status: variantsPerItem.some((r) => r.startsWith("0|")) ? "FAIL" : "PASS",
    evidence: `SQL histogram: ${variantsPerItem.join(" ; ")}`,
    notes:
      "docs/backlog.md records every ItemAdded being refused on the variant foreign key. Items with zero cloud " +
      "variants are the rows that cannot accept a replayed line.",
  });
};

run().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
