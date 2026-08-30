// T10 (Milestone 2): proves one real KDS<->edge socket session across the
// language boundary. This test imports the REAL apps/kds client modules
// (lanConfig.buildConnectionUrl, LanClient, ConnectionController, kdsStore)
// and drives them, over a genuine WebSocket, against the REAL compiled
// edge/device server (via tests/integration/kds-lan-bridge — see bridge.ts).
// Nothing here reimplements either side's protocol logic.
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import http from "node:http";
import crypto from "node:crypto";

import { startBridge, type BridgeHandle } from "./bridge";

// apps/kds is not our directory to edit, but its published module surface is
// exactly what a KDS screen actually runs, so we import it directly rather
// than re-describing its behaviour.
import { buildConnectionUrl, type LanConfig } from "../../../apps/kds/src/lib/lanConfig";
import { ConnectionController } from "../../../apps/kds/src/lib/connectionController";
import { useKdsStore } from "../../../apps/kds/src/store/kdsStore";

function configFor(bridge: BridgeHandle, overrides: Partial<LanConfig> = {}): LanConfig {
  return {
    url: bridge.wsUrl,
    outletId: bridge.info.outlet_id,
    deviceId: bridge.info.kds_device_id,
    deviceToken: bridge.info.kds_device_token,
    station: null,
    heartbeatTimeoutMs: 10_000,
    reconnectDelayMs: 250,
    transitionTimeoutMs: 5_000,
    ...overrides,
  };
}

/**
 * FAILS THE SUITE WITH THE REASON, rather than letting every test die on a
 * bare `ReferenceError: WebSocket is not defined`.
 *
 * This suite deliberately takes no `ws` dependency: the point of T10 is a
 * GENUINE socket, and a library socket is one more thing standing between the
 * test and the claim. The cost is a hard floor of Node 22, where the global is
 * first unflagged — and a floor stated only in a comment is not a floor. CI
 * pinned Node 20 and this file's four tests failed for eleven days while the
 * same suite passed by hand on Node 24, with M2 acceptance item 5 recorded as
 * met throughout. An environment requirement that is not checked is an
 * environment requirement that is not met.
 */
function requireGlobalWebSocket(): void {
  if (typeof globalThis.WebSocket === "undefined") {
    throw new Error(
      `This suite needs Node's global WebSocket, which is unflagged from Node 22. ` +
        `Running Node ${process.version}. Upgrade the runtime — do NOT add a \`ws\` ` +
        `dependency to get past this: a library socket would no longer prove the ` +
        `one thing T10 exists to prove, that a real KDS speaks to a real edge over ` +
        `a real socket.`,
    );
  }
}

/** Node's own global `WebSocket` (available without any dependency since
 * Node 22) — a genuine socket, not a fake satisfying `LanClient`'s
 * interface. */
function createRealSocket(url: string) {
  requireGlobalWebSocket();
  return new WebSocket(url) as unknown as import("../../../apps/kds/src/lib/lanClient").WebSocketLike;
}

function resetStore(): void {
  // Merge, not replace — replacing would also wipe the store's action
  // functions, which live on the same object as the data fields.
  useKdsStore.setState({
    kots: {},
    connectionStatus: "connecting",
    lastMessageAt: null,
    pendingByKotId: {},
  });
}

function waitFor(predicate: () => boolean, timeoutMs = 15_000, intervalMs = 25): Promise<void> {
  return new Promise((resolve, reject) => {
    const start = Date.now();
    const tick = () => {
      if (predicate()) {
        resolve();
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

/** Performs the raw WebSocket opening handshake by hand (not via the `ws`
 * client abstraction) so the HTTP status code the server answered with is
 * directly observable — proves the numeric 400, not just "some failure". */
function rawHandshakeStatus(url: string): Promise<number> {
  return new Promise((resolve, reject) => {
    const parsed = new URL(url);
    const req = http.request(
      {
        hostname: parsed.hostname,
        port: parsed.port,
        path: parsed.pathname + parsed.search,
        method: "GET",
        headers: {
          Connection: "Upgrade",
          Upgrade: "websocket",
          "Sec-WebSocket-Key": crypto.randomBytes(16).toString("base64"),
          "Sec-WebSocket-Version": "13",
        },
      },
      (res) => {
        resolve(res.statusCode ?? -1);
        res.resume();
      },
    );
    req.on("upgrade", (res) => {
      resolve(res.statusCode ?? -1);
    });
    req.on("error", reject);
    req.end();
  });
}

describe("KDS <-> edge/device LAN interop (T10)", () => {
  let bridge: BridgeHandle;
  let controller: ConnectionController | null = null;

  beforeEach(async () => {
    bridge = await startBridge();
    resetStore();
  });

  afterEach(async () => {
    controller?.stop();
    controller = null;
    await bridge.stop();
  });

  it("1+2: handshake succeeds with the client's own buildConnectionUrl, and a snapshot arrives first and is applied", async () => {
    // Prove buildConnectionUrl is genuinely in use, not bypassed: the URL
    // this test expects the client to hit is the one the client's own
    // function produces.
    const expectedUrl = buildConnectionUrl(configFor(bridge));
    expect(expectedUrl).toContain(`outlet_id=${bridge.info.outlet_id}`);
    expect(expectedUrl).toContain(`device_id=${bridge.info.kds_device_id}`);

    controller = new ConnectionController({
      config: configFor(bridge),
      createSocket: createRealSocket,
      store: useKdsStore,
    });
    controller.start();

    await waitFor(() => useKdsStore.getState().connectionStatus === "connected");
    await waitFor(() => useKdsStore.getState().kots[bridge.info.kot_id] !== undefined);

    const kot = useKdsStore.getState().kots[bridge.info.kot_id];
    expect(kot.status).toBe("NEW");
    expect(kot.order_id).toBe(bridge.info.order_id);
  });

  it("3: a set_kot_status intent round-trips: edge validates, confirms, client renders the confirmed state", async () => {
    controller = new ConnectionController({
      config: configFor(bridge),
      createSocket: createRealSocket,
      store: useKdsStore,
    });
    controller.start();
    await waitFor(() => useKdsStore.getState().kots[bridge.info.kot_id] !== undefined);

    controller.requestStatusChange(bridge.info.kot_id, "ACKNOWLEDGED");

    // Pending immediately after asking (the screen does not update status
    // itself — it waits for the edge's confirmation).
    expect(useKdsStore.getState().pendingByKotId[bridge.info.kot_id]).toBeDefined();

    await waitFor(() => useKdsStore.getState().kots[bridge.info.kot_id]?.status === "ACKNOWLEDGED");
    // Confirmation clears the pending marker.
    expect(useKdsStore.getState().pendingByKotId[bridge.info.kot_id]).toBeUndefined();
  });

  it("4: an illegal transition is rejected and the client never shows the false state", async () => {
    controller = new ConnectionController({
      config: configFor(bridge, { transitionTimeoutMs: 800 }),
      createSocket: createRealSocket,
      store: useKdsStore,
    });
    controller.start();
    await waitFor(() => useKdsStore.getState().kots[bridge.info.kot_id] !== undefined);

    // NEW -> SERVED skips ACKNOWLEDGED/PREPARING/READY: illegal per the KOT
    // state machine the edge enforces (edge/device/src/tests.rs asserts the
    // same transition server-side).
    controller.requestStatusChange(bridge.info.kot_id, "SERVED");

    // No confirming message ever arrives, so the pending transition times
    // out rather than silently resolving — that timeout IS the "does not
    // show a false state" behaviour: the cook sees "not confirmed", not a
    // ticket that quietly jumped to SERVED.
    await waitFor(() => useKdsStore.getState().pendingByKotId[bridge.info.kot_id]?.timedOut === true);

    // And the rendered KOT itself never moved off NEW.
    expect(useKdsStore.getState().kots[bridge.info.kot_id]?.status).toBe("NEW");
  });

  it("5: a handshake missing a required param is refused with HTTP 400, before any frame moves", async () => {
    const brokenConfig = configFor(bridge, { outletId: "" });
    const brokenUrl = buildConnectionUrl(brokenConfig);
    // buildConnectionUrl still sets the key with an empty value — the server
    // treats an empty outlet_id the same as a missing one (server.rs:
    // `!outlet_id.is_empty()`).
    expect(brokenUrl).toContain("outlet_id=");
    expect(new URL(brokenUrl).searchParams.get("outlet_id")).toBe("");

    const status = await rawHandshakeStatus(brokenUrl);
    expect(status).toBe(400);

    // Client-eye view of the same rejection: a real WebSocket against that
    // URL must never open, and must not deliver a false "connected" status
    // to the store.
    requireGlobalWebSocket();
    await new Promise<void>((resolve, reject) => {
      const socket = new WebSocket(brokenUrl);
      const timer = setTimeout(() => {
        socket.close();
        reject(new Error("socket neither opened nor errored/closed within 5s"));
      }, 5_000);
      socket.onopen = () => {
        clearTimeout(timer);
        socket.close();
        reject(new Error("socket opened despite a missing required handshake param"));
      };
      const finish = () => {
        clearTimeout(timer);
        resolve();
      };
      socket.onerror = finish;
      socket.onclose = finish;
    });
  });
});
