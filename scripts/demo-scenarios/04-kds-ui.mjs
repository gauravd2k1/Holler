/**
 * Stage 4 — the KDS PWA on 5174 against the REAL LAN socket on 9310.
 *
 * The KDS Vite server runs `--mode dev`, so apps/kds/.env.dev supplies the
 * LAN URL and the device identity and the page connects on mount. Nothing is
 * stubbed: the socket, the snapshot and the status bump all go to the POS
 * process. A bump is asserted on the CARD TEXT, never on the click — the KDS
 * is not authoritative and only repaints once the edge echoes kot_upserted.
 */
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { KDS_UI, REPO_ROOT, record, shotPath, shotRel } from "./lib.mjs";

const require = createRequire(join(REPO_ROOT, "apps", "kds", "package.json"));
const { chromium } = require("@playwright/test");

/**
 * Is the LAN socket's HOST PROCESS up at all?
 *
 * The KDS dev server and the socket live in different processes, so 5174 can
 * serve a perfectly healthy page while 9310 has nothing behind it. Without
 * this the stage would record "the KDS does not connect" as a product FAIL
 * when the truthful statement is "there was nothing to connect to" — the
 * distinction S-ENV-02 exists to preserve.
 */
async function lanHostUp() {
  try {
    await fetch("http://localhost:9320/", { signal: AbortSignal.timeout(3000) });
    return true;
  } catch {
    return false;
  }
}

/**
 * How many tickets does the EDGE actually put in the snapshot?
 *
 * An empty board has two completely different causes — the KDS failed to
 * render what it was sent, or it was sent nothing — and they are
 * indistinguishable from the page. Subscribing as a second client answers it
 * directly, so an empty board is never reported as a KDS defect when the
 * truthful statement is "there is no active ticket at the outlet".
 *
 * Returns null if the socket could not be used at all.
 */
async function snapshotTicketCount() {
  const raw = readFileSync(join(REPO_ROOT, "apps", "kds", ".env.dev"), "utf8");
  const env = {};
  for (const l of raw.split("\n")) {
    const m = l.match(/^(VITE_[A-Z_]+)=(.*)/);
    if (m) env[m[1]] = m[2].trim();
  }
  if (!env.VITE_KDS_DEVICE_TOKEN) return null;
  return new Promise((resolve) => {
    let ws;
    const done = (v) => { try { ws?.close(); } catch { /* already gone */ } resolve(v); };
    const t = setTimeout(() => done(null), 8000);
    try {
      ws = new WebSocket(`ws://localhost:9310/kds?outlet_id=${env.VITE_KDS_OUTLET_ID}&device_id=${env.VITE_KDS_DEVICE_ID}`);
    } catch {
      clearTimeout(t);
      return done(null);
    }
    ws.onopen = () => ws.send(JSON.stringify({ type: "auth", device_token: env.VITE_KDS_DEVICE_TOKEN }));
    ws.onerror = () => { clearTimeout(t); done(null); };
    ws.onmessage = (e) => {
      let f;
      try { f = JSON.parse(e.data); } catch { return; }
      if (f.type === "snapshot") { clearTimeout(t); done((f.kots ?? f.payload ?? []).length); }
    };
  });
}

const run = async () => {
  const lanUp = await lanHostUp();
  const downNote =
    "The POS process that hosts the 9310 LAN server is not running, so this is BLOCKED by an absent process, not " +
    "by a KDS defect. See S-ENV-02.";
  const browser = await chromium.launch();
  const page = await browser.newContext({ viewport: { width: 1280, height: 900 } }).then((c) => c.newPage());
  const consoleErrors = [];
  page.on("console", (m) => { if (m.type() === "error") consoleErrors.push(m.text()); });
  page.on("pageerror", (e) => consoleErrors.push(`pageerror: ${e.message}`));

  // -------------------------------------------------------------- S-KDS-03
  const resp = await page.goto(KDS_UI, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(2500);
  const configError = await page.locator("main.kds-config-error").count();
  // Located by text, not by role: the KDS renders its title in a header
  // element that carries no heading role, so getByRole("heading") finds
  // nothing even when the app has mounted correctly.
  const heading = await page.getByText("Kitchen Display", { exact: true }).first().isVisible().catch(() => false);
  await page.screenshot({ path: shotPath("kds-01-loaded"), fullPage: true });
  record({
    id: "S-KDS-03",
    demoStep: "1a / 3",
    scenario: "The KDS loads past its config gate",
    surface: "KDS 5174 (Chromium)",
    precondition: "Vite dev server on 5174 started with --mode dev, so .env.dev supplies the LAN config",
    steps: "Navigate to http://localhost:5174/, wait for mount",
    expected: "'Kitchen Display' heading, and NO 'KDS is not configured' screen",
    actual: `HTTP ${resp?.status()}; heading visible=${heading}; config-error screens=${configError}; console errors=${consoleErrors.length}${consoleErrors.length ? ` (${consoleErrors[0].slice(0, 140)})` : ""}`,
    status: heading && configError === 0 ? "PASS" : "FAIL",
    evidence: shotRel("kds-01-loaded"),
    notes: "The config-error screen is the falsifier: a missing env var replaces the whole app with it.",
  });

  // -------------------------------------------------------------- S-KDS-04
  const statusEl = page.getByTestId("connection-status");
  let connState = "<absent>";
  for (let i = 0; i < 20; i++) {
    connState = (await statusEl.getAttribute("data-status").catch(() => null)) ?? "<absent>";
    if (connState === "connected") break;
    await page.waitForTimeout(500);
  }
  const bannerText = await page.locator(".connection-banner").textContent().catch(() => null);
  await page.screenshot({ path: shotPath("kds-02-connected"), fullPage: true });
  record({
    id: "S-KDS-04",
    demoStep: "1a / 3",
    scenario: "The KDS connects to the real LAN socket hosted inside the POS process",
    surface: "KDS 5174 → ws 9310",
    precondition: "POS process bound on 9310; KDS device credential cached at the edge",
    steps: 'Poll [data-testid="connection-status"] for data-status="connected" for up to 10s',
    expected: 'data-status="connected" and no connection banner',
    actual: `data-status="${connState}"; banner=${JSON.stringify(bannerText)}${lanUp ? "" : ". " + downNote}`,
    status: connState === "connected" ? "PASS" : lanUp ? "FAIL" : "BLOCKED",
    evidence: shotRel("kds-02-connected"),
    notes: "The banner renders ONLY when not connected, so its absence is a second, independent signal.",
  });

  // -------------------------------------------------------------- S-KDS-05
  const cards = page.locator("article.ticket-card");
  let cardCount = 0;
  for (let i = 0; i < 16; i++) {
    cardCount = await cards.count();
    if (cardCount > 0) break;
    await page.waitForTimeout(500);
  }
  const emptyText = await page.locator("p.kds-board__empty").textContent().catch(() => null);
  const edgeTickets = await snapshotTicketCount();
  await page.screenshot({ path: shotPath("kds-03-tickets"), fullPage: true });
  record({
    id: "S-KDS-05",
    demoStep: "1a / 3",
    scenario: "The KDS renders live tickets received in the snapshot",
    surface: "KDS 5174 → ws 9310",
    precondition: "KOTs exist at the edge (the POS and S-CAP-14 have cut several this session)",
    steps: "Wait up to 8s for article.ticket-card elements",
    expected: "At least one ticket card carrying a data-kot-id",
    actual:
      cardCount > 0
        ? `${cardCount} ticket cards rendered, and the edge's own snapshot carried ${edgeTickets ?? "?"}`
        : `No ticket cards; board says ${JSON.stringify(emptyText)}. The edge's own snapshot carried ` +
          `${edgeTickets === null ? "could not be read" : edgeTickets} ticket(s)${lanUp ? "" : ". " + downNote}`,
    // Three outcomes, not two. A board showing what the edge sent is a PASS; a
    // board empty while the edge sent tickets is a real KDS FAIL; a board
    // empty because the edge sent nothing is neither.
    status: cardCount > 0 ? "PASS" : !lanUp ? "BLOCKED" : edgeTickets === 0 ? "BLOCKED" : "FAIL",
    evidence: `${shotRel("kds-03-tickets")}; edge snapshot ticket count = ${edgeTickets === null ? "unreadable" : edgeTickets}`,
    notes:
      edgeTickets === 0
        ? "NOT A KDS DEFECT. Subscribing to 9310 as a second client shows the EDGE puts zero tickets in the " +
          "snapshot, so the board is showing exactly what it was sent. There is no active ticket at this outlet, " +
          "and the two ways to create one are both unavailable here: the captain send fails on a foreign key " +
          "(S-CAP-10) and the till UI is a native WebView2 that Playwright cannot drive. Read this row as 'the " +
          "rendering path is unexercised', never as 'the KDS works' — an empty board is what a broken renderer " +
          "looks like too, and this run cannot tell them apart."
        : "",
  });

  if (cardCount === 0) {
    for (const [id, scenario] of [
      ["S-KDS-06", "Bumping a ticket advances its status on the board"],
      ["S-KDS-07", "A bumped status survives a page reload, so it was persisted at the edge"],
    ]) {
      record({
        id,
        demoStep: "3",
        scenario,
        surface: "KDS 5174 → ws 9310",
        precondition: "At least one ticket on the board",
        steps: "n/a",
        expected: "n/a",
        actual: "BLOCKED by S-KDS-05: no ticket card is on the board to bump",
        status: "BLOCKED",
        evidence: shotRel("kds-03-tickets"),
      });
    }
    await browser.close();
    return;
  }

  // -------------------------------------------------------------- S-KDS-06
  // Pick a card that still has a forward step (SERVED/CANCELLED render none).
  const bumpable = page.locator("article.ticket-card").filter({ has: page.locator("button.ticket-card__advance") }).first();
  const hasBumpable = (await bumpable.count()) > 0;
  let kotId = null, before = null, after = null, label = null;
  if (hasBumpable) {
    kotId = await bumpable.getAttribute("data-kot-id");
    before = (await bumpable.locator(".ticket-card__status").textContent())?.trim() ?? null;
    label = (await bumpable.locator("button.ticket-card__advance").textContent())?.trim() ?? null;
    await bumpable.locator("button.ticket-card__advance").click();
    // Assert on the CARD, not the click: the edge must echo kot_upserted first.
    for (let i = 0; i < 24; i++) {
      await page.waitForTimeout(500);
      const card = page.locator(`article.ticket-card[data-kot-id="${kotId}"]`);
      after = (await card.locator(".ticket-card__status").textContent().catch(() => null))?.trim() ?? null;
      if ((await card.count()) === 0) { after = "<card left the board>"; break; }
      if (after && after !== before) break;
    }
  }
  await page.screenshot({ path: shotPath("kds-04-bumped"), fullPage: true });
  record({
    id: "S-KDS-06",
    demoStep: "3",
    scenario: "Bumping a ticket advances its status, echoed back by the edge",
    surface: "KDS 5174 → ws 9310 → POS",
    precondition: hasBumpable ? `Ticket ${kotId} showing status "${before}" with a "${label}" button` : "A ticket with a forward transition",
    steps: 'Click button.ticket-card__advance, then poll the SAME card\'s .ticket-card__status until it changes',
    expected: "The card's status text changes — proof the edge accepted and echoed set_kot_status",
    actual: hasBumpable
      ? `"${before}" → "${after}" after clicking "${label}"`
      : "Every ticket on the board is already SERVED or CANCELLED, so no advance button renders",
    status: hasBumpable ? (after && after !== before ? "PASS" : "FAIL") : "NOT TESTABLE",
    evidence: shotRel("kds-04-bumped"),
    notes:
      "Asserted on the card, never on the click: the KDS is not authoritative and repaints only after the edge " +
      "echoes kot_upserted, so a click that changed nothing would still look like a click.",
  });

  // -------------------------------------------------------------- S-KDS-07
  let persisted = null;
  if (hasBumpable && after && after !== before) {
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.waitForTimeout(4000);
    const card = page.locator(`article.ticket-card[data-kot-id="${kotId}"]`);
    persisted = (await card.count()) === 0 ? "<card no longer on the board>" : ((await card.locator(".ticket-card__status").textContent().catch(() => null))?.trim() ?? null);
  }
  await page.screenshot({ path: shotPath("kds-05-after-reload"), fullPage: true });
  record({
    id: "S-KDS-07",
    demoStep: "3",
    scenario: "The bumped status survived a reload, so it was persisted at the edge and not held in page state",
    surface: "KDS 5174 → ws 9310",
    precondition: `Ticket ${kotId} bumped from "${before}" to "${after}" in S-KDS-06`,
    steps: "Reload the page, let the fresh snapshot arrive, read the same card's status",
    expected: `The same card still reads "${after}" — the snapshot is rebuilt from the edge, not from the browser`,
    actual: persisted === null ? "Not run — S-KDS-06 did not produce a status change to verify" : `After reload the card reads "${persisted}"`,
    status: persisted === null ? "BLOCKED" : persisted === after ? "PASS" : "FAIL",
    evidence: shotRel("kds-05-after-reload"),
    notes: "A reload discards all page state, so a status still showing after it came from the edge's snapshot.",
  });

  await browser.close();
};

run().catch((err) => {
  console.error(err);
  record({
    id: "S-KDS-ERR",
    demoStep: "3",
    scenario: "KDS UI stage completed without an unhandled failure",
    surface: "KDS 5174",
    precondition: "n/a",
    steps: "Run scripts/demo-scenarios/04-kds-ui.mjs",
    expected: "The stage runs to completion",
    actual: `Unhandled error: ${String(err).slice(0, 300)}`,
    status: "FAIL",
    evidence: "stderr of the scenario run",
  });
  process.exitCode = 1;
});
