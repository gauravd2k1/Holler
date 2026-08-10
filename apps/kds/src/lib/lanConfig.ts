// Connection configuration for the LAN hop to `edge/device` (ADR-014 §6).
// No hard-coded URL: the endpoint is supplied by the deploy-time environment
// (a Vite env var here; a PWA installed per-outlet reads its own config), so
// the same build works against whatever host/port the edge node is running
// on for that outlet.

export interface LanConfig {
  /** ws:// or wss:// URL of the edge node's KDS endpoint. */
  url: string;
  /** Identifies this screen to the edge for the audit trail
   * (`KdsLanCommand.device_id`) — the edge does not trust it for
   * authorization, only for "which screen asked". */
  deviceId: string;
  /** No heartbeat received within this window: connection is considered
   * stale (still "connected" at the socket level, but the data on screen is
   * no longer trustworthy without saying so). Configurable, not a magic
   * number in the component. */
  heartbeatTimeoutMs: number;
  /** Delay before an automatic reconnect attempt after a close/error. */
  reconnectDelayMs: number;
  /** How long a requested transition may sit unconfirmed before the UI
   * surfaces "not confirmed" instead of silently waiting forever. */
  transitionTimeoutMs: number;
}

const DEFAULT_HEARTBEAT_TIMEOUT_MS = 15_000;
const DEFAULT_RECONNECT_DELAY_MS = 2_000;
const DEFAULT_TRANSITION_TIMEOUT_MS = 8_000;

/** Reads `import.meta.env.VITE_KDS_LAN_URL` / `VITE_KDS_DEVICE_ID`. Throws
 * rather than falling back to a hard-coded host — an unconfigured KDS must
 * fail loudly at startup, not silently point at localhost. */
export function loadLanConfigFromEnv(env: Record<string, string | undefined>): LanConfig {
  const url = env.VITE_KDS_LAN_URL;
  const deviceId = env.VITE_KDS_DEVICE_ID;
  if (!url) {
    throw new Error("VITE_KDS_LAN_URL is not configured — cannot connect to the edge node.");
  }
  if (!deviceId) {
    throw new Error("VITE_KDS_DEVICE_ID is not configured — this screen has no device identity.");
  }
  return {
    url,
    deviceId,
    heartbeatTimeoutMs: DEFAULT_HEARTBEAT_TIMEOUT_MS,
    reconnectDelayMs: DEFAULT_RECONNECT_DELAY_MS,
    transitionTimeoutMs: DEFAULT_TRANSITION_TIMEOUT_MS,
  };
}
