import { defineConfig } from "vitest/config";

// Same reasoning as tests/integration/kds-lan/vitest.config.ts: this suite
// spawns a real OS process (the compiled e2e-scenario-harness Rust bridge)
// and drives it, and a real edge/device WebSocket server it starts, over
// genuine TCP — Node environment (not jsdom, so Node's own global
// WebSocket is used), generous timeouts, and no file parallelism (each run
// owns one harness child process and one scratch directory).
export default defineConfig({
  test: {
    environment: "node",
    globals: false,
    testTimeout: 300_000,
    hookTimeout: 300_000,
    fileParallelism: false,
  },
});
