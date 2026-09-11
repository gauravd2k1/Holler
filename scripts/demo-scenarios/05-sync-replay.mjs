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
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { SHOT_DIR, loadState, record, sql } from "./lib.mjs";

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
  // The claim that matters is about orders created NOW, not about every row
  // the cloud has ever held. A legacy order written by a device that was never
  // enrolled cannot be repaired by a fix to the config pull, and averaging it
  // into this row would hide the thing the row exists to report.
  const captainResolves = attribution.startsWith("WAITER|");
  const orphanIds = rows(
    `select o.id||' device_id='||o.device_id||' at '||o.created_at from "order" o left join device d on d.id=o.device_id where d.id is null order by o.created_at;`,
  );
  record({
    id: "S-SYNC-03",
    demoStep: "1a",
    scenario: "A newly created order replays with a device_id that resolves to a real device row in the cloud",
    surface: "postgres 5432",
    precondition: `Captain order ${orderId ?? "<none>"} created this run and replayed`,
    steps:
      'Left join "order".device_id to device.id for the order THIS run created, and report the kind; then count every order in the cloud that still fails to resolve.',
    expected: "The order created this run joins to a device row of kind WAITER",
    actual: orderId
      ? `Captain order ${orderId}: ${attribution}. Across all orders: ${anyAttribution.replace(/\n/g, "; ")}.`
      : "No captain order id available",
    status: !orderId ? "BLOCKED" : captainResolves ? "PASS" : "FAIL",
    evidence: `SQL left join "order" -> device; ${anyAttribution.replace(/\n/g, " ; ")}`,
    notes:
      `A SEPARATE, OPPOSITE GAP REMAINS AND IS NOT FIXED BY 758a70b — see S-SYNC-13. ${orphanIds.length} order(s) ` +
      "still fail to resolve, every one of them carrying device_id 0191a000-0000-7000-8000-00000000000b, which is " +
      "edge/database/src/bin/devseed.rs:41's DEVICE_ID — the TILL. That row is written locally by devseed and was " +
      "never enrolled in the cloud, so Postgres has no `device` row for it. Note the direction: T30 fixed " +
      "cloud→edge (a cloud-enrolled device missing its edge row); this is edge→cloud (an edge-seeded device missing " +
      'its cloud row). There is no FK from "order" to device in Postgres, so the orphan replays silently.',
    rerun:
      "WAS FAIL — no order in the cloud resolved to a device row, so waiter-versus-till attribution was unanswerable. " +
      "NOW PASS for the order this run created: it resolves to a WAITER device. Re-scoped to the order under test " +
      "because the rows that still fail are legacy till-authored ones a config-pull fix cannot reach; they are " +
      "carried as S-SYNC-13 rather than left averaged into this verdict.",
  });

  // --------------------------------------------------------------- S-SYNC-13
  record({
    id: "S-SYNC-13",
    demoStep: "1a / 6",
    scenario: "The till's own devseed device has no cloud `device` row, so till-authored orders replay unattributable",
    surface: "postgres 5432",
    precondition: "Orders authored by the till before this run, replayed into the cloud",
    steps: 'Left join "order".device_id to device.id and list every order that fails to resolve',
    expected: "Zero orders with an unresolvable device_id",
    actual:
      orphanIds.length === 0
        ? "Every order in the cloud resolves to a device row"
        : `${orphanIds.length} order(s) do not resolve: ${orphanIds.join(" | ")}`,
    status: orphanIds.length === 0 ? "PASS" : "FAIL",
    evidence: `SQL: select o.id from "order" o left join device d on d.id=o.device_id where d.id is null -> ${orphanIds.length} row(s)`,
    notes:
      "NOT THE T30 DEFECT AND NOT FIXED BY IT. devseed mints the till's `device` row directly in edge SQLite " +
      "(edge/database/src/bin/devseed.rs:41) and nothing ever enrols it with the backend, so the cloud has no row to " +
      "join to. Back-office attribution for till-authored orders is therefore blank — which is the same class of " +
      "defect T30 fixed, pointed the other way down the wire. Recorded, not fixed: enrolling the till is a change to " +
      "the seed path, outside this task's read-only scope.",
    rerun:
      "NEW ROW this run. Split out of S-SYNC-03, which previously reported both gaps as one verdict and so could not " +
      "show that the captain path had been fixed while the till path had not.",
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

  // --------------------------------------------------------------- S-SYNC-10
  // A replayed order that arrives with a total and no lines. The order row
  // and its item rows travel as SEPARATE outbox entries, so the parent can
  // land while every child is refused on order_item_variant_id_fkey — and
  // nothing about the parent says so.
  const empties = rows(
    `select o.id||' status='||o.status||' total_paise='||o.total_paise::text||' lines='||(select count(*) from order_item oi where oi.order_id=o.id)::text from "order" o order by o.created_at;`,
  );
  const emptyCount = Number(
    one(`select count(*) from "order" o where o.total_paise > 0 and not exists (select 1 from order_item oi where oi.order_id=o.id);`),
  );
  record({
    id: "S-SYNC-10",
    demoStep: "6",
    scenario: "A replayed order carries its line items, not just its total",
    surface: "postgres 5432",
    precondition: "Orders have replayed (S-SYNC-01) and the cloud holds menu_item_variant rows",
    steps: 'For every cloud order, count its order_item rows alongside its total_paise',
    expected: "No order with money on it and nothing in it",
    actual: `${emptyCount} order(s) carry a non-zero total with ZERO lines. Per order: ${empties.join(" | ")}`,
    status: emptyCount === 0 ? "PASS" : "FAIL",
    evidence: `SQL: ${empties.join(" ; ")}`,
    notes:
      "THE BACK OFFICE SHOWS AN ORDER FOR ₹300 WITH NOTHING IN IT. docs/backlog.md records every ItemAdded being " +
      "refused on the variant foreign key, and this is that refusal seen from the cloud side: the order row and its " +
      "item rows are separate outbox entries, so the parent lands and the children are rejected, leaving a total " +
      "with no lines to explain it. S-SYNC-08 names the cause — 12 cloud menu items carry no variant at all. " +
      "Demo step 6 reads from exactly this data.",
  });

  // --------------------------------------------------------------- S-SYNC-12
  // RE-MEASURED, not asserted. This row previously carried a previous run's
  // finding as literal text against pids that no longer exist, so it would
  // have reported FAIL forever regardless of what the current process does.
  // It now reads the committed artefact stage 07 writes and classifies on it.
  const watchArtefact = join(SHOT_DIR, "pos-uplink-watch.txt");
  let contacts = [];
  let watchWindow = "";
  try {
    const text = readFileSync(watchArtefact, "utf8").replace(/^﻿/, "");
    const pts = [];
    for (const raw of text.split(/\r?\n/)) {
      const m = raw.trim().match(/^tick\s+\d+\s*:\s*POS\|([^|]*)\|(.*)/);
      if (m) pts.push({ lastUsed: m[1].trim(), now: m[2].trim() });
    }
    contacts = [...new Set(pts.map((p) => p.lastUsed))];
    if (pts.length) watchWindow = `${pts[0].now} to ${pts[pts.length - 1].now} (${pts.length} samples)`;
  } catch {
    contacts = [];
  }
  // Postgres prints "2026-09-11 14:07:47.207865+00" — not ISO-8601. Date.parse
  // returns NaN rather than throwing, so an un-normalised stamp would turn
  // every gap into NaN and still render as a result.
  const pgTs = (s) => new Date(s.replace(" ", "T").replace(/([+-]\d{2})$/, "$1:00"));
  const contactGaps = [];
  for (let i = 1; i < contacts.length; i++) {
    contactGaps.push(Math.round((pgTs(contacts[i]) - pgTs(contacts[i - 1])) / 1000));
  }
  const worstGap = contactGaps.length ? Math.max(...contactGaps) : null;
  const pumpHealthy = worstGap !== null && worstGap <= 120;
  record({
    id: "S-SYNC-12",
    demoStep: "1a / 6",
    scenario: "A live POS keeps pumping for as long as it keeps serving",
    surface: "POS process -> backend 8080 -> postgres 5432",
    precondition:
      "holler-pos pid 55508 up for the whole sampling window (S-ENV-02), serving 9310 and 9320 throughout. The " +
      "documented interval is 60s (DEFAULT_PERIODIC_DRAIN_INTERVAL, apps/pos/src-tauri/src/state.rs:93).",
    steps:
      "Parse the committed uplink artefact — device_credential.last_used_at for the POS device sampled every 30s " +
      "from outside the POS, on Postgres's clock — and measure the interval between distinct cloud contacts.",
    expected: "Contact every ~60s for as long as the process is alive, with no gap beyond 2x the documented interval",
    actual:
      contacts.length === 0
        ? "No uplink artefact could be parsed, so the pump was NOT re-measured this run."
        : `${contacts.length} distinct contact(s) over ${watchWindow}. Longest gap between contacts: ` +
          `${worstGap === null ? "n/a — only one contact" : `${Math.floor(worstGap / 60)}m${worstGap % 60}s`} ` +
          `against a documented 60s.`,
    status: contacts.length === 0 ? "BLOCKED" : pumpHealthy ? "PASS" : "FAIL",
    evidence: `docs/demo-screens/scenarios/pos-uplink-watch.txt — ${watchWindow || "unparseable"}`,
    rerun:
      "WAS FAIL, carried as literal text from an earlier run against pids 74104 and 80612, both long gone — a row " +
      "that could never change verdict no matter what the live process did. NOW RE-MEASURED against pid 55508 from " +
      "the committed artefact. THE HISTORICAL FINDING IS NOT WITHDRAWN: a healthy window does not prove the stall " +
      "cannot recur, only that this process did not stall during this window.",
    notes:
      "HISTORICAL FINDING, FROM AN EARLIER RUN, RETAINED BECAUSE IT WAS REAL AND IS NOT DISPROVED BY A HEALTHY " +
      "WINDOW: on pid 74104, THE PUMP IS NOT SLOW — IT STOPPED, AND THE PROCESS DID NOT. This is worse than a long interval, because " +
      "every outward sign of health stayed green: the till served the LAN, the captain listener answered, the KDS " +
      "socket accepted clients. Nothing surfaced that the outlet had gone silent toward the cloud. " +
      "MEASURED CONSEQUENCE: the WAITER device enrolled at 14:14:23 could not pair until 14:31:37 — seventeen " +
      "minutes — because the config pull that delivers a credential to device_credential_cache rides this same " +
      "loop, and what finally delivered it was the STARTUP pull of the replacement process, not a tick. " +
      "NOT DIAGNOSED HERE: whether the pump thread died, wedged on the database lock, or the process was already " +
      "failing. This run observed the symptom on one instance and could not reproduce it on the second, so it is " +
      "reported as an observation with its artefacts, not as a root cause. The first measurement's own conclusion " +
      "('the pump interval is 12m27s') was WRONG and is superseded by this row: it spanned a process boundary, " +
      "where a startup drain and a shutdown drain cannot be told apart from periodic ticks.",
  });

  // --------------------------------------------------------------- S-SYNC-11
  // Config that is supposed to flow cloud -> edge, counted at the source.
  const cfg = {};
  for (const t of ["restaurant_table", "station", "printer", "tax_profile", "menu_category", "menu_item", "supplier"]) {
    cfg[t] = one(`select count(*) from ${t};`);
  }
  const cfgSummary = Object.entries(cfg).map(([t, n]) => `${t}=${n}`).join(", ");
  const emptyConfig = Object.entries(cfg).filter(([, n]) => n === "0").map(([t]) => t);
  record({
    id: "S-SYNC-11",
    demoStep: "1a / 2",
    scenario: "The cloud actually holds the config it is the authority for",
    surface: "postgres 5432",
    precondition:
      "restaurant_table, station, printer, tax_profile and the menu are CONFIG, cloud-authoritative, syncing down " +
      "(ADR-011, ADR-014). The edge caches them; the cloud owns them.",
    steps: "Count each cloud-authoritative config table in Postgres",
    expected: "Non-zero for every table the outlet is currently using",
    actual: `${cfgSummary}. Empty cloud-side: ${emptyConfig.length ? emptyConfig.join(", ") : "none"}`,
    status: emptyConfig.length === 0 ? "PASS" : "FAIL",
    evidence: `SQL counts — ${cfgSummary}`,
    notes:
      "THE TILL HAS TABLES AND STATIONS; THE CLOUD HAS NEITHER. They came from the edge dev seed, and " +
      "config apply UPSERTS WITHOUT PRUNING (contracts 0.7.0), so a pull can never add them and can never remove " +
      "them either — the outlet runs on config the cloud cannot reproduce, manage or show. For a demo whose pitch " +
      "is 'manage it from the back office', every table on the waiter's phone and every station a KOT routes to is " +
      "invisible to the back office. Note the asymmetry with menu_item, which IS seeded cloud-side and therefore IS " +
      "editable in the admin console.",
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
