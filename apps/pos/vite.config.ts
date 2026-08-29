import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Milestone 1 POS frontend. Tauri serves this over http://localhost:5173 in
// dev (see src-tauri/tauri.conf.json devUrl) and bundles the `dist/` build
// for production (frontendDist).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  test: {
    environment: "jsdom",
    globals: false,
    setupFiles: [],
    // `tests/` holds the Playwright dev-server smoke test, which drives a
    // real browser against `pnpm dev` and cannot run under vitest/jsdom.
    // Excluded here rather than renamed so it keeps the `.spec.ts` name every
    // Playwright convention expects — and so the two runners stay visibly
    // distinct: vitest covers pure logic, `pnpm smoke` covers the one runtime
    // vitest structurally cannot observe.
    exclude: ["node_modules/**", "tests/**"],
  },
});
