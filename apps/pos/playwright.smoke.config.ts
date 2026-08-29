import { defineConfig, devices } from "@playwright/test";

// ---------------------------------------------------------------------------
// THE POS DEV-SERVER SMOKE TEST
// ---------------------------------------------------------------------------
//
// This exists because THREE green checks cannot see the failures it catches.
// `tsc --noEmit`, `vitest` and `vite build` were all green through both POS
// white screens, and would be green through a third:
//
//   2026-08-20  POS white screen. A stale `node_modules/.vite` prebundle.
//               `optimizeDeps` is a DEV-SERVER mechanism that `vite build`
//               never reads, so the build structurally cannot fail on it.
//   2026-08-30  The same class again, during M5 T4: the prebundled copy of
//               `@holler/contracts` predated the procurement types, so the
//               browser would have thrown "does not provide an export named
//               'GrnGapReasonSchema'" while every suite stayed green.
//
// Two strikes on one mechanism, so this is no longer deferred.
//
// THREE PROPERTIES ARE LOAD-BEARING. Removing any one makes this file
// worthless for the defect it exists for:
//
// 1. IT DRIVES `pnpm dev`, NOT `vite build` + preview. A build-based smoke
//    test — including the headless run used to DIAGNOSE the 2026-08-20
//    incident — cannot catch this class at all, because the build never
//    consults the prebundle cache.
// 2. IT DOES NOT PASS `--force`. Forcing a re-optimise on every run is
//    precisely what hides a stale prebundle. This has to start the server the
//    way a developer starts it, or it cannot see what a developer sees.
// 3. IT ASSERTS ON CONSOLE AND PAGE ERRORS. The 2026-08-11 KDS crash was a
//    thrown `Illegal invocation` from a detached global builtin; the two POS
//    incidents were module-linking errors. Every one of them surfaces in the
//    browser console and NOWHERE else — not in a type, not in a unit test,
//    not in an exit code.
//
// Deliberately NOT a general E2E suite. The value is entirely in exercising
// the one runtime the other three checks cannot observe.
export default defineConfig({
  testDir: "./tests",
  testMatch: /dev-server-smoke\.spec\.ts/,
  timeout: 60_000,
  fullyParallel: false,
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: "http://localhost:5198",
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    // `pnpm dev` itself — the script a developer runs — with only a port
    // override so this never collides with a dev server already up on 5173.
    // NO `--force`: see property 2 above.
    command: "pnpm dev --port 5198 --strictPort",
    url: "http://localhost:5198",
    // Never reuse: a server someone already started may have been started
    // with `--force`, which is exactly the condition this test must not
    // inherit.
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
