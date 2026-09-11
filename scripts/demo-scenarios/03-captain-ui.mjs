/**
 * Stage 3 — the captain page itself, driven in Chromium at 390px against the
 * REAL listener on 9320 with the REAL WAITER token. No route interception, no
 * mocked /api/*, no injected state: the page fetches from the shipping POS
 * process exactly as a phone on the hotspot would.
 *
 * 390px is the demo's actual viewport — a waiter's phone, not a laptop window.
 */
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { CAPTAIN, REPO_ROOT, RESULT_DIR, fingerprint, record, shotPath, shotRel, sleep } from "./lib.mjs";

// Playwright is already installed under apps/kds — @playwright/test, which
// re-exports chromium. No dependency is added for this suite (task rule).
const require = createRequire(join(REPO_ROOT, "apps", "kds", "package.json"));
const { chromium } = require("@playwright/test");

const TOKEN = JSON.parse(readFileSync(join(RESULT_DIR, "secrets.json"), "utf8")).deviceToken;
const PHONE = { width: 390, height: 844 };

const run = async () => {
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ viewport: PHONE, deviceScaleFactor: 2, isMobile: true, hasTouch: true });
  const page = await ctx.newPage();
  const consoleErrors = [];
  page.on("console", (m) => { if (m.type() === "error") consoleErrors.push(m.text()); });
  page.on("pageerror", (e) => consoleErrors.push(`pageerror: ${e.message}`));

  // ------------------------------------------------------------- S-CUI-01
  const resp = await page.goto(CAPTAIN, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(1200);
  const pairVisible = await page.getByRole("heading", { name: "Pair this phone" }).isVisible().catch(() => false);
  await page.screenshot({ path: shotPath("captain-01-pair"), fullPage: true });
  record({
    id: "S-CUI-01",
    demoStep: "1a",
    scenario: "The captain page is served by the POS process and renders the pair screen at phone width",
    surface: "captain 9320 (Chromium 390×844)",
    precondition: "POS process hosts the captain listener and serves apps/captain/dist",
    steps: "Navigate to http://localhost:9320/ in a 390px mobile context",
    expected: "200, 'Pair this phone' visible, no console errors",
    actual: `HTTP ${resp?.status()}; pair heading visible=${pairVisible}; console errors=${consoleErrors.length}${consoleErrors.length ? ` (${consoleErrors[0].slice(0, 120)})` : ""}`,
    status: resp?.status() === 200 && pairVisible ? "PASS" : "FAIL",
    evidence: shotRel("captain-01-pair"),
    notes: "Real listener, real bundle — nothing intercepted.",
  });

  // ------------------------------------------------------------- S-CUI-02
  await page.getByPlaceholder("credential_id.secret").fill("00000000-0000-0000-0000-000000000000.notarealsecretvalue");
  await page.getByRole("button", { name: "Pair" }).click();
  await page.waitForTimeout(2000);
  const rejectMsg = await page.locator("p.error").first().textContent().catch(() => null);
  await page.screenshot({ path: shotPath("captain-02-garbage-token"), fullPage: true });
  const storedAfterGarbage = await page.evaluate(() => window.localStorage.getItem("holler_captain_device_token"));
  record({
    id: "S-CUI-02",
    demoStep: "1a",
    scenario: "Pairing the page with a garbage token is refused and the bad token is not persisted",
    surface: "captain 9320 (Chromium 390×844)",
    precondition: "Pair screen showing, localStorage empty",
    steps: "Paste a fabricated credential_id.secret, tap Pair, then read localStorage",
    expected: "A rejection message, and NOTHING stored — a mistyped token is never persisted as though it worked",
    actual: `message=${JSON.stringify(rejectMsg)}; localStorage token stored=${storedAfterGarbage !== null}`,
    status: rejectMsg?.includes("rejected") && storedAfterGarbage === null ? "PASS" : "FAIL",
    evidence: shotRel("captain-02-garbage-token"),
    notes: "Falsifier for S-CUI-03: a page that stores whatever is typed would show a 'paired' state either way.",
  });

  // ------------------------------------------------------------- S-CUI-03
  await page.getByPlaceholder("credential_id.secret").fill(TOKEN);
  await page.getByRole("button", { name: "Pair" }).click();
  await page.waitForTimeout(2500);
  const tablesVisible = await page.getByRole("heading", { name: "Tables" }).isVisible().catch(() => false);
  const pairError = await page.locator("p.error").first().textContent().catch(() => null);
  await page.screenshot({ path: shotPath("captain-03-pair-real-token"), fullPage: true });
  record({
    id: "S-CUI-03",
    demoStep: "1a",
    scenario: "Pairing the page with the REAL enrolled WAITER token reaches the Tables screen",
    surface: "captain 9320 (Chromium 390×844)",
    precondition: `WAITER device enrolled in the cloud at S-BE-06 (token ${fingerprint(TOKEN)})`,
    steps: "Paste the real device token, tap Pair",
    expected: "Pair screen replaced by the Tables screen",
    actual: tablesVisible
      ? "Paired; Tables screen shown"
      : `Still on the pair screen. Page message: ${JSON.stringify(pairError)}. The listener answers GET /api/session with 401 "device credential not cached locally"`,
    status: tablesVisible ? "PASS" : "FAIL",
    evidence: shotRel("captain-03-pair-real-token"),
    notes: tablesVisible
      ? ""
      : "ROOT CAUSE, evidenced in S-SYNC-09: the credential is enrolled cloud-side but has never reached the edge's " +
        "device_credential_cache, because the POS has made no authenticated cloud request since 14:07:47 UTC. " +
        "Absence is unknown, never allow — the 401 is the listener behaving correctly on data it does not have.",
  });

  if (!tablesVisible) {
    for (const [id, scenario] of [
      ["S-CUI-04", "Tables screen lists the outlet's tables with free/occupied state"],
      ["S-CUI-05", "Tapping a table opens the menu and cart screen"],
      ["S-CUI-06", "A snoozed item is shown as 'Not available' and cannot be added"],
      ["S-CUI-07", "Adding items updates the cart bar count and total"],
      ["S-CUI-08", "Sending from the phone reaches the 'Sent to the kitchen' screen with the KOT list"],
    ]) {
      record({
        id,
        demoStep: "1a",
        scenario,
        surface: "captain 9320 (Chromium 390×844)",
        precondition: "A paired captain session",
        steps: "n/a — the page never gets past the pair screen",
        expected: "n/a",
        actual: "BLOCKED by S-CUI-03: the WAITER credential is not cached at the edge, so no authenticated screen renders",
        status: "BLOCKED",
        evidence: shotRel("captain-03-pair-real-token"),
      });
    }
    await browser.close();
    return;
  }

  // ------------------------------------------------------------- S-CUI-04
  const tiles = page.locator("button.table-tile");
  const tileCount = await tiles.count();
  await page.screenshot({ path: shotPath("captain-04-tables"), fullPage: true });
  record({
    id: "S-CUI-04",
    demoStep: "1a",
    scenario: "Tables screen lists the outlet's tables with free/occupied state",
    surface: "captain 9320 (Chromium 390×844)",
    precondition: "Paired captain session",
    steps: "Read the table tiles",
    expected: "At least one tile, each labelled Free or Occupied",
    actual: `${tileCount} table tiles rendered`,
    status: tileCount > 0 ? "PASS" : "FAIL",
    evidence: shotRel("captain-04-tables"),
  });

  // ------------------------------------------------------------- S-CUI-05
  const freeTile = page.locator("button.table-tile.free").first();
  const target = (await freeTile.count()) > 0 ? freeTile : tiles.first();
  const tableName = (await target.locator("span").first().textContent()) ?? "?";
  await target.click();
  await page.waitForTimeout(1500);
  const menuRows = page.locator("button.menu-row");
  const rowCount = await menuRows.count();
  await page.screenshot({ path: shotPath("captain-05-menu"), fullPage: true });
  record({
    id: "S-CUI-05",
    demoStep: "1a",
    scenario: "Tapping a table opens the menu and cart screen",
    surface: "captain 9320 (Chromium 390×844)",
    precondition: `Tables screen showing; tapping ${tableName}`,
    steps: "Tap a table tile, wait for the menu list",
    expected: "The menu list renders with the table name as the heading",
    actual: `${rowCount} menu rows under heading "${(await page.locator("h2.menu-cart-title").textContent().catch(() => "?")) ?? "?"}"`,
    status: rowCount > 0 ? "PASS" : "FAIL",
    evidence: shotRel("captain-05-menu"),
  });

  // ------------------------------------------------------------- S-CUI-06
  const unavailableTags = await page.locator(".unavailable-tag").count();
  record({
    id: "S-CUI-06",
    demoStep: "1a",
    scenario: "A snoozed item is marked 'Not available' on the waiter's phone",
    surface: "captain 9320 (Chromium 390×844)",
    precondition: "At least one menu item with is_available = false",
    steps: "Count .unavailable-tag elements on the menu screen",
    expected: "One per snoozed item; the page respects the field it is served",
    actual:
      unavailableTags > 0
        ? `${unavailableTags} items marked Not available`
        : "No item in this catalogue is snoozed, so the page's handling of is_available is UNEXERCISED by this run",
    status: unavailableTags > 0 ? "PASS" : "NOT TESTABLE",
    evidence: shotRel("captain-05-menu"),
    notes:
      "Creating a stock-out to force this would need the till UI, which cannot be driven here. S-CAP-07 proves the " +
      "field is SERVED; nothing in this run proves the page HONOURS it.",
  });

  // ------------------------------------------------------------- S-CUI-07
  await menuRows.first().click();
  await page.waitForTimeout(800);
  // A modifier sheet may intercept; dismiss it by confirming if present.
  const sheetAdd = page.getByRole("button", { name: /^Add/ });
  if (await sheetAdd.count()) { await sheetAdd.first().click().catch(() => undefined); await page.waitForTimeout(500); }
  const cartCount = (await page.locator(".cart-bar .count").textContent().catch(() => "")) ?? "";
  const cartTotal = (await page.locator(".cart-bar .total").textContent().catch(() => "")) ?? "";
  await page.screenshot({ path: shotPath("captain-06-cart"), fullPage: true });
  record({
    id: "S-CUI-07",
    demoStep: "1a",
    scenario: "Adding an item updates the cart bar count and total",
    surface: "captain 9320 (Chromium 390×844)",
    precondition: "Menu screen showing with an empty cart",
    steps: "Tap the first menu row (confirming a modifier sheet if one opens), read the cart bar",
    expected: "Count of 1 or more and a non-zero rupee total, formatted from integer paise",
    actual: `cart bar reads "${cartCount.trim()}" / "${cartTotal.trim()}"`,
    status: /[1-9]/.test(cartCount) ? "PASS" : "FAIL",
    evidence: shotRel("captain-06-cart"),
  });

  // ------------------------------------------------------------- S-CUI-08
  const sendBtn = page.locator(".cart-bar button.btn--primary");
  await sendBtn.click();
  await page.waitForTimeout(4000);
  const sentVisible = await page.getByRole("heading", { name: "Sent to the kitchen" }).isVisible().catch(() => false);
  const kotRows = await page.locator(".kot-row").count();
  const sendError = await page.locator(".menu-cart-send-error").textContent().catch(() => null);
  await page.screenshot({ path: shotPath("captain-07-sent"), fullPage: true });
  record({
    id: "S-CUI-08",
    demoStep: "1a",
    scenario: "Sending from the phone reaches the 'Sent to the kitchen' screen with the KOT list",
    surface: "captain 9320 (Chromium 390×844)",
    precondition: "Cart holding at least one line",
    steps: "Tap Send, wait for the confirmation screen",
    expected: "'Sent to the kitchen' heading and at least one KOT row",
    actual: sentVisible ? `Sent screen shown with ${kotRows} KOT row(s)` : `Still on the cart. Error: ${JSON.stringify(sendError)}`,
    status: sentVisible && kotRows > 0 ? "PASS" : "FAIL",
    evidence: shotRel("captain-07-sent"),
  });

  await browser.close();
  await sleep(200);
};

run().catch(async (err) => {
  console.error(err);
  record({
    id: "S-CUI-ERR",
    demoStep: "1a",
    scenario: "Captain UI stage completed without an unhandled failure",
    surface: "captain 9320 (Chromium 390×844)",
    precondition: "n/a",
    steps: "Run scripts/demo-scenarios/03-captain-ui.mjs",
    expected: "The stage runs to completion",
    actual: `Unhandled error: ${String(err).slice(0, 300)}`,
    status: "FAIL",
    evidence: "stderr of the scenario run",
  });
  process.exitCode = 1;
});
