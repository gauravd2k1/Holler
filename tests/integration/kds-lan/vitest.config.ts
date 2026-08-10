import { defineConfig } from "vitest/config";

// T10: this suite spawns a real OS process (the compiled edge/device Rust
// server) and drives it over a real TCP/WebSocket connection, so it needs a
// Node environment (not jsdom — we want Node's own global WebSocket) and a
// generous timeout: the first run compiles the Rust harness binary, and
// even a warm run pays real process-start and socket round-trip cost, not
// simulated time.
export default defineConfig({
  test: {
    environment: "node",
    globals: false,
    testTimeout: 120_000,
    hookTimeout: 120_000,
    fileParallelism: false,
  },
});
