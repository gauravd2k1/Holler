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
  test: {
    environment: "jsdom",
    globals: false,
  },
});
