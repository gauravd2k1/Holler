/// <reference types="vitest" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// M6 Phase B back office. Unlike the POS and the KDS this is a CLOUD client:
// it talks to the Go backend over HTTP and has no edge, no SQLite and no LAN
// path. Nothing here runs at an outlet (ADR-013).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    // 5175: the POS dev server holds 5173 and the KDS holds 5174, and all
    // three are routinely up at once on this machine. strictPort so a
    // collision fails loudly instead of silently moving — a moved port is how
    // you end up testing yesterday's build.
    port: 5175,
    strictPort: true,
  },
  // @holler/contracts IS NEVER PREBUNDLED, AND THAT IS THE WHOLE POINT.
  //
  // It is a `file:` workspace dependency whose SOURCE changes while we work on
  // it. Vite's dep optimizer copies it into node_modules/.vite/deps once and
  // then serves that copy, so adding an export to packages/contracts and
  // importing it here resolves to nothing -- the module fails to load, React
  // never mounts, and the page is BLANK WITH NO ERROR ANYWHERE THE BUILD CAN
  // SEE. `vite build` cannot catch it either: optimizeDeps is a dev-server
  // mechanism the build never reads, so `pnpm build` stays green.
  //
  // That is not hypothetical. It happened to the POS on 2026-08-20 and again
  // here on 2026-09-08, both times after adding a contracts export -- the
  // prebundle was 6.5 hours older than the source. Both were fixed by deleting
  // the cache, which fixes the instance and leaves the trap in place.
  //
  // Excluding it makes Vite serve it from source, so a contracts change is
  // picked up like any other edited file. apps/pos and apps/kds still carry
  // the original trap.
  optimizeDeps: {
    exclude: ["@holler/contracts"],
  },
  test: {
    environment: "jsdom",
    globals: false,
  },
});
