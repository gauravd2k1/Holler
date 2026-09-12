#!/usr/bin/env node
/**
 * Photographs POS screens by running the REAL app with the Tauri IPC replaced
 * by contract-shaped fixtures.
 *
 * COMMITTED, AND THAT IS THE POINT. Two earlier demo-build passes built a
 * harness like this in a scratch directory and committed only the PNGs, so the
 * third pass had to write it again from nothing. This one re-runs:
 *
 *     node scripts/pos-screens.mjs             # starts its own dev server
 *     POS_UI=http://localhost:5198/ node scripts/pos-screens.mjs   # or reuse one
 *
 * SCRATCH PORTS ONLY (5198 by default). It never touches 8080, 9310 or the
 * captain port, and it needs no backend, no database and no Tauri shell.
 *
 * WHY FIXTURES AND NOT THE REAL TILL. A Tauri window cannot be launched from a
 * tool with redirected stdio -- it never appears -- so the only way to see
 * these screens from a script is the browser with `window.__TAURI_INTERNALS__`
 * stubbed. What that buys is real components, real CSS, real routing and real
 * Zod parsing: a shape the contract would reject fails here exactly as it
 * would on the till. What it does NOT buy is the Rust side, and no claim about
 * command behaviour may be made from these captures.
 */
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { spawn } from "node:child_process";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

// THE RESTAURANT'S NAME COMES FROM THE SEED, NOT FROM THIS FILE.
// `seed/demo-outlet.json` is the single source of truth; a fixture holding its
// own copy keeps showing the old name after a rename, and the screenshot then
// argues for a screen nobody has.
const seed = JSON.parse(readFileSync(join(repoRoot, "seed", "demo-outlet.json"), "utf8"));
const require = createRequire(join(repoRoot, "apps", "kds", "package.json"));
const { chromium } = require("@playwright/test");

const PORT = Number(process.env.POS_PORT ?? 5198);
const UI = process.env.POS_UI ?? `http://localhost:${PORT}/`;
const outDir = join(repoRoot, "docs", "demo-screens", "pos");
mkdirSync(outDir, { recursive: true });

// ---------------------------------------------------------------------------
// Fixtures. the client's own data at their own prices, shaped by packages/contracts -- the app
// parses every one of these with Zod, so a wrong shape fails the capture
// rather than rendering something the till could never show.
// ---------------------------------------------------------------------------
const OUTLET = "0191a000-0000-7000-8000-00000000000a";
const ORDER = "01a09700-0000-7000-8000-000000000001";
const INVOICE = "01a09700-0000-7000-8000-000000000002";

const order = {
  holler_order_id: ORDER,
  external_order_id: null,
  source: "POS",
  outlet_id: OUTLET,
  display_number: "A187",
  order_type: "DINE_IN",
  status: "SENT_TO_KITCHEN",
  table_id: "01a09700-0000-7000-8000-000000000010",
  customer: null,
  delivery_address: null,
  items: [
    { id: "01a09700-0000-7000-8000-000000000021", menu_item_id: "0191e200-0000-7000-8000-000000000061",
      variant_id: "0191e300-0000-7000-8000-000000000076", quantity: 1, unit_price_paise: 79500,
      line_total_paise: 79500, modifiers: [], notes: null },
    { id: "01a09700-0000-7000-8000-000000000022", menu_item_id: "0191e200-0000-7000-8000-000000000060",
      variant_id: "0191e300-0000-7000-8000-000000000074", quantity: 2, unit_price_paise: 59500,
      line_total_paise: 119000, modifiers: [], notes: null },
  ],
  subtotal_paise: 198500,
  discount_paise: 0,
  packaging_paise: 0,
  delivery_charge_paise: 0,
  taxes_paise: 9925,
  aggregator_discount_paise: 0,
  merchant_discount_paise: 0,
  total_paise: 208425,
  payment_status: "UNPAID",
  payment_source: null,
  preparation_time_minutes: null,
  rider: null,
  timestamps: { created_at: "2026-09-12T08:30:00Z", confirmed_at: "2026-09-12T08:31:00Z", updated_at: "2026-09-12T08:31:00Z" },
  source_payload: null,
  schema_version: 1,
};

const invoice = {
  id: INVOICE,
  outlet_id: OUTLET,
  order_id: ORDER,
  split_group_id: null,
  split_index: 1,
  split_count: 1,
  series_id: "0191a000-0000-7000-8000-000000000040",
  invoice_number: "FY26/PNQ/001423",
  invoice_date: "2026-09-12T08:35:00Z",
  business_date: "2026-09-12",
  status: "ISSUED",
  cancelled_reason: null,
  cancelled_at: null,
  customer_name: "Walk-in",
  customer_phone: null,
  customer_gstin: null,
  place_of_supply_state_code: "27",
  subtotal_paise: 198500,
  discount_paise: 0,
  taxable_value_paise: 198500,
  cgst_paise: 4962,
  sgst_paise: 4962,
  igst_paise: 0,
  cess_paise: 0,
  // 198500 + 4962 + 4962 = 208424, which rounds DOWN to 208400: the delta is
  // -24, not +76. The first version of this fixture rounded up and
  // InvoiceSchema refused it -- round_off cannot exceed half a rupee (ADR-016)
  // -- so the screen silently fell back to its pre-issue state. The schema was
  // right and the fixture was wrong, which is the whole reason these captures
  // parse through the contract rather than around it.
  round_off_paise: -24,
  grand_total_paise: 208400,
  compliance_version_id: "0191e700-0000-7000-8000-000000000001",
  // THE TS WIRE SHAPE CARRIES PARSED OBJECTS, not the `*_json` strings the
  // SQLite row stores: `tax_snapshot` and `fiscal_profile` are
  // z.record(z.unknown()). The first fixture sent the stored spelling and Zod
  // refused it -- the same class of mistake as the round_off above, and the
  // reason this harness parses through the contract instead of around it.
  tax_snapshot: {},
  fiscal_profile: ({
    legal_name: seed.tenant.name, trade_name: seed.outlet.name,
    // Equal to devseed.rs's OUTLET_* constants, mirrored for the same reason
    // receipt_preview mirrors them: the fiscal profile is opt-in billing config
    // and `devseed --emit-json` does not write it, so it cannot be read out of
    // seed/demo-outlet.json the way the names above are.
    address_line1: "Camp", address_line2: null, city: "Pune",
    state_code: "27", state_name: "Maharashtra", pincode: "411001",
    gstin: "27AAAAA0000A1Z5", fssai_number: "11522998000123",
    invoice_footer_text: "Thank you — please visit again",
  }),
  channel: "POS",
  tax_liability_party: "RESTAURANT",
  eco_operator_name: null,
  eco_operator_gstin: null,
  supply_classification: null,
  created_by_user_id: "0191a000-0000-7000-8000-00000000000c",
  created_at: "2026-09-12T08:35:00Z",
  updated_at: "2026-09-12T08:35:00Z",
  version: 1,
  sync_status: "PENDING",
  schema_version: 1,
  // INSIDE the invoice, because that is where InvoiceSchema puts them. The
  // first version of this fixture invented a `list_invoice_lines` command and
  // left `lines` off, so InvoiceSchema rejected every invoice, the query
  // errored, and the screen rendered its PRE-ISSUE state -- a true screen, and
  // not the one demo step 2 shows. Zod refusing a wrong shape here is the
  // harness working.
  lines: [],
};

const invoiceLines = [
  { id: "01a09700-0000-7000-8000-000000000031", invoice_id: INVOICE, order_item_id: order.items[0].id,
    line_no: 1, description: "Kimchi Stone Bowl — Chicken", hsn_sac: "996331", quantity: 1,
    unit_price_paise: 79500, gross_paise: 79500, discount_paise: 0, taxable_value_paise: 79500,
    tax_profile_id: "0191e600-0000-7000-8000-000000000001", cgst_rate_bps: 250, cgst_paise: 1987,
    sgst_rate_bps: 250, sgst_paise: 1987, igst_rate_bps: 0, igst_paise: 0, cess_rate_bps: 0,
    cess_paise: 0, total_paise: 83474, schema_version: 1 },
  { id: "01a09700-0000-7000-8000-000000000032", invoice_id: INVOICE, order_item_id: order.items[1].id,
    line_no: 2, description: "Pad Thai", hsn_sac: "996331", quantity: 2,
    unit_price_paise: 59500, gross_paise: 119000, discount_paise: 0, taxable_value_paise: 119000,
    tax_profile_id: "0191e600-0000-7000-8000-000000000001", cgst_rate_bps: 250, cgst_paise: 2975,
    sgst_rate_bps: 250, sgst_paise: 2975, igst_rate_bps: 0, igst_paise: 0, cess_rate_bps: 0,
    cess_paise: 0, total_paise: 124950, schema_version: 1 },
];

invoice.lines = invoiceLines;

/** Every command the captured screens reach, and nothing else. */
const COMMANDS = {
  // AuthenticatedPrincipalSchema, in full: the app parses this with Zod, so a
  // missing field fails the capture instead of rendering a half-signed-in
  // screen.
  login: {
    user_id: "0191a000-0000-7000-8000-00000000000c",
    tenant_id: "0191a000-0000-7000-8000-000000000001",
    outlet_id: OUTLET,
    full_name: "Dev Cashier",
    permissions: ["billing.manage", "order.create"],
    authenticated_offline: true,
    schema_version: 1,
  },
  list_orders: [order],
  get_order: order,
  list_invoices_for_order: [invoice],
  list_payments_for_order: [],
  list_discount_definitions: [],
  list_kots_for_order: [],
  list_menu_categories: [],
  list_menu_items: [],
  list_menu_item_variants: [],
  list_tables: [],
  // The banner/stock queries the till shell fires on every screen. Named from
  // a trace of the real app rather than guessed: the first version of this
  // table invented `sync_blocked_rows` and `sync_retrying_rows`, which the app
  // never calls.
  list_failed_print_jobs: [],
  list_blocked_outbox_rows: [],
  list_persistently_failing_outbox_rows: [],
  list_current_stock: [],
  // The white-label header read: the till asks who it belongs to rather than
  // holding a copy of the name.
  get_outlet_identity: { name: seed.outlet.name, outlet_id: OUTLET },
  get_active_draft_order: null,
  list_stations: [],
  // An OPEN shift, because billing is gated on one: without it the screen
  // shows "Open Shift" and no invoice, which is a true screen but not the one
  // demo step 2 shows. CashShiftSchema in full -- its refine() rejects a
  // half-filled CLOSED shift, so the shape is not negotiable.
  // Same row by id, because the screen re-reads the shift after opening it.
  get_cash_shift: null, // replaced below, once `openShift` is defined
  find_open_cash_shift: {
    id: "01a09700-0000-7000-8000-000000000040",
    outlet_id: OUTLET,
    device_id: "0191a000-0000-7000-8000-00000000000b",
    cashier_user_id: "0191a000-0000-7000-8000-00000000000c",
    status: "OPEN",
    opened_at: "2026-09-12T04:00:00Z",
    opening_cash_paise: 200000,
    closed_at: null,
    expected_cash_paise: null,
    actual_cash_paise: null,
    variance_paise: null,
    variance_reason: null,
    business_date: "2026-09-12",
    movements: [],
    created_at: "2026-09-12T04:00:00Z",
    updated_at: "2026-09-12T04:00:00Z",
    version: 1,
    schema_version: 1,
  },
};

COMMANDS.get_cash_shift = COMMANDS.find_open_cash_shift;

const initScript = `
  window.__TAURI_INTERNALS__ = {
    transformCallback: (cb) => cb,
    invoke: (cmd, args) => {
      const table = ${JSON.stringify(COMMANDS)};
      const key = cmd.replace(/^plugin:[^|]+\\|/, "");
      if (key in table) return Promise.resolve(table[key]);
      // LOUD, not silent: an unstubbed command means this harness is behind
      // the app, and a screen rendered from a swallowed error is a screenshot
      // of nothing.
      console.error("HARNESS: no fixture for Tauri command " + cmd);
      return Promise.reject(new Error("no fixture for " + cmd));
    },
  };
  window.__HOLLER_HARNESS__ = true;
`;

// ---------------------------------------------------------------------------
let server = null;
if (!process.env.POS_UI) {
  console.log(`starting the POS dev server on ${PORT}…`);
  server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], {
    cwd: join(repoRoot, "apps", "pos"),
    shell: true,
    stdio: "ignore",
    env: {
      ...process.env,
      // WITHOUT A VPA NO QR RENDERS AT ALL (docs/demo-status.md), so a capture
      // taken without these would show an invoice screen missing the thing
      // demo item 0b added, and nothing would say why. Fixture values: this
      // harness must never carry a real payee.
      // POS_NO_UPI=1 captures the UNCONFIGURED state instead -- the one the
      // operator met on 2026-09-12, where the screen said nothing at all.
      ...(process.env.POS_NO_UPI === "1"
        ? {}
        : {
            VITE_HOLLER_DEMO_UPI_VPA: process.env.VITE_HOLLER_DEMO_UPI_VPA ?? "payee@fixturebank",
            VITE_HOLLER_DEMO_UPI_PAYEE_NAME:
              process.env.VITE_HOLLER_DEMO_UPI_PAYEE_NAME ?? "Shinjuku Yakitori (fixture)",
          }),
    },
  });
  const deadline = Date.now() + 90_000;
  for (;;) {
    try {
      const r = await fetch(UI);
      if (r.ok) break;
    } catch { /* not up yet */ }
    if (Date.now() > deadline) throw new Error(`the dev server did not answer ${UI} within 90s`);
    await new Promise((r) => setTimeout(r, 1000));
  }
}

const browser = await chromium.launch();
const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
await context.addInitScript(initScript);
const page = await context.newPage();

const consoleErrors = [];
page.on("console", (m) => { if (m.type() === "error") consoleErrors.push(m.text()); });
page.on("pageerror", (e) => consoleErrors.push(`pageerror: ${e.message}`));

const captured = [];
async function capture(name, assertion) {
  if (!(await assertion())) {
    // Say WHY before dying: the commands with no fixture are almost always the
    // reason, and printing them turns "assertion failed" into a fix.
    for (const e of consoleErrors.filter((e) => e.startsWith("HARNESS"))) console.error(`  ${e}`);
    throw new Error(`refusing to photograph "${name}": its assertion failed`);
  }
  await page.screenshot({ path: join(outDir, `${name}.png`), fullPage: true });
  captured.push(name);
  console.log(`  captured ${name}`);
}

// SIGN IN BY FILLING THE FORM, never by writing the auth store directly: the
// app redirects an unauthenticated route to /login, and a harness that forged
// a session would be photographing a state the till cannot reach.
await page.goto(UI, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(1200);
if (page.url().includes("/login")) {
  await page.getByLabel("Email").fill("cashier@holler.test");
  await page.getByLabel("Password").fill("holler123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.waitForTimeout(1500);
}

// NAVIGATE BY CLICKING. `page.goto` after signing in reloads the app, and the
// POS session is in memory only -- the reload lands back on /login and the
// capture photographs the sign-in form under a billing-screen filename. That
// is the nine-identical-screenshots defect this repo already paid for
// (docs/demo-status.md); it is cheaper to click.
await page.getByRole("link", { name: /orders/i }).or(page.getByRole("button", { name: /orders/i })).first().click();
await page.waitForTimeout(1200);
await capture("pos-order-list", async () =>
  (await page.locator("text=A187").count()) > 0);

await page.getByRole("button", { name: "Bill", exact: true }).first().click();
await page.waitForTimeout(2500);

if (process.env.POS_TRACE) {
  console.log("  URL:", page.url());
  const body = await page.locator("body").innerText();
  console.log("  BODY:", body.slice(0, 500).replace(/[\r\n]+/g, " | "));
}
await capture("pos-billing-invoice", async () =>
  (await page.locator("text=FY26/PNQ/001423").count()) > 0);

// THE RESTAURANT'S NAME MUST BE ON THE SCREEN, and it must be the one the seed
// carries. A header that stops reading the outlet row, or starts holding its
// own copy, fails here rather than at a demo.
const headerText = await page.locator("header").first().innerText();
if (!headerText.includes(seed.outlet.name)) {
  throw new Error(
    `the header does not name the outlet: expected ${JSON.stringify(seed.outlet.name)}, got ${JSON.stringify(headerText)}`,
  );
}
console.log(`  header names the outlet: ${JSON.stringify(seed.outlet.name)}`);

// No UUID may reach this screen.
const text = await page.locator("body").innerText();
const uuids = text.match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi) ?? [];

console.log(`\n  console errors: ${consoleErrors.length}`);
for (const e of consoleErrors.slice(0, 5)) console.log(`    ${e}`);
console.log(`  raw UUIDs on screen: ${uuids.length}${uuids.length ? ` (${uuids[0]})` : ""}`);

const hashes = new Map();
for (const name of captured) {
  const h = createHash("sha256").update(readFileSync(join(outDir, `${name}.png`))).digest("hex").slice(0, 12);
  if (hashes.has(h)) throw new Error(`${name} is byte-identical to ${hashes.get(h)}`);
  hashes.set(h, name);
  console.log(`  ${name}  ${h}`);
}

await browser.close();
if (server) server.kill();
process.exit(consoleErrors.length === 0 && uuids.length === 0 ? 0 : 2);
