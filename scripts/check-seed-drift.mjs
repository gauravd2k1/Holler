#!/usr/bin/env node
// seed/demo-outlet.json is GENERATED, never hand-edited (seed/README.md).
// This check regenerates it from the authoring source
// (edge/database/src/bin/devseed.rs --emit-json) and fails the build if the
// committed file differs — the same "a claim in a comment is worth nothing
// unless something fails when it goes false" discipline seed/README.md's own
// header states as its reason for existing.
//
// WHAT THIS DOES NOT COVER, stated so the guarantee is not overread:
//   - It proves the COMMITTED FILE matches what the Rust seed structs would
//     produce right now. It does NOT prove either reader (edge devseed's own
//     non-emit seeding path, or backend/cmd/devseed/seedfile.go) actually
//     consumes every field correctly — that is exercised by
//     `cargo test --bin devseed` (edge) and the Go seed-file tests (cloud),
//     not by this script.
//   - It does NOT check the file's key order against seed/README.md's "File
//     format" table. build_shared_catalogue()'s own doc comment records why:
//     serde_json::Value sorts keys alphabetically without the
//     `preserve_order` feature, which is out of this repo's currently
//     enabled feature set. A committed file with the "wrong" key order but
//     otherwise byte-identical content is not something this check can see,
//     and is not a defect this check exists to catch — re-emission being
//     byte-stable is the actual guarantee it depends on.
//   - It does NOT run backend/cmd/devseed against the file, so a field this
//     repo's Go reader would reject (DisallowUnknownFields, a blank
//     required string, …) is not caught here either.

import { execFileSync } from "node:child_process";
import { readFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const EDGE_DB_DIR = join(repoRoot, "edge", "database");
const COMMITTED = join(repoRoot, "seed", "demo-outlet.json");

const fail = (message) => {
  console.error(`check-seed-drift: ${message}`);
  process.exit(1);
};

let committed;
try {
  committed = readFileSync(COMMITTED, "utf8");
} catch {
  fail(
    `cannot read ${COMMITTED} — run ` +
      `\`cargo run --bin devseed -- --emit-json seed/demo-outlet.json\` ` +
      `(from edge/database) and commit the result first.`,
  );
}

const scratch = mkdtempSync(join(tmpdir(), "holler-seed-drift-"));
const regenerated = join(scratch, "demo-outlet.json");

try {
  execFileSync(
    "cargo",
    ["run", "--quiet", "--bin", "devseed", "--", "--emit-json", regenerated],
    { cwd: EDGE_DB_DIR, stdio: ["ignore", "ignore", "inherit"] },
  );
} catch (e) {
  fail(`regenerating the catalogue failed: ${e.message}`);
} finally {
  // Nothing to clean up here — the regenerated file is read below and the
  // whole scratch dir is removed at the end regardless of outcome.
}

let regeneratedText;
try {
  regeneratedText = readFileSync(regenerated, "utf8");
} catch (e) {
  rmSync(scratch, { recursive: true, force: true });
  fail(`devseed --emit-json did not produce ${regenerated}: ${e.message}`);
}

rmSync(scratch, { recursive: true, force: true });

if (regeneratedText !== committed) {
  fail(
    "seed/demo-outlet.json is STALE — it does not match what " +
      "edge/database/src/bin/devseed.rs --emit-json produces right now.\n" +
      "  seed/demo-outlet.json is GENERATED, never hand-edited (seed/README.md).\n" +
      "  Edit the seed data in edge/database/src/bin/devseed.rs, then re-run:\n" +
      "    cd edge/database && cargo run --bin devseed -- --emit-json ../../seed/demo-outlet.json\n" +
      "  and commit both files together.",
  );
}

console.log(
  `check-seed-drift: ok — seed/demo-outlet.json matches the regenerated catalogue (${committed.length} bytes)`,
);
