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
  },
});
