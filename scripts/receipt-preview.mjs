#!/usr/bin/env node
/**
 * Renders the sample receipt and photographs it, so the thing a customer is
 * handed can be REVIEWED rather than imagined.
 *
 * COMMITTED ON PURPOSE. The demo build's earlier screenshot harnesses lived in
 * a session's scratch directory and only their PNGs reached the repository, so
 * the next pass had to rebuild them from nothing. This one re-runs:
 *
 *     node scripts/receipt-preview.mjs
 *
 * Two halves, both real:
 *   1. `cargo run --bin receipt_preview` renders through `render_invoice_html`
 *      -- the SAME function the print path calls, not a copy of it.
 *   2. Chromium opens that HTML at 80mm-thermal width, screenshots it, and
 *      prints it to PDF, which is exactly what the demo's "receipt printed to
 *      PDF and opened on screen" step produces.
 *
 * NO LIVE PORTS. It renders a file from a fixture; there is no POS, no
 * database, and nothing bound.
 */
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(join(repoRoot, "apps", "kds", "package.json"));
const { chromium } = require("@playwright/test");

const outDir = join(repoRoot, "docs", "demo-screens", "receipt");
mkdirSync(outDir, { recursive: true });

const htmlPath = join(tmpdir(), "holler-receipt-preview.html");

console.log("1/2  rendering through render_invoice_html…");
execFileSync(
  "cargo",
  ["run", "--quiet", "--manifest-path", join(repoRoot, "edge/printer/Cargo.toml"),
   "--bin", "receipt_preview", "--", htmlPath],
  { stdio: "inherit" },
);

console.log("2/2  photographing it…");
const browser = await chromium.launch();
// 80mm thermal paper is ~302 CSS px of printable width. The page is captured
// at that width because a receipt reviewed at 1440px is a receipt nobody
// checked: every wrapping decision in it depends on the real column.
const page = await browser.newPage({ viewport: { width: 302, height: 1400 }, deviceScaleFactor: 2 });

const consoleErrors = [];
page.on("console", (m) => { if (m.type() === "error") consoleErrors.push(m.text()); });
page.on("pageerror", (e) => consoleErrors.push(`pageerror: ${e.message}`));

await page.goto(pathToFileURL(htmlPath).href, { waitUntil: "networkidle" });

// Assert the receipt's own content BEFORE capturing it: a blank page
// photographs just as willingly as a good one.
const text = await page.locator("body").innerText();
// The restaurant's name is asserted FROM THE SEED, so this check follows a
// rename instead of pinning the name it happened to be written with.
const seed = JSON.parse(readFileSync(join(repoRoot, "seed", "demo-outlet.json"), "utf8"));
for (const required of [seed.outlet.name, "FY26/PNQ/001423", "27AAAAA0000A1Z5", "Pad Thai", "CGST"]) {
  if (!text.includes(required)) {
    throw new Error(`refusing to photograph the receipt: it does not contain ${JSON.stringify(required)}`);
  }
}
// The rule that keeps being broken elsewhere, checked here too.
const uuids = text.match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi) ?? [];
if (uuids.length > 0) {
  throw new Error(`the receipt shows ${uuids.length} raw UUID(s), starting with ${uuids[0]}`);
}

const png = join(outDir, "receipt-80mm.png");
await page.screenshot({ path: png, fullPage: true });

const pdf = join(outDir, "receipt-80mm.pdf");
await page.pdf({ path: pdf, width: "80mm", printBackground: true, margin: { top: "4mm", bottom: "4mm" } });

await browser.close();

const hash = (p) => createHash("sha256").update(readFileSync(p)).digest("hex").slice(0, 12);
console.log(`\n  ${png}  ${hash(png)}`);
console.log(`  ${pdf}  ${hash(pdf)}`);
console.log(`  console errors: ${consoleErrors.length}`);
console.log(`  raw UUIDs on the receipt: ${uuids.length}`);

process.exit(consoleErrors.length === 0 ? 0 : 2);
