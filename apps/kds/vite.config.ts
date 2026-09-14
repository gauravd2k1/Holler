import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Milestone 2 KDS PWA. LAN-served over the outlet network — no internet
// dependency. See src/lib/lanConfig.ts for how the WebSocket endpoint is
// resolved (never hard-coded, CLAUDE.md §Coding rules).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  // BIND ALL INTERFACES, NOT LOOPBACK. A KDS is by definition a screen on
  // ANOTHER machine — a display by the pass, a spare laptop, a phone propped
  // against the till. Vite's default host is `localhost`, so until now the
  // only device that could open this page was the one serving it, which makes
  // the LAN-first design (docs/spec/kitchen.md) untestable and undemoable.
  // The WebSocket it then opens is a separate matter and already goes to
  // VITE_KDS_LAN_URL on the LAN address.
  //
  // The exposure is real and bounded: this serves the kitchen display to
  // anything that can reach the machine, so it belongs on the outlet's own
  // network or the demo hotspot, never on a cafe's guest WiFi. It is a DEV
  // and DEMO server either way — nothing here ships to an outlet (ADR-013).
  server: {
    host: true,
    port: 5174,
    strictPort: true,
  },
  // Same reason, for `vite preview` — which serves the PRODUCTION build and
  // is what a rehearsal should use, since a dev server is a different runtime
  // from the bundle that ships (CLAUDE.md: build-green is not dev-works).
  preview: {
    host: true,
    port: 5174,
    strictPort: true,
  },
  test: {
    environment: "jsdom",
    globals: false,
    setupFiles: [],
    // tests/ holds the Playwright browser smoke suite (T13), which has its
    // own runner (`pnpm test:e2e`) and must never run under vitest/jsdom —
    // the whole point of that suite is a real browser.
    exclude: ["**/node_modules/**", "tests/**"],
  },
});
