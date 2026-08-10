// Transport for the LAN hop to `edge/device` (ADR-014 §6). Wraps a WebSocket
// behind a small interface so tests can drive it with a fake instead of a
// real socket/server — this module must not block on T3's edge server
// existing.
import { KdsLanMessageSchema, type KdsLanCommand, type KdsLanMessage } from "@holler/contracts";
import { buildConnectionUrl, type LanConfig } from "./lanConfig";

/** The subset of the `WebSocket` API this client needs. Real `WebSocket`
 * satisfies this structurally; tests supply a fake. */
export interface WebSocketLike {
  send(data: string): void;
  close(): void;
  onopen: (() => void) | null;
  onclose: (() => void) | null;
  onerror: ((ev: unknown) => void) | null;
  onmessage: ((ev: { data: string }) => void) | null;
}

export type WebSocketFactory = (url: string) => WebSocketLike;

export interface LanClientHandlers {
  onMessage: (message: KdsLanMessage) => void;
  onConnectionStatusChange: (status: "connecting" | "connected" | "disconnected") => void;
  /** Invoked for a socket payload that fails `KdsLanMessageSchema` — the
   * wire contract was violated, which must be visible, not swallowed. */
  onInvalidMessage: (raw: unknown, error: unknown) => void;
}

/** Manages one logical connection to the edge node: connects, reconnects on
 * close/error after `config.reconnectDelayMs`, and forwards parsed messages.
 * Never trusts inbound data without validating it against
 * `KdsLanMessageSchema` first (untrusted input crossing a process boundary,
 * same rule as the POS's Tauri boundary). */
export class LanClient {
  private socket: WebSocketLike | null = null;
  private closedByCaller = false;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(
    private readonly config: LanConfig,
    private readonly handlers: LanClientHandlers,
    private readonly createSocket: WebSocketFactory,
  ) {}

  connect(): void {
    this.closedByCaller = false;
    this.handlers.onConnectionStatusChange("connecting");
    // Identity comes from the handshake, not from a payload field — see
    // lanConfig.ts's transport note (ADR-014 §6).
    const socket = this.createSocket(buildConnectionUrl(this.config));
    this.socket = socket;

    socket.onopen = () => {
      this.handlers.onConnectionStatusChange("connected");
    };

    socket.onmessage = (ev) => {
      let raw: unknown;
      try {
        raw = JSON.parse(ev.data);
      } catch (err) {
        this.handlers.onInvalidMessage(ev.data, err);
        return;
      }
      const result = KdsLanMessageSchema.safeParse(raw);
      if (!result.success) {
        this.handlers.onInvalidMessage(raw, result.error);
        return;
      }
      this.handlers.onMessage(result.data);
    };

    socket.onerror = () => {
      // Handled uniformly via onclose, which every browser WebSocket fires
      // after onerror.
    };

    socket.onclose = () => {
      this.handlers.onConnectionStatusChange("disconnected");
      this.scheduleReconnect();
    };
  }

  private scheduleReconnect(): void {
    if (this.closedByCaller) return;
    if (this.reconnectTimer !== null) return;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      if (!this.closedByCaller) this.connect();
    }, this.config.reconnectDelayMs);
  }

  /** Sends KOT status intent. The screen does not update local state here —
   * it waits for the edge's `kot_upserted` (or a timeout, surfaced by the
   * caller via the pending-transition tracking in `kdsStore`). */
  sendCommand(command: KdsLanCommand): void {
    if (!this.socket) return;
    this.socket.send(JSON.stringify(command));
  }

  disconnect(): void {
    this.closedByCaller = true;
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.socket?.close();
    this.socket = null;
  }
}
