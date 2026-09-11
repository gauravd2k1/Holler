/// <reference types="vitest" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// docs/captain-api.md: this page is served by the till's own second HTTP
// listener (HOLLER_CAPTAIN_BIND_ADDR, default 0.0.0.0:9320), which serves the
// built `dist/` at `/` and the JSON API under `/api/`. A relative base keeps
// the built assets working regardless of which LAN address a phone reached
// the till on.
export default defineConfig({
  base: "./",
  plugins: [react()],
  clearScreen: false,
  server: {
    // 5176: POS holds 5173, KDS 5174, admin 5175.
    port: 5176,
    strictPort: true,
  },
  // @holler/contracts IS NEVER PREBUNDLED. It is a `file:` workspace
  // dependency whose source changes while we work on it; Vite's dep optimizer
  // otherwise copies it into node_modules/.vite/deps once and serves that
  // stale copy forever, which renders a blank page with no console error and
  // survives `pnpm build` (optimizeDeps is dev-server-only). Bit the POS and
  // apps/admin already; excluded here from day one.
  optimizeDeps: {
    exclude: ["@holler/contracts"],
  },
  test: {
    environment: "jsdom",
    globals: false,
  },
});
