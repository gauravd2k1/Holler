// CI entry point: a fixed-seed, reduced-count run (50 randomized scenarios
// + the 4 named regressions). The full 200+ randomized run is a manual
// command (`pnpm run:full`, docs/DEV_SETUP.md) — this test exists to keep
// the harness itself exercised and non-regressing on every push.
//
// Until M3 Track A, this asserted only harness-level fatals: two KNOWN
// product defects (docs/backlog-m2.md) made every invariant genuinely fail
// on some scenarios, and asserting 100% pass would have made the job
// permanently red for a reason unrelated to a real regression. That made
// this a smoke test for the harness, not a regression gate on the product —
// a NEW invariant violation would have passed CI silently, the exact
// failure mode the harness exists to end (docs/m3-planning.md, carried-
// forward gates).
//
// Track A closed the last invariant-level known defect (the mixed
// unrouted-line send). This now asserts every checked invariant outcome
// passed, for every scenario, in addition to the fatal-error check — a
// single new violation anywhere fails the job.
import { describe, expect, it } from "vitest";
import { runSuite } from "./run";
import { ALL_INVARIANTS, REQUIRED_SHAPES } from "./types";

const CI_SEED = 424242;
const CI_COUNT = 50;

describe("e2e-scenario-harness (CI reduced run)", () => {
  it(
    `runs ${CI_COUNT} fixed-seed randomized scenarios plus the named regressions with zero fatal errors and zero invariant violations`,
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
      // process crash, protocol desync, uncaught exception.
      expect(fatal.length).toBe(0);
      expect(summary.results.length).toBe(CI_COUNT + 4);
      // See scratch/run report (`summary.reportPath`, printed above by
      // `run.ts`) for the full replayable action sequence behind any of
      // these.
      console.log(`Full run report: ${summary.reportPath}`);

      // Per-invariant pass counts, not just fatals: every invariant a
      // scenario actually checked must have passed. `checked` can
      // legitimately vary per scenario (an invariant like durability only
      // fires on scenarios that happened to crash), but a `checked && !
      // passed` outcome anywhere is a real product/regression violation,
      // never something this suite excuses.
      for (const inv of ALL_INVARIANTS) {
        const failures = summary.results.filter((r) => r.invariants[inv].checked && !r.invariants[inv].passed);
        if (failures.length > 0) {
          console.error(
            `Invariant ${inv} failed on ${failures.length} scenario(s):`,
            failures.map((r) => `${r.name} (seed ${r.seed}): ${r.invariants[inv].detail ?? "(no detail)"}`),
          );
        }
        expect(failures.length, `invariant ${inv} must pass on every scenario that checked it`).toBe(0);
      }

      // Green-on-absent-data guard. Every invariant above can pass while its
      // subject never occurred: invariants 9/10 passed on 54/54 scenarios
      // for three tracks while every invoice carried a zero discount, every
      // bill was a single part, and no bill was ever queued to a printer.
      // A run that produced none of those proved nothing about them, so the
      // shape counts are asserted as hard as the invariants are.
      console.log("Shapes produced this run:", summary.shapeCounts);
      expect(
        summary.missingShapes,
        `these data shapes never occurred in the run, so the invariants covering them are green on absent data: ${summary.missingShapes.join(", ")}`,
      ).toEqual([]);
      for (const shape of REQUIRED_SHAPES) {
        expect(summary.shapeCounts[shape], `shape ${shape} must occur at least once`).toBeGreaterThan(0);
      }
    },
  );
});
