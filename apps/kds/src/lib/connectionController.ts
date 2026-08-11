// Wires `LanClient` to `kdsStore`: applies inbound messages, and turns
// "no heartbeat for a while" and "no confirmation of a requested transition"
// into visible state rather than silence (task requirements #4 and #2).
import type { KdsLanMessage, KotStatus } from "@holler/contracts";
import type { LanConfig } from "./lanConfig";
import { LanClient, type WebSocketFactory } from "./lanClient";
import type { useKdsStore } from "../store/kdsStore";

type KdsStore = ReturnType<typeof useKdsStore.getState>;
type Clock = () => number;

export interface ConnectionControllerDeps {
  config: LanConfig;
  createSocket: WebSocketFactory;
  store: {
    getState: () => KdsStore;
  };
  now?: Clock;
  setIntervalFn?: typeof setInterval;
  clearIntervalFn?: typeof clearInterval;
}

const HEARTBEAT_CHECK_INTERVAL_MS = 1_000;

export class ConnectionController {
  private readonly client: LanClient;
  private readonly store: ConnectionControllerDeps["store"];
  private readonly config: LanConfig;
  private readonly now: Clock;
  private readonly setIntervalFn: typeof setInterval;
  private readonly clearIntervalFn: typeof clearInterval;
  private tickTimer: ReturnType<typeof setInterval> | null = null;
  private socketConnected = false;

  constructor(deps: ConnectionControllerDeps) {
    this.config = deps.config;
    this.store = deps.store;
    this.now = deps.now ?? (() => Date.now());
    // `.bind(globalThis)` is load-bearing, not defensive style. Storing the
    // bare global on an instance field detaches it from its receiver, so
    // `this.setIntervalFn(...)` invokes it with the controller as `this`. Node
    // tolerates that — its timers are plain functions — but a browser's
    // `setInterval` is a WindowOrWorkerGlobalScope method and throws
    // "Illegal invocation" the moment `start()` runs. The Node-based socket
    // harness therefore passed while every real browser crashed on mount.
    this.setIntervalFn = deps.setIntervalFn ?? setInterval.bind(globalThis);
    this.clearIntervalFn = deps.clearIntervalFn ?? clearInterval.bind(globalThis);

    this.client = new LanClient(
      deps.config,
      {
        onConnectionStatusChange: (status) => this.handleSocketStatus(status),
        onMessage: (message) => this.handleMessage(message),
        onInvalidMessage: (raw, error) => {
          // A contract violation on the wire must be visible in
          // observability, not swallowed — logged, not thrown, so one bad
          // frame does not take the whole screen down. (No eslint-disable
          // needed here: this app's minimal eslint.config.js does not enable
          // `no-console` — see that file for why the rule set is deliberately
          // narrow.)
          console.error("KDS LAN: received message that failed KdsLanMessageSchema", {
            raw,
            error,
          });
        },
      },
      deps.createSocket,
    );
  }

  start(): void {
    this.client.connect();
    this.tickTimer = this.setIntervalFn(() => this.tick(), HEARTBEAT_CHECK_INTERVAL_MS);
  }

  stop(): void {
    this.client.disconnect();
    if (this.tickTimer !== null) {
      this.clearIntervalFn(this.tickTimer);
      this.tickTimer = null;
    }
  }

  requestStatusChange(kotId: string, status: KotStatus): void {
    const atMs = this.now();
    this.client.sendCommand({
      type: "set_kot_status",
      kot_id: kotId,
      status,
      device_id: this.config.deviceId,
      requested_at: new Date(atMs).toISOString(),
    });
    this.store.getState().beginPendingTransition(kotId, status, atMs);
  }

  private handleSocketStatus(status: "connecting" | "connected" | "disconnected"): void {
    this.socketConnected = status === "connected";
    if (status === "disconnected") {
      // A fresh connect must not show tickets left over from before the
      // drop — a KDS showing stale tickets and one showing nothing look
      // identical to a cook, so this must say "disconnected", not keep
      // rendering the last-known set.
      this.store.getState().clearAll();
    }
    this.store.getState().setConnectionStatus(status === "connected" ? "connected" : status);
  }

  private handleMessage(message: KdsLanMessage): void {
    const state = this.store.getState();
    state.noteMessageReceived(this.now());
    switch (message.type) {
      case "snapshot":
        state.applySnapshot(message.kots);
        break;
      case "kot_upserted":
        state.upsertKot(message.kot);
        break;
      case "kot_removed":
        state.removeKot(message.kot_id);
        break;
      case "heartbeat":
        // noteMessageReceived above is all a heartbeat needs to do.
        break;
    }
    // Any inbound message re-confirms liveness even if it isn't a
    // heartbeat, but only a genuinely connected socket counts.
    if (this.socketConnected) {
      state.setConnectionStatus("connected");
    }
  }

  private tick(): void {
    const state = this.store.getState();
    const nowMs = this.now();

    if (this.socketConnected && state.lastMessageAt !== null) {
      const silentFor = nowMs - state.lastMessageAt;
      if (silentFor >= this.config.heartbeatTimeoutMs) {
        state.setConnectionStatus("stale");
      }
    }

    for (const [kotId, pending] of Object.entries(state.pendingByKotId)) {
      if (!pending.timedOut && nowMs - pending.requestedAt >= this.config.transitionTimeoutMs) {
        state.timeoutPendingTransition(kotId);
      }
    }
  }
}
