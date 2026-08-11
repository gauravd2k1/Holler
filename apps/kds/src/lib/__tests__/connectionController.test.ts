import { describe, expect, it, beforeEach, vi } from "vitest";
import type { Kot } from "@holler/contracts";
import { ConnectionController } from "../connectionController";
import { useKdsStore } from "../../store/kdsStore";
import type { WebSocketLike } from "../lanClient";

const OUTLET_ID = "018e5a2e-0000-7c3d-9f4e-1234567890ab";
const DEVICE_ID = "018e5a2e-7777-7c3d-9f4e-1234567890ab";

const BASE_KOT: Kot = {
  id: "018e5a2e-6666-7c3d-9f4e-1234567890ab",
  order_id: "018e5a2e-2b1a-7c3d-9f4e-1234567890ab",
  station: "MAIN_KITCHEN",
  sequence: 1,
  status: "NEW",
  items: [
    {
      order_item_id: "018e5a2e-3333-7c3d-9f4e-1234567890ab",
      name: "Butter Chicken",
      quantity: 2,
      modifiers: ["Extra Spicy"],
      notes: "no onion",
    },
  ],
  created_by_device_id: DEVICE_ID,
  created_at: "2026-08-07T10:15:31Z",
  updated_at: "2026-08-07T10:15:31Z",
  schema_version: 1,
};

/** A fake socket the test drives directly — no real network, per the task's
 * "structure transport so it can be driven by a fake" requirement. */
class FakeSocket implements WebSocketLike {
  sent: string[] = [];
  closed = false;
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: ((ev: unknown) => void) | null = null;
  onmessage: ((ev: { data: string }) => void) | null = null;

  send(data: string): void {
    this.sent.push(data);
  }
  close(): void {
    this.closed = true;
    this.onclose?.();
  }
  open(): void {
    this.onopen?.();
  }
  receive(message: unknown): void {
    this.onmessage?.({ data: JSON.stringify(message) });
  }
}

function resetStore() {
  useKdsStore.setState({
    kots: {},
    connectionStatus: "connecting",
    lastMessageAt: null,
    pendingByKotId: {},
  });
}

beforeEach(() => {
  resetStore();
  vi.useFakeTimers();
});

function makeController(sockets: FakeSocket[]) {
  return new ConnectionController({
    config: {
      url: "ws://edge.local/kds",
      outletId: OUTLET_ID,
      deviceId: DEVICE_ID,
      station: null,
      heartbeatTimeoutMs: 5_000,
      reconnectDelayMs: 1_000,
      transitionTimeoutMs: 3_000,
    },
    createSocket: () => {
      const s = new FakeSocket();
      sockets.push(s);
      return s;
    },
    store: useKdsStore,
  });
}

describe("ConnectionController", () => {
  it("applies a snapshot fully on connect", () => {
    const sockets: FakeSocket[] = [];
    const controller = makeController(sockets);
    controller.start();
    sockets[0].open();
    sockets[0].receive({
      type: "snapshot",
      outlet_id: OUTLET_ID,
      sent_at: new Date().toISOString(),
      kots: [BASE_KOT],
    });

    expect(useKdsStore.getState().kots[BASE_KOT.id]).toEqual(BASE_KOT);
    expect(useKdsStore.getState().connectionStatus).toBe("connected");
    controller.stop();
  });

  it("replaces stale state with a fresh snapshot on reconnect rather than merging", () => {
    const sockets: FakeSocket[] = [];
    const controller = makeController(sockets);
    controller.start();
    sockets[0].open();
    sockets[0].receive({
      type: "snapshot",
      outlet_id: OUTLET_ID,
      sent_at: new Date().toISOString(),
      kots: [BASE_KOT],
    });
    expect(Object.keys(useKdsStore.getState().kots)).toHaveLength(1);

    // Connection drops...
    sockets[0].close();
    expect(useKdsStore.getState().connectionStatus).toBe("disconnected");
    // A dropped connection must not keep showing the old ticket set —
    // staleness must be visible, not silent.
    expect(useKdsStore.getState().kots).toEqual({});

    // ...and comes back with a different active set (ticket served while offline).
    vi.advanceTimersByTime(1_000);
    sockets[1].open();
    sockets[1].receive({
      type: "snapshot",
      outlet_id: OUTLET_ID,
      sent_at: new Date().toISOString(),
      kots: [],
    });

    expect(useKdsStore.getState().kots).toEqual({});
    expect(useKdsStore.getState().connectionStatus).toBe("connected");
    controller.stop();
  });

  it("shows stale when heartbeats stop arriving", () => {
    const sockets: FakeSocket[] = [];
    const controller = makeController(sockets);
    controller.start();
    sockets[0].open();
    sockets[0].receive({ type: "heartbeat", outlet_id: OUTLET_ID, sent_at: new Date().toISOString() });
    expect(useKdsStore.getState().connectionStatus).toBe("connected");

    vi.advanceTimersByTime(6_000); // > heartbeatTimeoutMs
    expect(useKdsStore.getState().connectionStatus).toBe("stale");
    controller.stop();
  });

  it("does not leave the UI in a false state when a transition is never confirmed", () => {
    const sockets: FakeSocket[] = [];
    const controller = makeController(sockets);
    controller.start();
    sockets[0].open();
    sockets[0].receive({
      type: "snapshot",
      outlet_id: OUTLET_ID,
      sent_at: new Date().toISOString(),
      kots: [BASE_KOT],
    });

    controller.requestStatusChange(BASE_KOT.id, "ACKNOWLEDGED");
    // The store must NOT optimistically show ACKNOWLEDGED.
    expect(useKdsStore.getState().kots[BASE_KOT.id].status).toBe("NEW");
    expect(useKdsStore.getState().pendingByKotId[BASE_KOT.id].timedOut).toBe(false);

    vi.advanceTimersByTime(4_200); // comfortably > transitionTimeoutMs, past the next 1s tick
    expect(useKdsStore.getState().kots[BASE_KOT.id].status).toBe("NEW");
    expect(useKdsStore.getState().pendingByKotId[BASE_KOT.id].timedOut).toBe(true);
    controller.stop();
  });

  it("clears pending state once the edge confirms the transition", () => {
    const sockets: FakeSocket[] = [];
    const controller = makeController(sockets);
    controller.start();
    sockets[0].open();
    sockets[0].receive({
      type: "snapshot",
      outlet_id: OUTLET_ID,
      sent_at: new Date().toISOString(),
      kots: [BASE_KOT],
    });

    controller.requestStatusChange(BASE_KOT.id, "ACKNOWLEDGED");
    sockets[0].receive({
      type: "kot_upserted",
      outlet_id: OUTLET_ID,
      sent_at: new Date().toISOString(),
      kot: { ...BASE_KOT, status: "ACKNOWLEDGED" },
    });

    expect(useKdsStore.getState().kots[BASE_KOT.id].status).toBe("ACKNOWLEDGED");
    expect(useKdsStore.getState().pendingByKotId[BASE_KOT.id]).toBeUndefined();
    controller.stop();
  });

  // Regression: the KDS crashed on mount in every real browser with
  // "Illegal invocation" while this whole suite — and the cross-language
  // socket harness — passed. `setInterval` is a method on the global object in
  // a browser and checks its receiver; storing the bare global on an instance
  // field detaches it, so `this.setIntervalFn(...)` invoked it with the
  // controller as `this`. Node's timers are plain functions and do not care,
  // so nothing running under Node could ever have caught it.
  //
  // This test installs a global that enforces the receiver check the way a
  // browser does. It fails without the `.bind(globalThis)` in the constructor.
  it("binds the timer globals, so start() survives a receiver-checking host", () => {
    const realSetInterval = globalThis.setInterval;
    const realClearInterval = globalThis.clearInterval;
    let scheduled = 0;

    try {
      globalThis.setInterval = function (this: unknown, ...args: unknown[]) {
        if (this !== globalThis) throw new TypeError("Illegal invocation");
        scheduled += 1;
        return (realSetInterval as (...a: unknown[]) => unknown).apply(globalThis, args);
      } as unknown as typeof setInterval;

      globalThis.clearInterval = function (this: unknown, ...args: unknown[]) {
        if (this !== globalThis) throw new TypeError("Illegal invocation");
        return (realClearInterval as (...a: unknown[]) => unknown).apply(globalThis, args);
      } as unknown as typeof clearInterval;

      // Constructed AFTER the strict globals are installed: the controller
      // captures them in its constructor, which is where the binding happens.
      const controller = makeController([]);
      expect(() => controller.start()).not.toThrow();
      expect(scheduled).toBe(1);
      expect(() => controller.stop()).not.toThrow();
    } finally {
      globalThis.setInterval = realSetInterval;
      globalThis.clearInterval = realClearInterval;
    }
  });
});
