import { useEffect, useState } from "react";
import type { CanonicalOrder } from "@holler/contracts";
import { PairScreen } from "./components/PairScreen";
import { TablesScreen } from "./components/TablesScreen";
import { MenuCartScreen } from "./components/MenuCartScreen";
import { SentScreen } from "./components/SentScreen";
import {
  appendOrderItems,
  createOrder,
  sendOrder,
  fetchSession,
  ApiError,
  type CaptainTable,
  type KotSummary,
  type Session,
} from "./lib/api";
import { cartToOrderItems, type CartLine } from "./lib/cart";
import { storedDeviceToken, clearDeviceToken } from "./lib/session";

type Screen =
  | { kind: "checking" }
  | { kind: "pair" }
  | { kind: "tables" }
  | { kind: "menu"; table: CaptainTable }
  | { kind: "sent"; order: CanonicalOrder; kots: KotSummary[] };

/**
 * The captain app — four screens, no router. docs/captain-api.md's flow is
 * strictly linear (pair once, then table -> menu -> send, repeat), so a state
 * machine here is simpler and lighter than pulling in TanStack Router for a
 * phone page with one path through it.
 */
export function App() {
  const [screen, setScreen] = useState<Screen>({ kind: "checking" });
  const [token, setToken] = useState<string | null>(null);
  const [session, setSession] = useState<Session | null>(null);
  const [cart, setCart] = useState<CartLine[]>([]);
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState<string | null>(null);

  // On first render, re-validate any stored token rather than trusting it.
  // A stored-but-now-revoked token must fall back to the pair screen, not a
  // stuck "checking" state.
  useEffect(() => {
    if (screen.kind !== "checking") return;
    const stored = storedDeviceToken();
    if (stored === null) {
      setScreen({ kind: "pair" });
      return;
    }
    fetchSession(stored)
      .then((s) => {
        setToken(stored);
        setSession(s);
        setScreen({ kind: "tables" });
      })
      .catch(() => {
        clearDeviceToken();
        setScreen({ kind: "pair" });
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [screen.kind]);

  if (screen.kind === "checking") {
    return <div className="screen">Checking device pairing…</div>;
  }

  function handlePaired(newToken: string, newSession: Session) {
    setToken(newToken);
    setSession(newSession);
    setScreen({ kind: "tables" });
  }

  function handleSelectTable(table: CaptainTable) {
    setCart([]);
    setSendError(null);
    setScreen({ kind: "menu", table });
  }

  async function handleSend(table: CaptainTable) {
    if (token === null || cart.length === 0) return;
    setSending(true);
    setSendError(null);
    try {
      const items = cartToOrderItems(cart);
      let order: CanonicalOrder;
      if (table.open_order_id !== null) {
        // Append-only: an order the kitchen may already have. Each line is
        // its own append call per docs/captain-api.md's request shape, sent
        // in sequence so the running order in the response is always current.
        let current: CanonicalOrder | null = null;
        for (const item of items) {
          current = await appendOrderItems(token, table.open_order_id, item);
        }
        // items.length > 0 was already checked above (cart.length === 0 guard).
        order = current as CanonicalOrder;
      } else {
        order = await createOrder(token, {
          order_type: "DINE_IN",
          table_id: table.id,
          items,
        });
      }
      const result = await sendOrder(token, order.holler_order_id);
      setCart([]);
      setScreen({ kind: "sent", order: result.order, kots: result.kots });
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) {
        clearDeviceToken();
        setToken(null);
        setSession(null);
        setScreen({ kind: "pair" });
        return;
      }
      setSendError(err instanceof Error ? err.message : String(err));
    } finally {
      setSending(false);
    }
  }

  return (
    <main>
      <header className="app-header">
        <span>{session?.outlet_name ?? "Holler Captain"}</span>
      </header>
      {screen.kind === "pair" && <PairScreen onPaired={handlePaired} />}
      {screen.kind === "tables" && token !== null && (
        <TablesScreen token={token} onSelectTable={handleSelectTable} />
      )}
      {screen.kind === "menu" && token !== null && (
        <MenuCartScreen
          token={token}
          tableName={screen.table.name}
          cart={cart}
          onCartChange={setCart}
          onSend={() => {
            handleSend(screen.table).catch(() => undefined);
          }}
          sending={sending}
          sendError={sendError}
        />
      )}
      {screen.kind === "sent" && (
        <SentScreen
          order={screen.order}
          kots={screen.kots}
          onDone={() => setScreen({ kind: "tables" })}
        />
      )}
    </main>
  );
}
