// T10 (Milestone 2) interop harness: spawns the REAL edge/device WebSocket
// server (compiled from tests/integration/kds-lan-bridge, which links
// holler-edge-device and holler-edge-database directly — no reimplementation
// of either) as a child process, and hands the driving test the ids/port it
// needs to build a real handshake URL with the REAL apps/kds
// `buildConnectionUrl`.
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import path from "node:path";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const BRIDGE_MANIFEST = path.resolve(__dirname, "../kds-lan-bridge/Cargo.toml");

export interface BridgeInfo {
  port: number;
  outlet_id: string;
  kds_device_id: string;
  /** `<credential_id>.<secret>` for the one KDS credential the bridge seeds
   * — edge/device rejects any connection whose first frame is not a
   * verifiable device_token (ADR-017 amendment). */
  kds_device_token: string;
  kot_id: string;
  order_id: string;
}

export interface BridgeHandle {
  info: BridgeInfo;
  /** Base ws:// URL, with no identity query params — the same shape
   * `LanConfig.url` expects, so callers pass it straight into
   * `loadLanConfigFromEnv`/a hand-built `LanConfig` without editing it. */
  wsUrl: string;
  httpBase: string;
  stop: () => Promise<void>;
}

/** Builds (if needed) and starts `kds-lan-bridge`, waits for its one-line
 * JSON ready message on stdout, and returns everything a test needs to talk
 * to the real server it just started. `cargo run` (not a pre-built path) so
 * this stays correct if the crate changes — a stale binary silently testing
 * the wrong server would be worse than the extra seconds. */
export async function startBridge(): Promise<BridgeHandle> {
  const child: ChildProcessWithoutNullStreams = spawn(
    "cargo",
    ["run", "--quiet", "--manifest-path", BRIDGE_MANIFEST],
    { stdio: ["pipe", "pipe", "inherit"] },
  );

  const info = await new Promise<BridgeInfo>((resolve, reject) => {
    const rl = createInterface({ input: child.stdout });
    const timer = setTimeout(() => {
      rl.close();
      reject(new Error("kds-lan-bridge did not print its ready line within 90s"));
    }, 90_000);

    rl.on("line", (line) => {
      const trimmed = line.trim();
      if (!trimmed.startsWith("{")) return; // tolerate stray non-JSON stdout noise
      try {
        const parsed = JSON.parse(trimmed) as BridgeInfo;
        clearTimeout(timer);
        rl.close();
        resolve(parsed);
      } catch {
        // Not our line; keep waiting.
      }
    });

    child.once("exit", (code, signal) => {
      clearTimeout(timer);
      reject(new Error(`kds-lan-bridge exited before printing its ready line (code=${code} signal=${signal})`));
    });
    child.once("error", (err) => {
      clearTimeout(timer);
      reject(err);
    });
  });

  const stop = (): Promise<void> =>
    new Promise((resolve) => {
      if (child.exitCode !== null || child.signalCode !== null) {
        resolve();
        return;
      }
      const onExit = () => resolve();
      child.once("exit", onExit);
      // The bridge treats any stdin line (or EOF) as "shut down cleanly".
      try {
        child.stdin.write("stop\n");
        child.stdin.end();
      } catch {
        // stdin already gone; fall through to the kill fallback below.
      }
      setTimeout(() => {
        if (child.exitCode === null && child.signalCode === null) {
          child.kill();
        }
      }, 5_000);
    });

  return {
    info,
    wsUrl: `ws://127.0.0.1:${info.port}/kds`,
    httpBase: `http://127.0.0.1:${info.port}`,
    stop,
  };
}
