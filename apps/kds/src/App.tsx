import { useEffect, useMemo, useRef, useState } from "react";
import { useKdsStore } from "./store/kdsStore";
import { ConnectionController } from "./lib/connectionController";
import { loadLanConfigFromEnv } from "./lib/lanConfig";
import { TicketCard } from "./components/TicketCard";
import { ConnectionBanner } from "./components/ConnectionBanner";
import type { KotStatus } from "@holler/contracts";

import type { WebSocketLike } from "./lib/lanClient";
import { noteTicketPainted, latencySummary } from "./lib/perf";

/** Adapts the browser's `WebSocket` (whose handler types carry an `Event`
 * argument) to the minimal `WebSocketLike` shape `LanClient` depends on. */
function createBrowserSocketFactory() {
  return (url: string): WebSocketLike => {
    const socket = new WebSocket(url);
    const adapter: WebSocketLike = {
      send: (data) => socket.send(data),
      close: () => socket.close(),
      onopen: null,
      onclose: null,
      onerror: null,
      onmessage: null,
    };
    socket.onopen = () => adapter.onopen?.();
    socket.onclose = () => adapter.onclose?.();
    socket.onerror = (ev) => adapter.onerror?.(ev);
    socket.onmessage = (ev) => adapter.onmessage?.({ data: String(ev.data) });
    return adapter;
  };
}

/**
 * The connection indicator used to print the raw state -- "● connecting",
 * "● disconnected" -- which is a variable name on a kitchen screen. A cook
 * reads this from across a hot line and needs a fact, not an enum member.
 */
function connectionLabel(status: string): string {
  switch (status) {
    case "connecting":
      return "Connecting…";
    case "reconnecting":
      return "Reconnecting…";
    case "disconnected":
      return "Offline";
    default:
      return status.charAt(0).toUpperCase() + status.slice(1);
  }
}

export function App() {
  const kots = useKdsStore((s) => s.kots);
  const connectionStatus = useKdsStore((s) => s.connectionStatus);
  const pendingByKotId = useKdsStore((s) => s.pendingByKotId);
  const [now, setNow] = useState(() => new Date());
  const [configError, setConfigError] = useState<string | null>(null);

  const controller = useMemo(() => {
    try {
      // The till's address is derived from the address THIS PAGE came from,
      // because the till is what serves this screen — see the derivation
      // note in `lib/lanConfig.ts`. Read here at mount rather than at
      // dev-server start, which is the whole point: a baked address goes
      // stale the moment the network moves and is retried silently for ever.
      const config = loadLanConfigFromEnv(import.meta.env as unknown as Record<string, string>, {
        protocol: window.location.protocol,
        hostname: window.location.hostname,
      });
      return new ConnectionController({
        config,
        createSocket: createBrowserSocketFactory(),
        store: useKdsStore,
      });
    } catch (err) {
      setConfigError(err instanceof Error ? err.message : String(err));
      return null;
    }
    // Intentionally runs once on mount only — see the config-load try/catch
    // above; there is no dependency that should retrigger it.
  }, []);

  useEffect(() => {
    if (!controller) return;
    controller.start();
    return () => controller.stop();
  }, [controller]);

  useEffect(() => {
    const timer = setInterval(() => setNow(new Date()), 1_000);
    return () => clearInterval(timer);
  }, []);

  const tickets = useMemo(
    () =>
      Object.values(kots).sort(
        (a, b) => new Date(a.created_at).getTime() - new Date(b.created_at).getTime(),
      ),
    [kots],
  );

  // Perf marker: the far end of "captain send -> KDS render". One line the
  // FIRST time a ticket id appears on this screen -- never on a re-render,
  // which would bury the timestamp that matters under its own repeats.
  // Same `HOLLER-PERF` format as the Rust side, correlated by order id.
  //
  // IT NOW REPORTS THE LATENCY RATHER THAN A TIMESTAMP TO BE SUBTRACTED BY
  // HAND. The old line was half a measurement: it had to be paired with
  // `kot_upserted_emitted` from the POS terminal and the difference worked out
  // manually, which nobody does under demo pressure -- so the number went
  // unrecorded for days. `sent_at` is already on the frame, so the KDS can
  // just do the arithmetic. See lib/perf.ts for why there are two numbers.
  const seenTickets = useRef(new Set<string>());
  const [, forcePerfRender] = useState(0);
  useEffect(() => {
    let painted = false;
    for (const kot of tickets) {
      if (seenTickets.current.has(kot.id)) continue;
      seenTickets.current.add(kot.id);
      const latency = noteTicketPainted(kot.id, kot.order_id);
      if (latency) {
        painted = true;
        console.log(
          `HOLLER-PERF ts=${new Date().toISOString()} event=kds_ticket_rendered ` +
            `id=${kot.order_id} wire_ms=${latency.wireMs} render_ms=${latency.renderMs}`,
        );
      } else {
        // No sample: this ticket arrived in a snapshot (a reconnect or first
        // load), not as a live upsert. Timing a snapshot ticket would measure
        // how long it sat on the till before this screen connected, which is
        // not what anyone means by kitchen latency.
        console.log(
          `HOLLER-PERF ts=${new Date().toISOString()} event=kds_ticket_rendered ` +
            `id=${kot.order_id} wire_ms=n/a (snapshot, not a live ticket)`,
        );
      }
    }
    if (painted) forcePerfRender((n) => n + 1);
  }, [tickets]);

  // ?perf=1 shows the readout on screen. OFF by default and deliberately so:
  // a latency box is for a rehearsal, and a stray debug overlay in front of a
  // client is exactly the kind of dev furniture the demo brief forbids.
  const showPerf = useMemo(
    () => new URLSearchParams(window.location.search).get("perf") === "1",
    [],
  );
  const perf = latencySummary();

  if (configError) {
    return (
      <main className="kds-config-error" role="alert">
        <h1>KDS is not configured</h1>
        <p>{configError}</p>
      </main>
    );
  }

  return (
    <main className="kds-screen">
      <header className="holler-header">
        <div className="holler-header__brand">
          <img src="/holler_no_bg.png" alt="Holler" className="holler-logo" />
          <span>Kitchen Display</span>
        </div>
      </header>
      {/* Permanent status indicator, unlike `ConnectionBanner` (which hides
          itself once connected so a healthy screen isn't cluttered). This one
          stays in the DOM in every state, including "connected", so a smoke
          test — or a cook glancing at the corner — can always confirm the
          screen believes it is live without waiting for a problem to show a
          banner. See docs/retro.md 2026-08-11: nothing in the pipeline could
          previously observe "the app actually reached its connected state in
          a real browser". */}
      <div
        className={`kds-connection-indicator kds-connection-indicator--${connectionStatus}`}
        data-testid="connection-status"
        data-status={connectionStatus}
        role="status"
      >
        {connectionStatus === "connected" ? "● Connected" : `● ${connectionLabel(connectionStatus)}`}
      </div>
      <ConnectionBanner status={connectionStatus} />
      {showPerf && (
        <div className="kds-perf" role="status" data-testid="perf-readout">
          {perf.count === 0 ? (
            <span>waiting for a live ticket…</span>
          ) : (
            <>
              <span>
                last <strong>{perf.last!.wireMs} ms</strong>
              </span>
              <span>
                worst <strong>{perf.worstWireMs} ms</strong>
              </span>
              <span>
                render {perf.last!.renderMs} ms · n={perf.count}
              </span>
              {/* Stated, not hidden: `last` and `worst` cross two clocks. On a
                  KDS running on the till there is no skew and they are exact;
                  on another device they carry whatever that device's clock is
                  out by. `render` is measured entirely here and cannot skew,
                  so a sane render beside an absurd wire figure means the
                  clocks disagree, not that the software is slow. */}
              <span className="kds-perf__note">
                wire crosses two clocks; render is local
              </span>
            </>
          )}
        </div>
      )}
      <div className="kds-board">
        {tickets.map((kot) => (
          <TicketCard
            key={kot.id}
            kot={kot}
            now={now}
            pending={pendingByKotId[kot.id]}
            onAdvance={(kotId, status: KotStatus) => controller?.requestStatusChange(kotId, status)}
          />
        ))}
        {tickets.length === 0 && connectionStatus === "connected" && (
          <p className="kds-board__empty">No active tickets</p>
        )}
        {/* AN EMPTY BOARD MUST SAY WHICH EMPTY IT IS. Until now the board
            said "No active tickets" only when CONNECTED and showed nothing at
            all otherwise -- so a kitchen screen that had not connected yet,
            or had dropped, was a blank rectangle indistinguishable from a
            quiet service. The two are opposite facts for a cook. */}
        {tickets.length === 0 && connectionStatus !== "connected" && (
          <p className="kds-board__empty">
            {connectionStatus === "connecting"
              ? "Connecting to the till…"
              : "Not connected to the till — tickets will appear when the connection returns."}
          </p>
        )}
      </div>
    </main>
  );
}
