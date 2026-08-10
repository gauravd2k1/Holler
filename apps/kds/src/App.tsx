import { useEffect, useMemo, useState } from "react";
import { useKdsStore } from "./store/kdsStore";
import { ConnectionController } from "./lib/connectionController";
import { loadLanConfigFromEnv } from "./lib/lanConfig";
import { TicketCard } from "./components/TicketCard";
import { ConnectionBanner } from "./components/ConnectionBanner";
import type { KotStatus } from "@holler/contracts";

import type { WebSocketLike } from "./lib/lanClient";

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

export function App() {
  const kots = useKdsStore((s) => s.kots);
  const connectionStatus = useKdsStore((s) => s.connectionStatus);
  const pendingByKotId = useKdsStore((s) => s.pendingByKotId);
  const [now, setNow] = useState(() => new Date());
  const [configError, setConfigError] = useState<string | null>(null);

  const controller = useMemo(() => {
    try {
      const config = loadLanConfigFromEnv(import.meta.env as unknown as Record<string, string>);
      return new ConnectionController({
        config,
        createSocket: createBrowserSocketFactory(),
        store: useKdsStore,
      });
    } catch (err) {
      setConfigError(err instanceof Error ? err.message : String(err));
      return null;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
      <ConnectionBanner status={connectionStatus} />
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
      </div>
    </main>
  );
}
