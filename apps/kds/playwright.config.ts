import { defineConfig, devices } from "@playwright/test";

// Real-browser smoke test for the KDS (T13, docs/retro.md 2026-08-11: "The
// KDS crashed in every browser while every test passed"). Every other check
// for this app runs under Node — vitest/jsdom, and the kds-lan integration
// suite drives the real client modules from a Node process too. Neither can
// observe a browser-only failure like a detached global builtin. This is the
// one check in the pipeline that actually launches a browser.
//
// The dev server is started with `--mode e2e`, which makes Vite load
// `.env.e2e` (tracked, non-secret — see that file). `tests/smoke.spec.ts`
// starts its own stub LAN WebSocket server on the port named there before
// the browser navigates.
export default defineConfig({
  testDir: "./tests",
  timeout: 30_000,
  fullyParallel: false,
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: "http://localhost:5199",
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    // Deliberately a different port from vite.config.ts's dev default
    // (5174) so this test never collides with a developer's own `pnpm dev`
    // running alongside it.
    command: "pnpm exec vite --mode e2e --port 5199 --strictPort",
    url: "http://localhost:5199",
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
