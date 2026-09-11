/**
 * Stage 6 — the admin console on 5175 against the REAL backend on 8080.
 *
 * Two scenarios here are KNOWN-ABSENT and are recorded as NOT BUILT rather
 * than skipped: demo step 4 (orders) and demo step 5 (stock variance) each
 * name an admin screen that does not exist — src/components holds exactly
 * SignIn, MenuScreen, SuppliersScreen and GoodsReceiptsScreen.
 *
 * LOGIN IS BUDGETED. backend/internal/auth/ratelimit.go allows five attempts
 * per fifteen minutes per client IP and a rate-limited login is byte-identical
 * to a wrong password, so this stage signs in ONCE and never retries.
 */
import { createRequire } from "node:module";
import { join } from "node:path";
import { ADMIN_UI, CASHIER, OWNER, REPO_ROOT, record, shotPath, shotRel } from "./lib.mjs";

const require = createRequire(join(REPO_ROOT, "apps", "kds", "package.json"));
const { chromium } = require("@playwright/test");

const notBuilt = (id, step, scenario, why) =>
  record({
    id,
    demoStep: step,
    scenario,
    surface: "admin 5175",
    precondition: "The admin console signed in",
    steps: "Enumerate apps/admin/src/components and the tab bar for the screen",
    expected: `A ${scenario.toLowerCase()} screen the demo can show`,
    actual: why,
    status: "FAIL",
    evidence:
      "apps/admin/src/components contains exactly SignIn.tsx, MenuScreen.tsx, SuppliersScreen.tsx, " +
      "GoodsReceiptsScreen.tsx; apps/admin/src/lib/api.ts exposes only menu, supplier and goods-receipt endpoints",
    notes: "KNOWN ABSENT — recorded as FAIL rather than skipped, because the demo script names this step.",
  });

const run = async () => {
  const browser = await chromium.launch();
  const page = await browser.newContext({ viewport: { width: 1440, height: 900 } }).then((c) => c.newPage());
  const consoleErrors = [];
  page.on("console", (m) => { if (m.type() === "error") consoleErrors.push(m.text()); });
  page.on("pageerror", (e) => consoleErrors.push(`pageerror: ${e.message}`));

  // -------------------------------------------------------------- S-ADM-01
  let resp = null;
  try {
    resp = await page.goto(ADMIN_UI, { waitUntil: "domcontentloaded", timeout: 15000 });
  } catch (err) {
    record({
      id: "S-ADM-01",
      demoStep: "2",
      scenario: "The admin console loads and shows its sign-in form",
      surface: "admin 5175",
      precondition: "Vite dev server on 5175",
      steps: "Navigate to http://localhost:5175/",
      expected: "200 and a 'Sign in' heading",
      actual: `Could not reach the dev server: ${String(err).slice(0, 160)}`,
      status: "BLOCKED",
      evidence: "navigation error; nothing listening on 5175",
    });
    for (const [id, step, scenario] of [
      ["S-ADM-02", "2", "Sign in to the admin console with real credentials"],
      ["S-ADM-03", "2", "Menu and pricing loads from the live backend"],
      ["S-ADM-04", "2", "A price edit persists through the backend"],
      ["S-ADM-05", "2", "Suppliers screen loads"],
      ["S-ADM-06", "6", "Goods receipts screen loads"],
      ["S-ADM-07", "2", "No raw UUID is shown to a human on any admin screen"],
    ]) {
      record({
        id, demoStep: step, scenario, surface: "admin 5175",
        precondition: "The admin dev server answering on 5175",
        steps: "n/a", expected: "n/a",
        actual: "BLOCKED by S-ADM-01: nothing is listening on 5175",
        status: "BLOCKED", evidence: "port 5175 refused",
      });
    }
    notBuilt("S-ADM-08", "4", "Orders screen", "No orders screen exists in apps/admin.");
    notBuilt("S-ADM-09", "5", "Stock variance screen", "No stock-variance screen exists in apps/admin.");
    await browser.close();
    return;
  }
  await page.waitForTimeout(1500);
  const envError = await page.locator("p.error").filter({ hasText: "is not set" }).count();
  const signInVisible = await page.getByRole("heading", { name: "Sign in" }).isVisible().catch(() => false);
  await page.screenshot({ path: shotPath("admin-01-signin"), fullPage: true });
  record({
    id: "S-ADM-01",
    demoStep: "2",
    scenario: "The admin console loads and shows its sign-in form",
    surface: "admin 5175",
    precondition: "Vite dev server on 5175; VITE_ADMIN_API_BASE_URL=http://localhost:8080 in apps/admin/.env.local",
    steps: "Navigate to http://localhost:5175/",
    expected: "200, a 'Sign in' heading, and no 'VITE_… is not set' env error",
    actual: `HTTP ${resp?.status()}; Sign in heading=${signInVisible}; env-error blocks=${envError}; console errors=${consoleErrors.length}`,
    status: resp?.status() === 200 && signInVisible && envError === 0 ? "PASS" : "FAIL",
    evidence: shotRel("admin-01-signin"),
  });

  // -------------------------------------------------------------- S-ADM-02
  // ONE attempt. No retry loop — see the budget note at the top of this file.
  await page.getByLabel("Email").fill(OWNER.email);
  await page.getByLabel("Password").fill(OWNER.password);
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.waitForTimeout(4000);
  const signedIn = await page.getByRole("heading", { name: "Menu and pricing" }).isVisible().catch(() => false);
  const signInError = await page.locator("p.error").first().textContent().catch(() => null);
  await page.screenshot({ path: shotPath("admin-02-after-signin"), fullPage: true });
  record({
    id: "S-ADM-02",
    demoStep: "2",
    scenario: "Sign in to the admin console with real credentials against the live backend",
    surface: "admin 5175 → backend 8080",
    precondition: `${OWNER.email} exists; login budget not exhausted for this IP`,
    steps: "Fill Email and Password, click Sign in (ONE attempt — the endpoint allows five per fifteen minutes per IP)",
    expected: "The tab bar and the Menu and pricing screen replace the form",
    actual: signedIn ? "Signed in; Menu and pricing screen shown" : `Still on Sign in. Message: ${JSON.stringify(signInError)}`,
    status: signedIn ? "PASS" : "FAIL",
    evidence: shotRel("admin-02-after-signin"),
    notes: signedIn
      ? "Session is in-memory only: any reload returns to Sign in, which costs another attempt against the budget."
      : "A failed sign-in here is ambiguous by design — the same body is returned for a wrong password and for an " +
        "exhausted rate-limit budget (S-BE-09).",
  });

  if (!signedIn) {
    for (const [id, step, scenario] of [
      ["S-ADM-03", "2", "Menu and pricing loads from the live backend"],
      ["S-ADM-04", "2", "A price edit persists through the backend"],
      ["S-ADM-05", "2", "Suppliers screen loads"],
      ["S-ADM-06", "6", "Goods receipts screen loads"],
      ["S-ADM-07", "2", "No raw UUID is shown to a human on any admin screen"],
    ]) {
      record({
        id, demoStep: step, scenario, surface: "admin 5175",
        precondition: "A signed-in admin session",
        steps: "n/a", expected: "n/a",
        actual: "BLOCKED by S-ADM-02: sign-in did not succeed",
        status: "BLOCKED", evidence: shotRel("admin-02-after-signin"),
      });
    }
    notBuilt("S-ADM-08", "4", "Orders screen", "No orders screen exists in apps/admin.");
    notBuilt("S-ADM-09", "5", "Stock variance screen", "No stock-variance screen exists in apps/admin.");
    await browser.close();
    return;
  }

  // -------------------------------------------------------------- S-ADM-03
  await page.waitForTimeout(1500);
  const menuRows = page.getByRole("row");
  const menuRowCount = await menuRows.count();
  const loadingMenu = await page.getByText("Loading menu…").count();
  await page.screenshot({ path: shotPath("admin-03-menu"), fullPage: true });
  record({
    id: "S-ADM-03",
    demoStep: "2",
    scenario: "Menu and pricing loads real rows from the live backend",
    surface: "admin 5175 → backend 8080",
    precondition: "Signed in; the cloud holds 43 menu_item rows",
    steps: "Read the menu table rows",
    expected: "A populated table, not a loading or empty state",
    actual: `${menuRowCount} table rows (including the header); 'Loading menu…' present=${loadingMenu > 0}`,
    status: menuRowCount > 1 && loadingMenu === 0 ? "PASS" : "FAIL",
    evidence: shotRel("admin-03-menu"),
  });

  // -------------------------------------------------------------- S-ADM-04
  // A price edit that must survive a round trip through the backend. The
  // falsifier is the read-back: a screen that only updates its own cache
  // would look identical until something else re-fetched.
  let priceResult = "not attempted";
  let priceStatus = "FAIL";
  const editRow = page.getByRole("row").filter({ has: page.getByRole("button", { name: "Edit" }) }).first();
  if (await editRow.count()) {
    const rowText = (await editRow.textContent()) ?? "";
    await editRow.getByRole("button", { name: "Edit" }).click();
    await page.waitForTimeout(600);
    const priceInput = page.getByLabel("Price");
    const original = await priceInput.inputValue();
    const bumped = (Math.round((Number(original) + 1) * 100) / 100).toFixed(2);
    await priceInput.fill(bumped);
    await page.getByRole("button", { name: "Save" }).click();
    await page.waitForTimeout(3000);
    const saveError = await page.locator("span.error").first().textContent().catch(() => null);
    // Read back from the SERVER, not from the screen's cache.
    await page.getByRole("button", { name: "Suppliers" }).click();
    await page.waitForTimeout(1200);
    await page.getByRole("button", { name: "Menu and pricing" }).click();
    await page.waitForTimeout(2500);
    const reread = (await page.getByRole("row").filter({ hasText: rowText.slice(0, 20).trim() }).first().textContent().catch(() => "")) ?? "";
    const persisted = reread.includes(bumped);
    priceStatus = persisted ? "PASS" : "FAIL";
    priceResult = `Edited ${original} → ${bumped}; save error=${JSON.stringify(saveError)}; after leaving and re-entering the screen the row reads ${persisted ? `${bumped} (persisted)` : `"${reread.slice(0, 120)}" (NOT persisted)`}`;
  } else {
    priceResult = "No row exposed an Edit button";
  }
  await page.screenshot({ path: shotPath("admin-04-price-edit"), fullPage: true });
  record({
    id: "S-ADM-04",
    demoStep: "2",
    scenario: "A price edit persists through the backend, not just in the screen's cache",
    surface: "admin 5175 → backend 8080 → postgres",
    precondition: "Menu and pricing loaded with at least one editable row",
    steps: "Edit, bump the price by ₹1, Save, navigate away to Suppliers and back, re-read the row",
    expected: "The re-read row shows the new price — proof it went through PATCH /menu/items/{itemId}",
    actual: priceResult,
    status: priceStatus,
    evidence: shotRel("admin-04-price-edit"),
    notes:
      "The navigate-away-and-back is the falsifier: a screen that only mutated local state would show the new price " +
      "until something re-fetched, and the naive assertion would pass.",
  });

  // -------------------------------------------------------------- S-ADM-05
  await page.getByRole("button", { name: "Suppliers" }).click();
  await page.waitForTimeout(2500);
  const suppliersHeading = await page.getByRole("heading", { name: "Suppliers and pack sizes" }).isVisible().catch(() => false);
  const suppliersError = await page.locator("p.error").first().textContent().catch(() => null);
  const supplierCards = await page.locator("article.card").count();
  const suppliersEmpty = await page.getByText("No suppliers yet.").count();
  await page.screenshot({ path: shotPath("admin-05-suppliers"), fullPage: true });
  record({
    id: "S-ADM-05",
    demoStep: "2",
    scenario: "Suppliers and pack sizes loads from the live backend",
    surface: "admin 5175 → backend 8080",
    precondition: "Signed in",
    steps: "Click the Suppliers tab, wait for the query to settle",
    expected: "The heading renders and the screen shows suppliers or its empty state, with no error",
    actual: `heading=${suppliersHeading}; supplier cards=${supplierCards}; empty state shown=${suppliersEmpty > 0}; error=${JSON.stringify(suppliersError)}`,
    status: suppliersHeading && !suppliersError ? "PASS" : "FAIL",
    evidence: shotRel("admin-05-suppliers"),
  });

  // -------------------------------------------------------------- S-ADM-06
  await page.getByRole("button", { name: "Goods receipts" }).click();
  await page.waitForTimeout(2500);
  const grnHeading = await page.getByRole("heading", { name: "Goods receipts" }).isVisible().catch(() => false);
  const grnError = await page.locator("p.error").first().textContent().catch(() => null);
  const grnCards = await page.locator("article.card").count();
  const grnEmpty = await page.getByText("No goods receipts have replayed for this outlet yet.").count();
  await page.screenshot({ path: shotPath("admin-06-goods-receipts"), fullPage: true });
  record({
    id: "S-ADM-06",
    demoStep: "6",
    scenario: "Goods receipts loads and is labelled as the replica it is",
    surface: "admin 5175 → backend 8080",
    precondition: "Signed in; a GRN is edge-authoritative and the cloud holds a copy (ADR-019)",
    steps: "Click the Goods receipts tab, wait for the query to settle",
    expected: "The heading renders; receipts or the empty state, no error",
    actual: `heading=${grnHeading}; GRN cards=${grnCards}; empty state shown=${grnEmpty > 0}; error=${JSON.stringify(grnError)}`,
    status: grnHeading && !grnError ? "PASS" : "FAIL",
    evidence: shotRel("admin-06-goods-receipts"),
    notes: "GET /procurement/goods-receipts serves a REPLICA and every surface must label it as one (contracts 0.7.0).",
  });

  // -------------------------------------------------------------- S-ADM-07
  const uuidRe = /\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi;
  const found = {};
  for (const tab of ["Menu and pricing", "Suppliers", "Goods receipts"]) {
    await page.getByRole("button", { name: tab }).click();
    await page.waitForTimeout(2000);
    const text = await page.locator("body").innerText();
    found[tab] = [...new Set(text.match(uuidRe) ?? [])].length;
  }
  const totalUuids = Object.values(found).reduce((a, b) => a + b, 0);
  await page.screenshot({ path: shotPath("admin-07-uuid-scan"), fullPage: true });
  record({
    id: "S-ADM-07",
    demoStep: "2",
    scenario: "No raw UUID is shown to a human on any admin screen",
    surface: "admin 5175",
    precondition: "All three tabs reachable",
    steps: "Read the rendered text of each tab and match the UUID pattern",
    expected: "0 — human-facing identifiers are short (Order #A184, GRN/20260902), never a primary key",
    actual: `Distinct UUIDs visible per tab: ${Object.entries(found).map(([k, v]) => `${k}=${v}`).join(", ")}`,
    status: totalUuids === 0 ? "PASS" : "FAIL",
    evidence: shotRel("admin-07-uuid-scan"),
    notes:
      totalUuids > 0
        ? "CLAUDE.md: 'Human-facing numbers are short … Never expose sequential PKs as security identifiers.' A UUID " +
          "on screen is not a security hole but it is unreadable in a demo, and the Suppliers form asks a human to " +
          "TYPE an 'Inventory item id'."
        : "",
  });

  // --------------------------------------------------- S-ADM-08 / S-ADM-09
  notBuilt(
    "S-ADM-08",
    "4",
    "Orders screen",
    "NOT BUILT. The admin console has three tabs — Menu and pricing, Suppliers, Goods receipts. There is no orders " +
      "screen, no order route in apps/admin/src/lib/api.ts, and no component for one. Demo step 4 has no surface.",
  );
  notBuilt(
    "S-ADM-09",
    "5",
    "Stock variance screen",
    "NOT BUILT. No stock, variance or count screen exists in the admin console and no inventory endpoint is wired " +
      "into apps/admin/src/lib/api.ts. Demo step 5 has no surface — and gap A7 means the underlying stock_count rows " +
      "never reach the cloud either (S-SYNC-04), so the data is absent as well as the screen.",
  );

  await browser.close();
};

run().catch((err) => {
  console.error(err);
  record({
    id: "S-ADM-ERR",
    demoStep: "2",
    scenario: "Admin UI stage completed without an unhandled failure",
    surface: "admin 5175",
    precondition: "n/a",
    steps: "Run scripts/demo-scenarios/06-admin-ui.mjs",
    expected: "The stage runs to completion",
    actual: `Unhandled error: ${String(err).slice(0, 300)}`,
    status: "FAIL",
    evidence: "stderr of the scenario run",
  });
  process.exitCode = 1;
});
