// Connection configuration for the LAN hop to `edge/device` (ADR-014 §6).
// No hard-coded URL: the endpoint is supplied by the deploy-time environment
// (a Vite env var here; a PWA installed per-outlet reads its own config), so
// the same build works against whatever host/port the edge node is running
// on for that outlet.
//
// Transport note (post-merge interop fix): T3's `edge/device` server takes
// connection identity only from handshake query params —
// `ws://host:port/kds?outlet_id=...&device_id=...[&station=...]` — and
// rejects a connection missing `outlet_id`/`device_id` with HTTP 400 before
// any frame moves. This matches ADR-014 §6: identity comes from the
// connection, not from a payload field a client could set to anything.

export interface LanConfig {
  /** Base ws:// or wss:// URL of the edge node's KDS endpoint, without the
   * identity query params — those are appended by `buildConnectionUrl`. */
  url: string;
  /** This outlet, required by the edge handshake. */
  outletId: string;
  /** Identifies this screen to the edge — used both for the handshake query
   * param the edge authenticates against, and for the audit trail inside
   * `KdsLanCommand.device_id`. The same config value drives both so the two
   * cannot silently disagree. */
  deviceId: string;
  /** Optional station filter (e.g. `TANDOOR`) so a screen only receives
   * tickets for its station. Omitted entirely from the handshake when unset
   * — the edge then sends everything. */
  station: string | null;
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

/** Reads `import.meta.env.VITE_KDS_LAN_URL` / `VITE_KDS_OUTLET_ID` /
 * `VITE_KDS_DEVICE_ID` (and optional `VITE_KDS_STATION`). Throws rather than
 * falling back to a hard-coded host or a guessed identity — an unconfigured
 * KDS must fail loudly at startup, not silently point at localhost or an
 * empty outlet/device id that the edge would 400 on anyway. */
export function loadLanConfigFromEnv(env: Record<string, string | undefined>): LanConfig {
  const url = env.VITE_KDS_LAN_URL;
  const outletId = env.VITE_KDS_OUTLET_ID;
  const deviceId = env.VITE_KDS_DEVICE_ID;
  const station = env.VITE_KDS_STATION;
  if (!url) {
    throw new Error("VITE_KDS_LAN_URL is not configured — cannot connect to the edge node.");
  }
  if (!outletId) {
    throw new Error("VITE_KDS_OUTLET_ID is not configured — this screen has no outlet identity.");
  }
  if (!deviceId) {
    throw new Error("VITE_KDS_DEVICE_ID is not configured — this screen has no device identity.");
  }
  return {
    url,
    outletId,
    deviceId,
    station: station && station.length > 0 ? station : null,
    heartbeatTimeoutMs: DEFAULT_HEARTBEAT_TIMEOUT_MS,
    reconnectDelayMs: DEFAULT_RECONNECT_DELAY_MS,
    transitionTimeoutMs: DEFAULT_TRANSITION_TIMEOUT_MS,
  };
}

/** Builds the full handshake URL by appending `outlet_id`, `device_id` and
 * (when set) `station` to `config.url`. Uses the `URL` API rather than
 * string concatenation so a base URL that already carries a query string,
 * or a trailing slash, does not produce something malformed. */
export function buildConnectionUrl(config: LanConfig): string {
  const parsed = new URL(config.url);
  parsed.searchParams.set("outlet_id", config.outletId);
  parsed.searchParams.set("device_id", config.deviceId);
  if (config.station) {
    parsed.searchParams.set("station", config.station);
  }
  return parsed.toString();
}
