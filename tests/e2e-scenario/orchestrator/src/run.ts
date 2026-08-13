import path from "node:path";
import fs from "node:fs";
import os from "node:os";
import { HarnessBridge } from "./bridge";
import { runScenario, type ScenarioOptions } from "./runner";
import { writeReport } from "./report";
import type { ScenarioResult } from "./types";

// A run report is a scratch artifact, not a tracked file — it used to land
// at tests/e2e-scenario/REPORT.md (inside the repo, so every run left an
// uncommitted diff or, worse, a stray tracked file). It now writes under
// the OS temp directory instead, one file per run so concurrent/repeated
// runs never clobber each other. Override with `HOLLER_E2E_REPORT_DIR` (CI
// sets this to upload the file as a build artifact instead of relying on
// the ephemeral runner's own temp dir).
function reportPath(seedBase: number): string {
  const dir = process.env.HOLLER_E2E_REPORT_DIR
    ? path.resolve(process.env.HOLLER_E2E_REPORT_DIR)
    : path.join(os.tmpdir(), "holler-e2e-scenario-reports");
  fs.mkdirSync(dir, { recursive: true });
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  return path.join(dir, `REPORT-${seedBase}-${stamp}.md`);
}

/** Named regression cases for every bug found the week this track was
 * commissioned (docs/retro.md). Each replays the exact shape of the bug
 * using the same real command surface every randomized scenario uses. */
export function namedRegressions(seedBase: number): ScenarioOptions[] {
  return [
    // The P0: DRAFT order locks order_type/table_id at the first tapped
    // item. Regression: after the fix (update_order_shape), a cart step
    // that changes type/table must succeed, and a DINE_IN order with no
    // table set at creation must become sendable once a table is set.
    { name: "shape-lock-after-first-tap", seed: seedBase + 9001, maxCartSteps: 3 },
    // zero-station-item-send: M3 Track A regression guard. A mixed order
    // (routable + unrouted lines) used to send silently, dropping the
    // unrouted line with no signal (docs/backlog-m2.md). Forced here via a
    // fixed seed known (from this file's own earlier manual runs) to pick
    // the no-station fixture early alongside a routable item, so the case
    // is deterministic rather than merely probable — asserts invariant 4's
    // UNROUTED_KITCHEN_ITEMS rejection, not the old silent-success shape.
    { name: "zero-station-item-send", seed: seedBase + 9002, maxCartSteps: 2 },
    // double-send-already-sent: default flow already sends once then
    // again; naming it explicitly here pins it as a standing regression
    // case independent of the random cart-step count.
    { name: "double-send-already-sent", seed: seedBase + 9003, maxCartSteps: 0 },
    // stuck-draft-rescue: forces a mid-draft crash, then continues —
    // proves a DRAFT order recovered after a crash is still shape-editable
    // and still reachable by get_active_draft_order's equivalent path
    // (introspection here, since the harness drives *_impl directly).
    { name: "stuck-draft-rescue", seed: seedBase + 9004, forceCrash: "mid-draft", maxCartSteps: 1 },
  ];
}

export interface RunSummary {
  results: ScenarioResult[];
  fatalCount: number;
  reportPath: string;
}

export async function runSuite(seedBase: number, randomizedCount: number, includeNamed = true): Promise<RunSummary> {
  const bridge = await HarnessBridge.start(String(seedBase));
  const results: ScenarioResult[] = [];
  try {
    if (includeNamed) {
      for (const opts of namedRegressions(seedBase)) {
        results.push(await runScenario(bridge, opts));
      }
    }
    for (let i = 0; i < randomizedCount; i++) {
      const seed = seedBase + i;
      results.push(await runScenario(bridge, { name: `random-${i}`, seed }));
    }
  } finally {
    await bridge.stop();
  }
  const outPath = reportPath(seedBase);
  writeReport(outPath, seedBase, results);
  return { results, fatalCount: results.filter((r) => r.fatalError).length, reportPath: outPath };
}
