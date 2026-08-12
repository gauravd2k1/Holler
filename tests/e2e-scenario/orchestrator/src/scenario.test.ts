// CI entry point: a fixed-seed, reduced-count run (50 randomized scenarios
// + the 4 named regressions). The full 200+ randomized run is a manual
// command (`pnpm run:full`, docs/DEV_SETUP.md) — this test exists to keep
// the harness itself exercised and non-regressing on every push, not to
// gate on every invariant passing: some invariant failures here are KNOWN,
// already-diagnosed product defects (see tests/e2e-scenario/REPORT.md's
// Findings section after a run) that this track exists to surface, not to
// fix. Asserting 100% invariant pass would make this test permanently red
// for a reason that has nothing to do with a regression in the harness.
import { describe, expect, it } from "vitest";
import { runSuite } from "./run";

const CI_SEED = 424242;
const CI_COUNT = 50;

describe("e2e-scenario-harness (CI reduced run)", () => {
  it(
    `runs ${CI_COUNT} fixed-seed randomized scenarios plus the named regressions without a harness-level fatal error`,
    async () => {
      const summary = await runSuite(CI_SEED, CI_COUNT);
      const fatal = summary.results.filter((r) => r.fatalError);
      if (fatal.length > 0) {
        console.error(
          "Fatal (harness-level, not invariant) errors:",
          fatal.map((r) => `${r.name} (seed ${r.seed}): ${r.fatalError}`),
        );
      }
      // A fatal error means the harness/bridge/KDS driver itself broke —
      // process crash, protocol desync, uncaught exception — as opposed to
      // an invariant catching a real product defect, which is expected and
      // is reported, not asserted away here.
      expect(fatal.length).toBe(0);
      expect(summary.results.length).toBe(CI_COUNT + 4);
    },
  );
});
