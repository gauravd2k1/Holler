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
// packages/contracts was OUTSIDE this list until contracts 0.8.1, and .sql was
// outside EXTENSIONS, so the shared schema -- the one place a platform name
// would be hardest to remove later and would bind every consumer at once --
// was the one place this check could not see. Found while widening
// `order.source`: a per-platform member such as AGGREGATOR_ONDC would have
// landed in both stores with the boundary check green. Watched failing against
// a planted member before this line was added (ADR-026).
const SEARCH_ROOTS = [
  "backend/internal",
  "backend/cmd",
  "edge",
  "apps/pos/src",
  "apps/admin/src",
  "packages/contracts",
];

// The one place platform vocabulary is allowed to live.
const ADAPTER_DIR = join("backend", "internal", "aggregators", "adapters");

const EXTENSIONS = new Set([".go", ".ts", ".tsx", ".rs", ".sql", ".yaml"]);

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

// TWO patterns, because the two groups need different boundary rules.
//
// PLATFORM NAMES are separator-aware: `` treats `_` as a word character, so
// `ondc` does NOT match AGGREGATOR_ONDC, ondc_platform or PLATFORM_ONDC --
// which is to say it misses the single most likely way a platform name enters a
// shared schema, as part of an enum member. Found by planting AGGREGATOR_ONDC
// in packages/contracts and watching this check stay GREEN (contracts 0.8.1,
// ADR-026).
//
// PROTOCOL TOKENS keep ``. `on_update` in its Beckn sense is a standalone
// callback name; `order_item_quantity_is_bounded_on_update` is a trigger whose
// name happens to end that way, and flagging it would teach everyone to ignore
// this check -- the failure mode of a check that cries wolf is that it stops
// being read at all.
const PROTOCOL_TOKENS = new Set([
  "beckn",
  "on_search", "on_select", "on_init", "on_confirm", "on_status", "on_cancel",
  "on_update", "on_track", "on_rating", "on_support", "on_subscribe",
  "bpp_id", "bap_id", "bpp_uri", "bap_uri", "x-gateway-authorization",
]);

const escapeToken = (t) => t.replace(/[-/\^$*+?.()|[\]{}]/g, "\$&");
const platformNames = FORBIDDEN.filter((t) => !PROTOCOL_TOKENS.has(t));
const protocolTokens = FORBIDDEN.filter((t) => PROTOCOL_TOKENS.has(t));

const patterns = [
  new RegExp(`(?<![A-Za-z0-9])(${platformNames.map(escapeToken).join("|")})(?![A-Za-z0-9])`, "i"),
  new RegExp(`\b(${protocolTokens.map(escapeToken).join("|")})\b`, "i"),
];

// The DEPRECATED members of order.source (ADR-026). AGGREGATOR_ZOMATO and
// AGGREGATOR_SWIGGY name platforms in the shared schema and are exactly what
// this check exists to prevent -- but they were written in contracts 0004,
// nothing has ever emitted either, and REMOVING a CHECK member is a breaking
// change. They are carried, deprecated, until the next breaking bump.
//
// The exemption is deliberately NARROW: declaration files under
// packages/contracts only. The same member appearing in backend, edge or app
// code is still a violation, because that would be the core branching on a
// platform rather than a schema carrying a legacy value.
//
// REMOVAL TRIGGER: the next breaking contracts bump removes both members, and
// removes this exemption in the same commit. An exemption that outlives its
// reason is a silenced failure (contracts 0.6.0).
const DEPRECATED_MEMBER_LINE = /AGGREGATOR_(ZOMATO|SWIGGY)/;
const EXEMPT_ROOT = "packages/contracts";

// The ONE file outside packages/contracts that may name a deprecated member:
// the migration runner that refuses to widen over one. Its guard has a test,
// and a test proving "the migration stops when a row carries this member"
// cannot be written without naming the member -- the pre-0035 CHECK admits only
// the original five, so no stand-in value can reach the guard at all.
//
// Same removal trigger as the members themselves: the next breaking contracts
// bump deletes both members, this exemption and that test together.
const DEPRECATION_GUARD_FILE = "edge/database/src/migrations.rs";

function isExempt(rel, line) {
  if (!DEPRECATED_MEMBER_LINE.test(line)) return false;
  const path = rel.split(sep).join("/");
  return path.startsWith(EXEMPT_ROOT) || path === DEPRECATION_GUARD_FILE;
}
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
    if (isExempt(rel, line)) return;
    for (const pattern of patterns) {
      const m = line.match(pattern);
      if (m) {
        violations.push({ file: rel, line: i + 1, token: m[1], text: line.trim().slice(0, 120) });
        break;
      }
    }
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
