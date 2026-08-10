import { describe, expect, it } from "vitest";
import { loadLanConfigFromEnv } from "../lanConfig";

describe("loadLanConfigFromEnv", () => {
  it("builds a config from the env vars", () => {
    const config = loadLanConfigFromEnv({
      VITE_KDS_LAN_URL: "ws://192.168.1.50:7000/kds",
      VITE_KDS_DEVICE_ID: "018e5a2e-7777-7c3d-9f4e-1234567890ab",
    });
    expect(config.url).toBe("ws://192.168.1.50:7000/kds");
    expect(config.deviceId).toBe("018e5a2e-7777-7c3d-9f4e-1234567890ab");
  });

  it("throws rather than defaulting to a hard-coded host when unconfigured", () => {
    expect(() => loadLanConfigFromEnv({})).toThrow(/VITE_KDS_LAN_URL/);
  });

  it("throws when the device id is missing", () => {
    expect(() =>
      loadLanConfigFromEnv({ VITE_KDS_LAN_URL: "ws://host/kds" }),
    ).toThrow(/VITE_KDS_DEVICE_ID/);
  });
});
