import { describe, expect, it } from "vitest";
import { buildConnectionUrl, deriveLanUrlFromPage, loadLanConfigFromEnv } from "../lanConfig";

// The identity vars are required on every path; only the ADDRESS is derived.
const IDENTITY = {
  VITE_KDS_OUTLET_ID: "018e5a2e-0000-7c3d-9f4e-1234567890ab",
  VITE_KDS_DEVICE_ID: "018e5a2e-7777-7c3d-9f4e-1234567890ab",
  VITE_KDS_DEVICE_TOKEN: "018e5a2e-cred-0000-9f4e-1234567890ab.secret",
};

// The kitchen laptop's view: this page was served by the till at that address
// (`docs/demo-wednesday.md` step 3 opens `http://<till>:5174/`).
const FROM_TILL = { protocol: "http:", hostname: "192.168.137.1" };

describe("loadLanConfigFromEnv", () => {
  it("builds a config from the env vars", () => {
    const config = loadLanConfigFromEnv({
      VITE_KDS_LAN_URL: "ws://192.168.1.50:7000/kds",
      VITE_KDS_OUTLET_ID: "018e5a2e-0000-7c3d-9f4e-1234567890ab",
      VITE_KDS_DEVICE_ID: "018e5a2e-7777-7c3d-9f4e-1234567890ab",
      VITE_KDS_DEVICE_TOKEN: "018e5a2e-cred-0000-9f4e-1234567890ab.secret",
    });
    expect(config.url).toBe("ws://192.168.1.50:7000/kds");
    expect(config.outletId).toBe("018e5a2e-0000-7c3d-9f4e-1234567890ab");
    expect(config.deviceId).toBe("018e5a2e-7777-7c3d-9f4e-1234567890ab");
    expect(config.deviceToken).toBe("018e5a2e-cred-0000-9f4e-1234567890ab.secret");
    expect(config.station).toBeNull();
  });

  it("carries an optional station filter when configured", () => {
    const config = loadLanConfigFromEnv({
      VITE_KDS_LAN_URL: "ws://192.168.1.50:7000/kds",
      VITE_KDS_OUTLET_ID: "018e5a2e-0000-7c3d-9f4e-1234567890ab",
      VITE_KDS_DEVICE_ID: "018e5a2e-7777-7c3d-9f4e-1234567890ab",
      VITE_KDS_DEVICE_TOKEN: "018e5a2e-cred-0000-9f4e-1234567890ab.secret",
      VITE_KDS_STATION: "TANDOOR",
    });
    expect(config.station).toBe("TANDOOR");
  });

  it("derives the till's address from the page when no URL is configured", () => {
    // The fix. The kitchen laptop loaded this page from the till, so the till
    // is at that hostname -- re-read on every load, so it cannot go stale.
    const config = loadLanConfigFromEnv({ ...IDENTITY }, FROM_TILL);
    expect(config.url).toBe("ws://192.168.137.1:9310/kds");
  });

  it("derives localhost when the screen is opened on the till itself", () => {
    const config = loadLanConfigFromEnv({ ...IDENTITY }, { protocol: "http:", hostname: "localhost" });
    expect(config.url).toBe("ws://localhost:9310/kds");
  });

  it("lets an explicitly configured URL win over the derived one", () => {
    // The escape hatch AND the rollback: a deployment that does NOT serve this
    // screen from the till sets the var and gets the old behaviour verbatim.
    const config = loadLanConfigFromEnv(
      { ...IDENTITY, VITE_KDS_LAN_URL: "ws://10.0.0.9:7000/kds" },
      FROM_TILL,
    );
    expect(config.url).toBe("ws://10.0.0.9:7000/kds");
  });

  it("honours an overridden edge port while still deriving the host", () => {
    const config = loadLanConfigFromEnv({ ...IDENTITY, VITE_KDS_LAN_PORT: "9999" }, FROM_TILL);
    expect(config.url).toBe("ws://192.168.137.1:9999/kds");
  });

  it("throws when there is neither a configured URL nor a page to derive from", () => {
    expect(() => loadLanConfigFromEnv({ ...IDENTITY })).toThrow(/address/i);
  });

  it("still throws on a missing identity even though the address derives", () => {
    // Deriving an ADDRESS is safe; guessing an IDENTITY never is. The
    // credential in particular cannot be derived from anything.
    expect(() => loadLanConfigFromEnv({}, FROM_TILL)).toThrow(/VITE_KDS_OUTLET_ID/);
  });

  it("throws when the outlet id is missing", () => {
    expect(() =>
      loadLanConfigFromEnv({
        VITE_KDS_LAN_URL: "ws://host/kds",
        VITE_KDS_DEVICE_ID: "018e5a2e-7777-7c3d-9f4e-1234567890ab",
        VITE_KDS_DEVICE_TOKEN: "018e5a2e-cred-0000-9f4e-1234567890ab.secret",
      }),
    ).toThrow(/VITE_KDS_OUTLET_ID/);
  });

  it("throws when the device id is missing", () => {
    expect(() =>
      loadLanConfigFromEnv({
        VITE_KDS_LAN_URL: "ws://host/kds",
        VITE_KDS_OUTLET_ID: "018e5a2e-0000-7c3d-9f4e-1234567890ab",
        VITE_KDS_DEVICE_TOKEN: "018e5a2e-cred-0000-9f4e-1234567890ab.secret",
      }),
    ).toThrow(/VITE_KDS_DEVICE_ID/);
  });

  it("throws when the device token is missing", () => {
    expect(() =>
      loadLanConfigFromEnv({
        VITE_KDS_LAN_URL: "ws://host/kds",
        VITE_KDS_OUTLET_ID: "018e5a2e-0000-7c3d-9f4e-1234567890ab",
        VITE_KDS_DEVICE_ID: "018e5a2e-7777-7c3d-9f4e-1234567890ab",
      }),
    ).toThrow(/VITE_KDS_DEVICE_TOKEN/);
  });
});

describe("buildConnectionUrl", () => {
  const outletId = "018e5a2e-0000-7c3d-9f4e-1234567890ab";
  const deviceId = "018e5a2e-7777-7c3d-9f4e-1234567890ab";
  const deviceToken = "018e5a2e-cred-0000-9f4e-1234567890ab.secret";

  it("appends outlet_id and device_id to a plain base URL", () => {
    const url = buildConnectionUrl({
      url: "ws://192.168.1.50:7000/kds",
      outletId,
      deviceId,
      deviceToken,
      station: null,
      heartbeatTimeoutMs: 1,
      reconnectDelayMs: 1,
      transitionTimeoutMs: 1,
    });
    const parsed = new URL(url);
    expect(parsed.origin).toBe("ws://192.168.1.50:7000");
    expect(parsed.pathname).toBe("/kds");
    expect(parsed.searchParams.get("outlet_id")).toBe(outletId);
    expect(parsed.searchParams.get("device_id")).toBe(deviceId);
    expect(parsed.searchParams.has("station")).toBe(false);
  });

  // ADR-017 §3: the whole point of moving the credential out of the query
  // string. `device_token` must never appear in `buildConnectionUrl`'s
  // output — it travels only in `LanClient`'s first WS frame
  // (`lanClient.test.ts`).
  it("never places device_token in the connection URL", () => {
    const url = buildConnectionUrl({
      url: "ws://192.168.1.50:7000/kds",
      outletId,
      deviceId,
      deviceToken,
      station: "TANDOOR",
      heartbeatTimeoutMs: 1,
      reconnectDelayMs: 1,
      transitionTimeoutMs: 1,
    });
    expect(url).not.toContain(deviceToken);
    expect(new URL(url).searchParams.has("device_token")).toBe(false);
  });

  it("adds the station param only when configured", () => {
    const url = buildConnectionUrl({
      url: "ws://192.168.1.50:7000/kds",
      outletId,
      deviceId,
      deviceToken,
      station: "TANDOOR",
      heartbeatTimeoutMs: 1,
      reconnectDelayMs: 1,
      transitionTimeoutMs: 1,
    });
    expect(new URL(url).searchParams.get("station")).toBe("TANDOOR");
  });

  it("merges with a base URL that already carries a query string", () => {
    const url = buildConnectionUrl({
      url: "ws://192.168.1.50:7000/kds?debug=1",
      outletId,
      deviceId,
      deviceToken,
      station: null,
      heartbeatTimeoutMs: 1,
      reconnectDelayMs: 1,
      transitionTimeoutMs: 1,
    });
    const parsed = new URL(url);
    expect(parsed.searchParams.get("debug")).toBe("1");
    expect(parsed.searchParams.get("outlet_id")).toBe(outletId);
    expect(parsed.searchParams.get("device_id")).toBe(deviceId);
  });

  it("handles a base URL with a trailing slash without producing a malformed path", () => {
    const url = buildConnectionUrl({
      url: "ws://192.168.1.50:7000/kds/",
      outletId,
      deviceId,
      deviceToken,
      station: null,
      heartbeatTimeoutMs: 1,
      reconnectDelayMs: 1,
      transitionTimeoutMs: 1,
    });
    const parsed = new URL(url);
    expect(parsed.pathname).toBe("/kds/");
    expect(parsed.searchParams.get("outlet_id")).toBe(outletId);
    expect(parsed.searchParams.get("device_id")).toBe(deviceId);
  });
});

describe("deriveLanUrlFromPage", () => {
  it("keeps the edge's port, never the page's", () => {
    // The page is served by Vite on 5174 and the edge listens on 9310.
    // Reusing the page's port would aim the socket at the dev server, which
    // answers -- so the failure would look like a protocol fault, not an
    // address one.
    expect(deriveLanUrlFromPage({ protocol: "http:", hostname: "192.168.137.1" }, "9310")).toBe(
      "ws://192.168.137.1:9310/kds",
    );
  });

  it("uses wss when the page itself was served over https", () => {
    // A secure page opening an insecure socket is blocked by the browser as
    // mixed content, and blocked silently -- the exact failure shape this
    // change exists to remove.
    expect(deriveLanUrlFromPage({ protocol: "https:", hostname: "till.local" }, "9310")).toBe(
      "wss://till.local:9310/kds",
    );
  });

  it("brackets a bare IPv6 literal", () => {
    // Browsers disagree about whether `location.hostname` keeps the brackets,
    // so both spellings must produce the same valid URL.
    expect(deriveLanUrlFromPage({ protocol: "http:", hostname: "::1" }, "9310")).toBe(
      "ws://[::1]:9310/kds",
    );
    expect(deriveLanUrlFromPage({ protocol: "http:", hostname: "[::1]" }, "9310")).toBe(
      "ws://[::1]:9310/kds",
    );
  });

  it("throws rather than building a hostless URL", () => {
    // `file://` has no hostname. Failing loudly beats emitting `ws://:9310`,
    // which parses and connects to nothing.
    expect(() => deriveLanUrlFromPage({ protocol: "http:", hostname: "" }, "9310")).toThrow(
      /hostname/i,
    );
  });

  it("produces a URL the handshake builder can extend", () => {
    // Derivation and `buildConnectionUrl` must compose: the derived string is
    // the base the identity params get appended to.
    const url = buildConnectionUrl({
      url: deriveLanUrlFromPage({ protocol: "http:", hostname: "192.168.137.1" }, "9310"),
      outletId: "018e5a2e-0000-7c3d-9f4e-1234567890ab",
      deviceId: "018e5a2e-7777-7c3d-9f4e-1234567890ab",
      deviceToken: "018e5a2e-cred-0000-9f4e-1234567890ab.secret",
      station: null,
      heartbeatTimeoutMs: 1,
      reconnectDelayMs: 1,
      transitionTimeoutMs: 1,
    });
    const parsed = new URL(url);
    expect(parsed.origin).toBe("ws://192.168.137.1:9310");
    expect(parsed.pathname).toBe("/kds");
    expect(parsed.searchParams.get("outlet_id")).toBe("018e5a2e-0000-7c3d-9f4e-1234567890ab");
  });
});
