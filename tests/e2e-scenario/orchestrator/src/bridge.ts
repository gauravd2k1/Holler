// Spawns the REAL e2e-scenario-harness Rust bridge (compiled from
// tests/e2e-scenario/harness, which links holler_pos_lib, holler-edge-
// database and holler-edge-device directly) and speaks its line-delimited
// JSON-RPC protocol over stdio. Extends tests/integration/kds-lan/bridge.ts's
// spawn/ready-line pattern; adds a synchronous request/response call for the
// richer protocol this track needs (POS actions, DB introspection), plus a
// crash-simulation primitive that force-kills and respawns the whole child
// process rather than trying to fake a crash in-process (see `crash()`'s
// doc comment for why: an in-process trick leaks the OS SQLite file handle
// and Windows then keeps the plaintext file locked against the very
// `Db::open` call meant to recover it — an artifact of the process still
// being alive, which a real crash never has).
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface, type Interface } from "node:readline";
import { fileURLToPath } from "node:url";
import path from "node:path";
import fs from "node:fs";
import os from "node:os";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const HARNESS_MANIFEST = process.env.HOLLER_E2E_FALSIFY_MANIFEST
  ? path.resolve(process.env.HOLLER_E2E_FALSIFY_MANIFEST)
  : path.resolve(__dirname, "../../harness/Cargo.toml");

export interface ModifierFixture {
  id: string;
  group_name: string;
  option_name: string;
  price_delta_paise: number;
}

export interface TemplateInfo {
  outlet_id: string;
  pos_device_id: string;
  kds_device_id: string;
  /** `<credential_id>.<secret>` — the connection's first frame (ADR-017
   * amendment, apps/kds/src/lib/lanConfig.ts's AUTHENTICATION note). */
  kds_device_token: string;
  cashier_user_id: string;
  stations: { single: string; multi_extra: string };
  tables: [string, string];
  items: {
    single_station: {
      id: string;
      unit_price_paise: number;
      variant_id: string;
      modifier_ids: string[];
      modifiers: ModifierFixture[];
    };
    single_station_2: { id: string; unit_price_paise: number };
    multi_station: { id: string; unit_price_paise: number };
    no_station: { id: string; unit_price_paise: number };
  };
  /** The outlet's seeded discount catalogue. `applies` is what the CASHIER
   * this harness bills as can actually use — the permission-gated one is
   * seeded precisely so a refusal can be asserted, not skipped. */
  discounts: {
    percent: DiscountFixture & { value_bps: number };
    permission_gated: DiscountFixture & { value_bps: number; required_permission: string };
    reason_gated: DiscountFixture & { value_paise: number };
  };
  printers: { bill: string; kitchen: string };
}

export interface DiscountFixture {
  id: string;
  code: string;
  method: "PERCENT" | "AMOUNT";
  requires_reason: boolean;
  requires_permission: boolean;
  applies: boolean;
}

export interface ScenarioInfo extends TemplateInfo {
  port: number;
  scenario_dir: string;
}

export class HarnessBridge {
  private child!: ChildProcessWithoutNullStreams;
  private rl!: Interface;
  private queue: Array<(line: string) => void> = [];
  private buffer: string[] = [];
  scratchDir: string;

  private constructor(scratchDir: string) {
    this.scratchDir = scratchDir;
  }

  static async start(seedTag: string): Promise<HarnessBridge> {
    const scratchDir = fs.mkdtempSync(path.join(os.tmpdir(), `holler-e2e-${seedTag}-`));
    const bridge = new HarnessBridge(scratchDir);
    await bridge.spawnAndAwaitReady();
    return bridge;
  }

  private spawnAndAwaitReady(): Promise<void> {
    const child: ChildProcessWithoutNullStreams = spawn(
      "cargo",
      ["run", "--quiet", "--manifest-path", HARNESS_MANIFEST],
      { stdio: ["pipe", "pipe", "inherit"], env: { ...process.env, HOLLER_E2E_DATA_DIR: this.scratchDir } },
    );
    this.child = child;
    this.queue = [];
    this.buffer = [];
    this.rl = createInterface({ input: child.stdout });
    this.rl.on("line", (line) => {
      const trimmed = line.trim();
      if (!trimmed.startsWith("{")) return; // devseed's own stdout noise etc.
      const next = this.queue.shift();
      if (next) next(trimmed);
      else this.buffer.push(trimmed);
    });
    return this.readOneLine(180_000).then((readyLine) => {
      const ready = JSON.parse(readyLine) as { ready?: boolean };
      if (!ready.ready) throw new Error(`harness did not send a ready line: ${readyLine}`);
    });
  }

  private readOneLine(timeoutMs: number): Promise<string> {
    if (this.buffer.length > 0) return Promise.resolve(this.buffer.shift() as string);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`harness did not respond within ${timeoutMs}ms`)), timeoutMs);
      this.queue.push((line) => {
        clearTimeout(timer);
        resolve(line);
      });
    });
  }

  /** One synchronous request/response round trip. The harness is single-
   * threaded over stdio (CLAUDE.md: no pipelining), so callers must not
   * issue a second `request` before the first resolves. */
  async request<T = unknown>(payload: Record<string, unknown>): Promise<T> {
    this.child.stdin.write(JSON.stringify(payload) + "\n");
    const line = await this.readOneLine(30_000);
    return JSON.parse(line) as T;
  }

  async newScenario(): Promise<ScenarioInfo> {
    return this.request<ScenarioInfo>({ op: "new_scenario" });
  }

  async closeScenario(): Promise<void> {
    await this.request({ op: "close_scenario" });
  }

  /** Crash simulation (invariant 6): force-kills the harness process with
   * NO chance to run its own graceful-shutdown path (SIGKILL, and on
   * Windows `child.kill()` maps to `TerminateProcess`, which the OS cannot
   * refuse) — exactly what a power cut does, leaving the current
   * scenario's plaintext SQLite file and its "unclean" marker on disk
   * (`edge/database/src/lib.rs::Db::open`'s own crash-leftover detection).
   * A fresh harness process is then spawned against the SAME
   * `HOLLER_E2E_DATA_DIR`, and asked to resume the named scenario — that
   * call runs the crate's real recovery path, not a simulation of it. */
  async crashAndResume(scenarioDir: string): Promise<ScenarioInfo> {
    this.child.kill("SIGKILL");
    await new Promise<void>((resolve) => {
      if (this.child.exitCode !== null || this.child.signalCode !== null) {
        resolve();
        return;
      }
      this.child.once("exit", () => resolve());
      // Belt-and-braces: SIGKILL should be immediate, but never hang the
      // suite if the OS is slow to report the exit.
      setTimeout(resolve, 5_000);
    });
    await this.spawnAndAwaitReady();
    return this.request<ScenarioInfo>({ op: "resume_scenario", dir: scenarioDir });
  }

  stop(): Promise<void> {
    return new Promise((resolve) => {
      const cleanup = () => {
        fs.rmSync(this.scratchDir, { recursive: true, force: true });
        resolve();
      };
      if (this.child.exitCode !== null || this.child.signalCode !== null) {
        cleanup();
        return;
      }
      this.child.once("exit", cleanup);
      try {
        this.child.stdin.write("stop\n");
        this.child.stdin.end();
      } catch {
        // stdin already gone.
      }
      setTimeout(() => {
        if (this.child.exitCode === null && this.child.signalCode === null) this.child.kill();
      }, 5_000);
    });
  }
}
