import { useState } from "react";
import { MenuScreen } from "./components/MenuScreen";
import { SuppliersScreen } from "./components/SuppliersScreen";
import { GoodsReceiptsScreen } from "./components/GoodsReceiptsScreen";
import { configError } from "./lib/api";
import { SignIn } from "./components/SignIn";
import { currentPrincipal, signOut, type Principal } from "./lib/session";

const TABS = [
  { id: "menu", label: "Menu and pricing" },
  { id: "suppliers", label: "Suppliers" },
  { id: "receipts", label: "Goods receipts" },
] as const;

type TabId = (typeof TABS)[number]["id"];

export function App() {
  const [tab, setTab] = useState<TabId>("menu");
  const [principal, setPrincipal] = useState<Principal | null>(currentPrincipal());

  // A missing base URL or outlet id is reported here rather than as a wall of
  // failed requests. The app has no default for either: a build that silently
  // points at localhost works in exactly one environment and looks correct in
  // all of them.
  const misconfigured = configError();
  if (misconfigured !== null) {
    return (
      <main>
        <h1>Holler Admin</h1>
        <p className="error">
          {misconfigured} Set it in <code>apps/admin/.env.local</code> and restart the dev
          server.
        </p>
      </main>
    );
  }

  // The token lives in memory only, so a refresh returns here. That is the
  // deliberate cost of not putting a bearer token in localStorage.
  if (principal === null) {
    return (
      <main>
        <SignIn onSignedIn={setPrincipal} />
      </main>
    );
  }

  return (
    <main>
      <header>
        <span>
          {principal.email} · outlet {principal.outlet_id}
        </span>
        <button
          type="button"
          onClick={() => {
            signOut();
            setPrincipal(null);
          }}
        >
          Sign out
        </button>
      </header>
      <nav>
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            className={t.id === tab ? "active" : ""}
            onClick={() => setTab(t.id)}
          >
            {t.label}
          </button>
        ))}
      </nav>
      {tab === "menu" && <MenuScreen />}
      {tab === "suppliers" && <SuppliersScreen />}
      {tab === "receipts" && <GoodsReceiptsScreen />}
    </main>
  );
}
