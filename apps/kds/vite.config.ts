import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Milestone 2 KDS PWA. LAN-served over the outlet network — no internet
// dependency. See src/lib/lanConfig.ts for how the WebSocket endpoint is
// resolved (never hard-coded, CLAUDE.md §Coding rules).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5174,
    strictPort: true,
  },
  test: {
    environment: "jsdom",
    globals: false,
    setupFiles: [],
  },
});
