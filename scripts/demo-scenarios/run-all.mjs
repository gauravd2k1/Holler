/**
 * Run every scenario stage in order against whatever is up, then build the
 * sheet. Stages are independent and each records its own BLOCKED rows, so a
 * stage whose surface is down does not stop the ones after it.
 *
 * Usage:
 *   node scripts/demo-scenarios/run-all.mjs             run now
 *   node scripts/demo-scenarios/run-all.mjs --wait 900  sleep first, to let
 *                                                       the login rate-limit
 *                                                       window roll
 *
 * WHY THE WAIT EXISTS: backend/internal/auth/ratelimit.go allows five login
 * attempts per fifteen minutes per client IP. A suite that signs in, fails,
 * and immediately retries spends the budget and then reports a credential
 * fault — the misread CLAUDE.md records costing a debugging detour. Waiting is
 * the only way to clear it without restarting the backend, which this suite is
 * forbidden to do.
 */
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { sleep } from "./lib.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const STAGES = [
  "00-environment.mjs",
  "01-backend-enroll.mjs",
  "02-captain-api.mjs",
  "03-captain-ui.mjs",
  "04-kds-ui.mjs",
  "05-sync-replay.mjs",
  "06-admin-ui.mjs",
  "08-backoffice-api.mjs",
];

// 07-uplink-watch.mjs is NOT in that list. It samples for six minutes by
// design — the thing it measures is an interval, and a one-shot probe cannot
// see one. Run it alongside a full pass, with the till up:
//   node scripts/demo-scenarios/07-uplink-watch.mjs 12 30

const run = async () => {
  const waitIdx = process.argv.indexOf("--wait");
  if (waitIdx !== -1) {
    const secs = Number(process.argv[waitIdx + 1] ?? 900);
    console.log(`waiting ${secs}s for the login rate-limit window to roll…`);
    await sleep(secs * 1000);
  }

  for (const stage of STAGES) {
    console.log(`\n=== ${stage} ===`);
    const r = spawnSync(process.execPath, [join(HERE, stage)], { stdio: "inherit" });
    if (r.status !== 0) console.log(`(${stage} exited ${r.status} — its rows are recorded either way)`);
  }

  console.log("\n=== build-sheet.py ===");
  spawnSync("python", [join(HERE, "build-sheet.py")], { stdio: "inherit" });
};

run().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
