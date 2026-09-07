#!/usr/bin/env node
// M6 Phase C (C-4): platform vocabulary may not leak out of its adapter.
//
// WHY THIS IS A BUILD CHECK AND NOT A CONVENTION. A one-adapter abstraction is
// indistinguishable from no abstraction: the internal contract silently takes
// the shape of whichever platform was written first, and the tell is always the
// same -- a platform's name appearing somewhere it has no business being. By
// the time anyone notices, the second platform has arrived and the whole thing
// is redone.
//
// So the boundary is enforced structurally. If `ondc`, `beckn`, `swiggy`,
// `zomato`, an `on_*` callback name or a Beckn signing header appears outside
// `backend/internal/aggregators/adapters/`, this fails the build.
//
// A BOUNDARY NOBODY HAS WATCHED FAIL IS NOT A BOUNDARY. M6 C8's falsifier is to
// introduce a platform-specific branch in the core, watch this go RED, and then
// remove it -- recorded in the acceptance file with the output of both runs.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";

const ROOT = process.cwd();

// Searched for leaks. Deliberately NOT the whole repository: the contracts
// package, the docs and this script itself all legitimately discuss platforms.
const SEARCH_ROOTS = ["backend/internal", "backend/cmd", "edge", "apps/pos/src", "apps/admin/src"];

// The one place platform vocabulary is allowed to live.
const ADAPTER_DIR = join("backend", "internal", "aggregators", "adapters");

const EXTENSIONS = new Set([".go", ".ts", ".tsx", ".rs"]);

// Word-boundary matched, case-insensitive. Two groups, and the distinction
// matters when reading a failure:
//
//   PLATFORM NAMES  -- a platform's identity reaching code that should be
//                      platform-agnostic. This is the abstraction failing.
//   PROTOCOL TOKENS -- Beckn's callback actions and signing headers. These
//                      leaking means the transport shape has reached the core,
//                      which is the same failure one level down.
const FORBIDDEN = [
  // Platform names.
  "swiggy",
  "zomato",
  "ondc",
  "beckn",
  // Beckn callback actions. The whole on_* family, not a sample: an
  // abstraction that leaks `on_update` is as broken as one that leaks
  // `on_confirm`, and listing only the ones we happen to have implemented
  // would let the next one through.
  "on_search",
  "on_select",
  "on_init",
  "on_confirm",
  "on_status",
  "on_cancel",
  "on_update",
  "on_track",
  "on_rating",
  "on_support",
  "on_subscribe",
  // Beckn participant identifiers and signing headers, in their Beckn sense.
  "bpp_id",
  "bap_id",
  "bpp_uri",
  "bap_uri",
  "x-gateway-authorization",
];

const pattern = new RegExp(`\\b(${FORBIDDEN.map((t) => t.replace(/[-/\\^$*+?.()|[\]{}]/g, "\\$&")).join("|")})\\b`, "i");

function walk(dir, out) {
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    return out;
  }
  for (const entry of entries) {
    if (entry === "node_modules" || entry === "target" || entry === "dist" || entry === ".git") continue;
    const full = join(dir, entry);
    let st;
    try {
      st = statSync(full);
    } catch {
      continue;
    }
    if (st.isDirectory()) {
      walk(full, out);
    } else if (EXTENSIONS.has(entry.slice(entry.lastIndexOf(".")))) {
      out.push(full);
    }
  }
  return out;
}

const files = [];
for (const root of SEARCH_ROOTS) walk(join(ROOT, root), files);

const violations = [];
let scanned = 0;

for (const file of files) {
  const rel = relative(ROOT, file);
  // The adapters are where this vocabulary belongs.
  if (rel.split(sep).join("/").startsWith(ADAPTER_DIR.split(sep).join("/"))) continue;
  scanned++;

  const lines = readFileSync(file, "utf8").split(/\r?\n/);
  lines.forEach((line, i) => {
    const m = line.match(pattern);
    if (m) violations.push({ file: rel, line: i + 1, token: m[1], text: line.trim().slice(0, 120) });
  });
}

if (violations.length > 0) {
  console.error("check-aggregator-boundary: platform vocabulary outside its adapter\n");
  for (const v of violations) {
    console.error(`  ${v.file}:${v.line}  "${v.token}"`);
    console.error(`      ${v.text}`);
  }
  console.error(`
${violations.length} violation(s) across ${scanned} scanned file(s).

A platform's name or protocol vocabulary appearing outside
backend/internal/aggregators/adapters/ means the core has learned which
platform it is talking to. That is the abstraction failing, and it fails
QUIETLY -- everything still compiles and every test still passes, right up
until the next platform needs a different branch in the same place.

Move the platform-specific part into its adapter. If a genuinely
platform-agnostic identifier collides with this list, rename the identifier
rather than widening the check: the check is cheap and the abstraction is not.`);
  process.exit(1);
}

console.log(
  `check-aggregator-boundary: OK — ${scanned} file(s) outside the adapter directory carry no platform vocabulary.`,
);
