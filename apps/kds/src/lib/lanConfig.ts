// Connection configuration for the LAN hop to `edge/device` (ADR-014 §6).
// No hard-coded URL: the endpoint is supplied by the deploy-time environment
// (a Vite env var here; a PWA installed per-outlet reads its own config), so
// the same build works against whatever host/port the edge node is running
// on for that outlet.
//
// Transport note (post-merge interop fix): `edge/device`'s server takes
// connection IDENTITY only from handshake query params —
// `ws://host:port/kds?outlet_id=...&device_id=...[&station=...]` — and
// rejects a connection missing `outlet_id`/`device_id` with HTTP 400 before
// any frame moves. This matches ADR-014 §6: identity comes from the
// connection, not from a payload field a client could set to anything.
//
// AUTHENTICATION (ADR-017 hole 3, added post-M2): outlet_id/device_id are
// identity, not authentication — a UUID is not a secret. The edge now also
// requires this screen's enrolled `device_token` as the connection's FIRST
// WebSocket frame, `{"type":"auth","device_token":"<token>"}`, sent by
// `LanClient` immediately on `onopen`, before anything else. That shape is
// NOT in `lan.ts`'s `KdsLanCommandSchema`/`KdsLanMessageSchema` — this file
// is read-only to this track — and is a candidate for promotion into the
// frozen contract by the orchestrator. A header was the other option
// `lan.ts`'s transport note names, but a browser `WebSocket` cannot set
// custom headers on its handshake at all, which is why this app uses the
// first-frame message instead.

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
  /** This screen's enrolled device credential (`POST /devices/enroll`,
   * `<credential_id>.<secret>`), sent as the connection's first frame — see
   * the AUTHENTICATION note above. Never logged, never rendered, never
   * included in an error message this app constructs. */
  deviceToken: string;
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
 * `VITE_KDS_DEVICE_ID` / `VITE_KDS_DEVICE_TOKEN` (and optional
 * `VITE_KDS_STATION`). Throws rather than falling back to a hard-coded host
 * or a guessed identity — an unconfigured KDS must fail loudly at startup,
 * not silently point at localhost, an empty outlet/device id the edge would
 * 400 on anyway, or (now) an absent credential the edge would reject after
 * the handshake already succeeded. */
export function loadLanConfigFromEnv(env: Record<string, string | undefined>): LanConfig {
  const url = env.VITE_KDS_LAN_URL;
  const outletId = env.VITE_KDS_OUTLET_ID;
  const deviceId = env.VITE_KDS_DEVICE_ID;
  const deviceToken = env.VITE_KDS_DEVICE_TOKEN;
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
  if (!deviceToken) {
    throw new Error(
      "VITE_KDS_DEVICE_TOKEN is not configured — this screen has no enrolled credential and the edge will reject it (ADR-017).",
    );
  }
  return {
    url,
    outletId,
    deviceId,
    deviceToken,
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
