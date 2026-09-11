/**
 * Shared plumbing for the T27 end-to-end demo scenario suite.
 *
 * Every scenario in this directory drives a REAL service on the live stack —
 * backend 8080, captain 9320, KDS LAN socket 9310, Vite dev servers 5174/5175.
 * Nothing here mocks an endpoint. A scenario that cannot be driven records
 * NOT TESTABLE with its reason; it never substitutes a fake and reports PASS.
 *
 * Results accumulate as JSON lines under RESULT_DIR so each script can be run
 * on its own and `build-sheet.mjs` can assemble the workbook from whatever ran.
 */
import { appendFileSync, mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
export const RESULT_DIR = join(REPO_ROOT, ".scenario-results");
export const SHOT_DIR = join(REPO_ROOT, "docs", "demo-screens", "scenarios");

export const BACKEND = process.env.HOLLER_BACKEND ?? "http://localhost:8080";
export const CAPTAIN = process.env.HOLLER_CAPTAIN ?? "http://localhost:9320";
export const KDS_WS = process.env.HOLLER_KDS_WS ?? "ws://localhost:9310";
export const KDS_UI = process.env.HOLLER_KDS_UI ?? "http://localhost:5174";
export const ADMIN_UI = process.env.HOLLER_ADMIN_UI ?? "http://localhost:5175";

export const TENANT_ID = "0191a000-0000-7000-8000-000000000001";
export const OUTLET_ID = "0191a000-0000-7000-8000-00000000000a";

export const CASHIER = { email: "cashier@holler.test", password: "holler123" };
export const OWNER = { email: "owner@holler.test", password: "holler123" };

mkdirSync(RESULT_DIR, { recursive: true });
mkdirSync(SHOT_DIR, { recursive: true });

/**
 * A device token is `<credential_id>.<secret>`. It must never reach a
 * committed file, a screenshot or a report (task rule, and the 0.4.0 audit
 * redact list in spirit). Everything that names a token names this instead.
 */
export function fingerprint(token) {
  if (typeof token !== "string" || token.length === 0) return "<none>";
  const dot = token.indexOf(".");
  const credPrefix = (dot > 0 ? token.slice(0, dot) : token).slice(0, 8);
  return `${credPrefix}…/len=${token.length}`;
}

export function redact(text) {
  if (typeof text !== "string") return text;
  // credential_id.secret — uuid-ish dot base64/hex-ish
  return text.replace(/[0-9a-f-]{20,}\.[A-Za-z0-9_-]{16,}/g, "<redacted-token>");
}

const RESULTS_FILE = join(RESULT_DIR, "results.jsonl");

/**
 * Which stage recorded a row, and which execution of it.
 *
 * A STAGE'S RE-RUN SUPERSEDES EVERY ROW IT PREVIOUSLY WROTE, not merely the
 * ids it happens to write again. Without this, a stage that failed early and
 * emitted a placeholder (S-CAP-BLOCKED) leaves that row in the sheet forever,
 * because the successful re-run never emits that id to overwrite it — and the
 * sheet then shows a path both working and blocked at once. Same defect shape
 * as an exemption outliving its reason: the stale row reads exactly like a
 * fresh one.
 */
const STAGE = basename(process.argv[1] ?? "unknown");
const RUN_ID = new Date().toISOString();

/**
 * Record one scenario row. `status` is PASS / FAIL / BLOCKED / NOT TESTABLE.
 * Every field is written even when empty so the sheet's columns stay aligned.
 */
export function record(row) {
  const full = {
    id: row.id,
    demoStep: row.demoStep ?? "",
    scenario: row.scenario,
    surface: row.surface,
    precondition: row.precondition ?? "",
    steps: row.steps ?? "",
    expected: row.expected ?? "",
    actual: redact(String(row.actual ?? "")),
    status: row.status,
    evidence: redact(String(row.evidence ?? "")),
    notes: redact(String(row.notes ?? "")),
    stage: STAGE,
    runId: RUN_ID,
    at: new Date().toISOString(),
  };
  appendFileSync(RESULTS_FILE, JSON.stringify(full) + "\n", "utf8");
  const mark = { PASS: "PASS", FAIL: "FAIL", BLOCKED: "BLOCK", "NOT TESTABLE": "N/T" }[row.status] ?? "?";
  console.log(`[${mark.padEnd(5)}] ${row.id}  ${row.scenario}`);
  if (row.status !== "PASS") console.log(`         actual: ${full.actual}`);
  return full;
}

export function loadResults() {
  if (!existsSync(RESULTS_FILE)) return [];
  return readFileSync(RESULTS_FILE, "utf8")
    .split("\n")
    .filter((l) => l.trim() !== "")
    .map((l) => JSON.parse(l));
}

export function resetResults() {
  writeFileSync(RESULTS_FILE, "", "utf8");
}

/** Scenario-local state shared between scripts (ids, never secrets in git). */
const STATE_FILE = join(RESULT_DIR, "state.json");
export function saveState(patch) {
  const cur = loadState();
  writeFileSync(STATE_FILE, JSON.stringify({ ...cur, ...patch }, null, 2), "utf8");
}
export function loadState() {
  if (!existsSync(STATE_FILE)) return {};
  return JSON.parse(readFileSync(STATE_FILE, "utf8").replace(/^﻿/, ""));
}

/** HTTP helper that never throws on a non-2xx — the status IS the evidence. */
export async function http(url, opts = {}) {
  const started = Date.now();
  let res, text;
  try {
    res = await fetch(url, opts);
    text = await res.text();
  } catch (err) {
    return { ok: false, status: 0, transportError: String(err), body: null, text: "", ms: Date.now() - started };
  }
  let body = null;
  try {
    body = JSON.parse(text);
  } catch {
    /* not JSON — text stands */
  }
  return { ok: res.ok, status: res.status, body, text, headers: res.headers, ms: Date.now() - started };
}

/**
 * POST /auth/login.
 *
 * BUDGETED, DELIBERATELY. `LoginRateLimitAttempts = 5` per
 * `LoginRateLimitWindow = 15 minutes`, keyed on client IP
 * (backend/internal/auth/ratelimit.go), and a rate-limited login returns the
 * IDENTICAL `unauthorized` body a wrong password does — ADR-012 chose that on
 * purpose so the endpoint leaks nothing. The consequence for any suite that
 * logs in repeatedly is that the sixth attempt is indistinguishable from a
 * credential fault, which is exactly the misread CLAUDE.md records costing a
 * debugging detour. So: cache the token, and spend an attempt only when there
 * is no cached one.
 */
const JWT_CACHE = join(RESULT_DIR, "jwt-cache.json");

export async function login({ email, password }, { cache = true } = {}) {
  if (cache && existsSync(JWT_CACHE)) {
    const held = JSON.parse(readFileSync(JWT_CACHE, "utf8").replace(/^﻿/, ""));
    const entry = held[email];
    if (entry && Date.now() - entry.at < 10 * 60 * 1000) {
      return { ok: true, status: 200, body: entry.body, text: "", fromCache: true, ms: 0 };
    }
  }
  const res = await http(`${BACKEND}/auth/login`, {
    method: "POST",
    headers: { "content-type": "application/json", "X-Tenant-ID": TENANT_ID },
    body: JSON.stringify({ email, password, outlet_id: OUTLET_ID }),
  });
  if (cache && res.status === 200 && res.body?.access_token) {
    const held = existsSync(JWT_CACHE) ? JSON.parse(readFileSync(JWT_CACHE, "utf8").replace(/^﻿/, "")) : {};
    held[email] = { at: Date.now(), body: res.body };
    writeFileSync(JWT_CACHE, JSON.stringify(held), "utf8");
  }
  return res;
}

/** Run a read-only SQL statement against the live Postgres container. */
export function sql(statement) {
  return execFileSync(
    "docker",
    ["exec", "-i", "holler-postgres-1", "psql", "-U", "holler", "-d", "holler", "-t", "-A", "-F", "|", "-c", statement],
    { encoding: "utf8" },
  ).trim();
}

export function shotPath(name) {
  return join(SHOT_DIR, `${name}.png`);
}

/** Relative path for the Evidence column, so the sheet stays portable. */
export function shotRel(name) {
  return `docs/demo-screens/scenarios/${name}.png`;
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
