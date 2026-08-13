// Real-browser smoke test (T13, docs/retro.md 2026-08-11: "The KDS crashed
// in every browser while every test passed"). This is the one check in the
// whole pipeline that runs headless Chromium instead of Node — vitest uses
// jsdom's Node timers, and the kds-lan integration suite drives the real
// client modules from a Node process against a real compiled Rust server.
// Both prove protocol; neither can observe a browser-only failure mode like
// a detached global builtin (`setInterval` etc. stored bare on an instance
// field), which is exactly what shipped and crashed on mount before
// `1f31e98`.
//
// The stub server below is intentionally NOT edge/device — that integration
// belongs to tests/integration/kds-lan, which this task does not touch. It
// only has to speak the frozen `KdsLanMessageSchema` shape well enough to
// get the client to its "connected" state.
import { test, expect } from "@playwright/test";
import { WebSocketServer, type WebSocket as WsSocket } from "ws";

// Not imported from @holler/contracts here: Playwright's test runner uses
// Node's native TypeScript type-stripping (Node 22.6+), which refuses to
// strip files under node_modules — and the contracts package's "main" points
// straight at its TS source (packages/contracts/src/index.ts), so a runtime
// import of it fails specifically under this runner even though it works
// fine under Vite/vitest. The shape below is hand-matched to
// `KdsLanMessageSchema`'s "snapshot" variant in packages/contracts/src/types/
// lan.ts — kept minimal (empty `kots`) rather than round-tripped through
// `KotSchema`, since this stub's only job is to get the client to
// "connected", not to exercise ticket rendering (that is TicketCard's own
// vitest coverage).

// Must match apps/kds/.env.e2e's VITE_KDS_LAN_URL/VITE_KDS_OUTLET_ID/
// VITE_KDS_DEVICE_ID/VITE_KDS_DEVICE_TOKEN — the dev server under test is
// started with those values baked in via `--mode e2e`.
const STUB_LAN_PORT = 9401;
const OUTLET_ID = "018e5a2e-0000-7000-8000-0000000000e2";
const DEVICE_ID = "018e5a2e-0000-7000-8000-0000000000e3";
const DEVICE_TOKEN = "e2e-test.secret";

let wss: WebSocketServer;

test.beforeAll(() => {
  wss = new WebSocketServer({ port: STUB_LAN_PORT });
  wss.on("connection", (socket: WsSocket, request) => {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");
    // Same handshake contract edge/device enforces (ADR-015): identity comes
    // from the connection's query params, not a payload field. device_token
    // is deliberately NOT among them (ADR-017 §3) — it must never show up
    // here, only in the first WS frame below.
    expect(url.searchParams.get("outlet_id")).toBe(OUTLET_ID);
    expect(url.searchParams.get("device_id")).toBe(DEVICE_ID);
    expect(url.searchParams.has("device_token")).toBe(false);

    // ADR-017 hole 3, real-browser proof: the actual `WebSocket` this app
    // constructs sends the auth frame as its first message, exercising the
    // exact code path `LanClient.connect`'s `onopen` handler runs — jsdom
    // (connectionController.test.ts) proves the call happens, this proves a
    // real browser's WebSocket actually delivers it in order. This stub
    // does not enforce rejection (it is not edge/device, see the header
    // comment), it only asserts the frame arrived and only then sends the
    // snapshot — mirroring the real server's ordering.
    socket.once("message", (data) => {
      const frame = JSON.parse(data.toString());
      expect(frame).toEqual({ type: "auth", device_token: DEVICE_TOKEN });

      const snapshot = {
        type: "snapshot" as const,
        outlet_id: OUTLET_ID,
        sent_at: new Date().toISOString(),
        kots: [] as const,
      };
      socket.send(JSON.stringify(snapshot));
    });
  });
});

test.afterAll(async () => {
  await new Promise<void>((resolve, reject) => {
    wss.close((err) => (err ? reject(err) : resolve()));
  });
});

test("KDS mounts in a real browser, connects, and logs no console errors", async ({ page }) => {
  const consoleErrors: string[] = [];
  const pageErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") consoleErrors.push(msg.text());
  });
  page.on("pageerror", (err) => {
    pageErrors.push(err.message);
  });

  await page.goto("/");

  const indicator = page.getByTestId("connection-status");
  await expect(indicator).toBeVisible();
  await expect(indicator).toHaveAttribute("data-status", "connected", { timeout: 10_000 });
  await expect(indicator).toHaveText("● Connected");

  expect(consoleErrors, `console errors: ${JSON.stringify(consoleErrors)}`).toEqual([]);
  expect(pageErrors, `uncaught page errors: ${JSON.stringify(pageErrors)}`).toEqual([]);
});
