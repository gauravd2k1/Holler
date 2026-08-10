import { describe, expect, it } from "vitest";
import { buildConnectionUrl, loadLanConfigFromEnv } from "../lanConfig";

describe("loadLanConfigFromEnv", () => {
  it("builds a config from the env vars", () => {
    const config = loadLanConfigFromEnv({
      VITE_KDS_LAN_URL: "ws://192.168.1.50:7000/kds",
      VITE_KDS_OUTLET_ID: "018e5a2e-0000-7c3d-9f4e-1234567890ab",
      VITE_KDS_DEVICE_ID: "018e5a2e-7777-7c3d-9f4e-1234567890ab",
    });
    expect(config.url).toBe("ws://192.168.1.50:7000/kds");
    expect(config.outletId).toBe("018e5a2e-0000-7c3d-9f4e-1234567890ab");
    expect(config.deviceId).toBe("018e5a2e-7777-7c3d-9f4e-1234567890ab");
    expect(config.station).toBeNull();
  });

  it("carries an optional station filter when configured", () => {
    const config = loadLanConfigFromEnv({
      VITE_KDS_LAN_URL: "ws://192.168.1.50:7000/kds",
      VITE_KDS_OUTLET_ID: "018e5a2e-0000-7c3d-9f4e-1234567890ab",
      VITE_KDS_DEVICE_ID: "018e5a2e-7777-7c3d-9f4e-1234567890ab",
      VITE_KDS_STATION: "TANDOOR",
    });
    expect(config.station).toBe("TANDOOR");
  });

  it("throws rather than defaulting to a hard-coded host when unconfigured", () => {
    expect(() => loadLanConfigFromEnv({})).toThrow(/VITE_KDS_LAN_URL/);
  });

  it("throws when the outlet id is missing", () => {
    expect(() =>
      loadLanConfigFromEnv({
        VITE_KDS_LAN_URL: "ws://host/kds",
        VITE_KDS_DEVICE_ID: "018e5a2e-7777-7c3d-9f4e-1234567890ab",
      }),
    ).toThrow(/VITE_KDS_OUTLET_ID/);
  });

  it("throws when the device id is missing", () => {
    expect(() =>
      loadLanConfigFromEnv({
        VITE_KDS_LAN_URL: "ws://host/kds",
        VITE_KDS_OUTLET_ID: "018e5a2e-0000-7c3d-9f4e-1234567890ab",
      }),
    ).toThrow(/VITE_KDS_DEVICE_ID/);
  });
});

describe("buildConnectionUrl", () => {
  const outletId = "018e5a2e-0000-7c3d-9f4e-1234567890ab";
  const deviceId = "018e5a2e-7777-7c3d-9f4e-1234567890ab";

  it("appends outlet_id and device_id to a plain base URL", () => {
    const url = buildConnectionUrl({
      url: "ws://192.168.1.50:7000/kds",
      outletId,
      deviceId,
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

  it("adds the station param only when configured", () => {
    const url = buildConnectionUrl({
      url: "ws://192.168.1.50:7000/kds",
      outletId,
      deviceId,
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
