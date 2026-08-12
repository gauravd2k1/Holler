import path from "node:path";
import { fileURLToPath } from "node:url";
import { HarnessBridge } from "./bridge";
import { runScenario, type ScenarioOptions } from "./runner";
import { writeReport } from "./report";
import type { ScenarioResult } from "./types";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
// tests/e2e-scenario/REPORT.md, per spec — one level above this package
// (tests/e2e-scenario/orchestrator), not inside it.
export const REPORT_PATH = path.resolve(__dirname, "../../REPORT.md");

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
    // zero-station-item-send: exercised whenever the randomized item pool
    // includes the no-station fixture — forced here via a fixed seed known
    // (from this file's own earlier manual runs) to pick it early, so the
    // regression is deterministic rather than merely probable.
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
  writeReport(REPORT_PATH, seedBase, results);
  return { results, fatalCount: results.filter((r) => r.fatalError).length };
}
