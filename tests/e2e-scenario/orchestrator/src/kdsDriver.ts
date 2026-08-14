// Drives the REAL apps/kds client modules over a genuine WebSocket —
// exactly the tests/integration/kds-lan pattern, reused rather than
// reimplemented. This file imports apps/kds's published module surface; it
// is not our directory to edit.
import { buildConnectionUrl, type LanConfig } from "../../../../apps/kds/src/lib/lanConfig";
import { ConnectionController } from "../../../../apps/kds/src/lib/connectionController";
import { useKdsStore } from "../../../../apps/kds/src/store/kdsStore";
import type { ScenarioInfo } from "./bridge";

// Local mirror of packages/contracts' KotStatusSchema literals — avoided
// importing "@holler/contracts" directly from this file so this test-only
// package does not need its own dependency/resolution setup for it; the
// real apps/kds modules imported below still validate every wire value
// against the real Zod schema internally.
export type KotStatus = "NEW" | "ACKNOWLEDGED" | "PREPARING" | "READY" | "SERVED" | "CANCELLED";

function createRealSocket(url: string) {
  return new WebSocket(url) as unknown as import("../../../../apps/kds/src/lib/lanClient").WebSocketLike;
}

export function resetKdsStore(): void {
  useKdsStore.setState({
    kots: {},
    connectionStatus: "connecting",
    lastMessageAt: null,
    pendingByKotId: {},
  });
}

export function waitFor(predicate: () => boolean, timeoutMs = 5_000, intervalMs = 10): Promise<number> {
  const start = Date.now();
  return new Promise((resolve, reject) => {
    const tick = () => {
      if (predicate()) {
        resolve(Date.now() - start);
        return;
      }
      if (Date.now() - start > timeoutMs) {
        reject(new Error(`timed out after ${timeoutMs}ms waiting for condition`));
        return;
      }
      setTimeout(tick, intervalMs);
    };
    tick();
  });
}

/** One KDS screen against one scenario's edge/device server, driven with
 * the real `ConnectionController`/`useKdsStore`. `useKdsStore` is a
 * module-level singleton (Zustand) — this driver is deliberately
 * single-instance per scenario to avoid two concurrent controllers racing
 * one store; see README "Known limitations" for what that leaves
 * unexercised (independent multi-screen fan-out). */
export class KdsDriver {
  private controller: ConnectionController | null = null;
  private info: ScenarioInfo;

  constructor(info: ScenarioInfo) {
    this.info = info;
  }

  private config(overrides: Partial<LanConfig> = {}): LanConfig {
    return {
      url: `ws://127.0.0.1:${this.info.port}/kds`,
      outletId: this.info.outlet_id,
      deviceId: this.info.kds_device_id,
      deviceToken: this.info.kds_device_token,
      station: null,
      heartbeatTimeoutMs: 10_000,
      reconnectDelayMs: 250,
      transitionTimeoutMs: 2_000,
      ...overrides,
    };
  }

  async connect(): Promise<void> {
    resetKdsStore();
    const expectedUrl = buildConnectionUrl(this.config());
    if (!expectedUrl.includes(`outlet_id=${this.info.outlet_id}`)) {
      throw new Error("buildConnectionUrl did not include outlet_id — client/server handshake drift");
    }
    this.controller = new ConnectionController({
      config: this.config(),
      createSocket: createRealSocket,
      store: useKdsStore,
    });
    this.controller.start();
    await waitFor(() => useKdsStore.getState().connectionStatus === "connected", 10_000);
  }

  disconnect(): void {
    this.controller?.stop();
    this.controller = null;
  }

  async reconnect(): Promise<void> {
    this.disconnect();
    await this.connect();
  }

  requestStatusChange(kotId: string, status: KotStatus): void {
    if (!this.controller) throw new Error("KDS not connected");
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    this.controller.requestStatusChange(kotId, status as any);
  }

  state() {
    return useKdsStore.getState();
  }

  /** Waits for `kotId` to appear (or disappear, for `expectAbsent`) in the
   * live store, returning the latency in ms — invariant 3's raw data
   * point. */
  async waitForKot(kotId: string, timeoutMs = 2_000): Promise<number> {
    return waitFor(() => useKdsStore.getState().kots[kotId] !== undefined, timeoutMs);
  }

  async waitForStatus(kotId: string, status: string, timeoutMs = 2_000): Promise<number> {
    return waitFor(() => useKdsStore.getState().kots[kotId]?.status === status, timeoutMs);
  }

  async waitForRemoved(kotId: string, timeoutMs = 2_000): Promise<number> {
    return waitFor(() => useKdsStore.getState().kots[kotId] === undefined, timeoutMs);
  }

  /** Invariant 8 (status echo): polls the REAL POS-side read command
   * (`list_kots` over the Rust bridge) until it reflects a KDS-driven
   * status change — proves the change actually landed in the shared
   * SQLite state the cashier's own screen reads, not merely that the KDS
   * store believes it. */
  async waitForStatusOnBridge(
    bridge: { request: <T>(payload: Record<string, unknown>) => Promise<T> },
    orderId: string,
    kotId: string,
    status: string,
    timeoutMs = 2_000,
  ): Promise<number> {
    const start = Date.now();
    for (;;) {
      const resp = await bridge.request<{ ok: boolean; kots?: { id: string; status: string }[] }>({
        op: "list_kots",
        order_id: orderId,
      });
      const kot = resp.kots?.find((k) => k.id === kotId);
      if (kot?.status === status) return Date.now() - start;
      if (Date.now() - start > timeoutMs) {
        throw new Error(`timed out after ${timeoutMs}ms waiting for POS-side echo of ${kotId} -> ${status}`);
      }
      await new Promise((r) => setTimeout(r, 25));
    }
  }

  /** Waits for a requested transition's pending marker to time out (the
   * expected outcome for an unconfirmable request — see
   * tests/integration/kds-lan test 4). */
  static async waitForPendingTimeout(driver: KdsDriver, kotId: string, timeoutMs: number): Promise<void> {
    await waitFor(() => driver.state().pendingByKotId[kotId]?.timedOut === true, timeoutMs);
  }
}
