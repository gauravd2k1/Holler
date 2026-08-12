// Manual full-run entry point (docs/DEV_SETUP.md). CI runs a fixed reduced
// count via scenario.test.ts instead — see that file and README.md.
import { runSuite, REPORT_PATH } from "./run";

function argValue(flag: string, fallback: string): string {
  const idx = process.argv.indexOf(flag);
  if (idx === -1 || idx === process.argv.length - 1) return fallback;
  return process.argv[idx + 1];
}

async function main() {
  const seed = Number(argValue("--seed", String(Date.now() >>> 0)));
  const count = Number(argValue("--count", "200"));
  console.log(`e2e-scenario-harness: running ${count} randomized scenarios (+ named regressions), base seed ${seed}`);
  const summary = await runSuite(seed, count);
  console.log(`Done. ${summary.results.length} scenarios, ${summary.fatalCount} fatal errors. Report: ${REPORT_PATH}`);
  if (summary.fatalCount > 0) process.exitCode = 1;
}

main().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
