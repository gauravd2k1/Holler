// Connection configuration for the LAN hop to `edge/device` (ADR-014 §6).
//
// THE TILL'S ADDRESS IS DERIVED FROM THE PAGE, NOT BAKED INTO IT.
//
// This screen is served BY the till: the kitchen laptop opens
// `http://<till>:5174/` and your own laptop opens `http://localhost:5174/`
// (`docs/demo-wednesday.md` step 3). So the host this page was loaded from IS
// the till, on every machine that can see this screen at all, and it cannot
// go stale — it is re-read from the page on every load.
//
// What it replaces, and why: `VITE_KDS_LAN_URL` is read by Vite at
// DEV-SERVER START and frozen into the bundle as a literal string. A server
// started before the network changed, or a `.env.dev` written for a previous
// hotspot lease, therefore serves a dead address for ever — and `lanClient`
// reconnects on every close, so it retries that dead address silently and
// never errors. `docs/backlog.md` records the live incident: a host that was
// never one of this machine's addresses was written, printed back as an `OK`
// line, and passed every downstream check, because `Test-NetConnection`
// succeeded against the WRONG host and nothing compared the two. It cost
// three separate failures in one evening.
//
// The env var REMAINS SUPPORTED and still wins when set, because deriving is
// only correct while the page is served by the till. A deployment that hosts
// this screen somewhere else — a CDN, a separate kiosk image, a reverse proxy
// — sets `VITE_KDS_LAN_URL` and gets exactly today's behaviour. That is the
// escape hatch, and it is also the rollback: setting it restores the old path
// with no code change.
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

/** The edge node's default LAN bind port (`HOLLER_LAN_BIND_ADDR` defaults to
 * `0.0.0.0:9310`, `edge/device/src/server.rs`) and the path its WebSocket
 * handshake is served on. Overridable per-install by `VITE_KDS_LAN_PORT`;
 * the PATH is fixed because the edge routes on it. */
const DEFAULT_LAN_PORT = "9310";
const LAN_PATH = "/kds";

/** The parts of `window.location` this module needs, and nothing more.
 * Passed in rather than read from a global so the derivation is a pure
 * function with tests, and so no module-level code touches `window`. */
export interface PageOrigin {
  /** `"http:"` or `"https:"` — decides `ws:` vs `wss:`. */
  protocol: string;
  /** Host without the port. May be an IPv6 literal. */
  hostname: string;
}

/** Builds the edge node's WebSocket base URL from the address this page was
 * served from. The PORT is the edge's, never the page's: the page comes from
 * Vite on 5174 and the edge listens on 9310, so reusing the page's port would
 * point the socket at the dev server. */
export function deriveLanUrlFromPage(page: PageOrigin, port: string): string {
  const hostname = page.hostname;
  if (!hostname) {
    throw new Error(
      "This page has no hostname to derive the till's address from (opened from a file rather than served?). Set VITE_KDS_LAN_URL explicitly.",
    );
  }
  // A page served over https must not open an insecure socket — the browser
  // blocks it as mixed content, and the failure is silent in exactly the way
  // this whole change exists to stop.
  const scheme = page.protocol === "https:" ? "wss" : "ws";
  // An IPv6 literal must stay bracketed inside a URL. Browsers disagree about
  // whether `location.hostname` keeps the brackets, so normalise rather than
  // trust either spelling.
  const host = hostname.includes(":") && !hostname.startsWith("[") ? `[${hostname}]` : hostname;
  return `${scheme}://${host}:${port}${LAN_PATH}`;
}

/** Reads `import.meta.env.VITE_KDS_OUTLET_ID` / `VITE_KDS_DEVICE_ID` /
 * `VITE_KDS_DEVICE_TOKEN` (and optional `VITE_KDS_STATION`,
 * `VITE_KDS_LAN_PORT`, `VITE_KDS_LAN_URL`).
 *
 * The till's ADDRESS comes from `page` — see the derivation note at the top
 * of this file — with `VITE_KDS_LAN_URL` overriding it when set.
 *
 * Still throws rather than guessing an IDENTITY. An unconfigured KDS must
 * fail loudly at startup, not point at an empty outlet/device id the edge
 * would 400 on anyway, or an absent credential the edge would reject after
 * the handshake already succeeded. The address is now derivable; a
 * credential never is. */
export function loadLanConfigFromEnv(
  env: Record<string, string | undefined>,
  page?: PageOrigin,
): LanConfig {
  const configuredUrl = env.VITE_KDS_LAN_URL;
  const outletId = env.VITE_KDS_OUTLET_ID;
  const deviceId = env.VITE_KDS_DEVICE_ID;
  const deviceToken = env.VITE_KDS_DEVICE_TOKEN;
  const station = env.VITE_KDS_STATION;
  let url: string;
  if (configuredUrl) {
    // Explicitly configured: used verbatim, exactly as before. This is the
    // escape hatch AND the rollback.
    url = configuredUrl;
  } else if (page) {
    url = deriveLanUrlFromPage(page, env.VITE_KDS_LAN_PORT || DEFAULT_LAN_PORT);
  } else {
    throw new Error(
      "Cannot work out the till's address: this screen was loaded without a page origin to derive from and VITE_KDS_LAN_URL is not set.",
    );
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
