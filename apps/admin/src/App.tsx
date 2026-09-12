import { useState } from "react";
import { MenuScreen } from "./components/MenuScreen";
import { SuppliersScreen } from "./components/SuppliersScreen";
import { GoodsReceiptsScreen } from "./components/GoodsReceiptsScreen";
import { OrdersScreen } from "./components/OrdersScreen";
import { configError } from "./lib/api";
import { SignIn } from "./components/SignIn";
import { currentPrincipal, signOut, type Principal } from "./lib/session";

const TABS = [
  { id: "orders", label: "Orders" },
  { id: "menu", label: "Menu and pricing" },
  { id: "suppliers", label: "Suppliers" },
  { id: "receipts", label: "Goods receipts" },
] as const;

type TabId = (typeof TABS)[number]["id"];

// The brand mark on every state this screen can be in, sign-in and
// misconfigured included — not only once a principal is signed in.
function Brand() {
  return (
    <header className="holler-header">
      <div className="holler-header__brand">
        <img src="/holler_no_bg.png" alt="Holler" className="holler-logo" />
        <span>Holler Admin</span>
      </div>
    </header>
  );
}

export function App() {
  // Menu stays the landing tab. Orders is FIRST in the bar because demo steps
  // 4 and 5 go there, but the landing tab is deliberately unchanged: the
  // scenario harness asserts the Menu screen is what replaces the sign-in
  // form, and reshaping the product to keep a test string true -- or silently
  // breaking that assertion -- are both worse than leaving it alone.
  const [tab, setTab] = useState<TabId>("menu");
  const [principal, setPrincipal] = useState<Principal | null>(currentPrincipal());

  // A missing base URL or outlet id is reported here rather than as a wall of
  // failed requests. The app has no default for either: a build that silently
  // points at localhost works in exactly one environment and looks correct in
  // all of them.
  const misconfigured = configError();
  if (misconfigured !== null) {
    return (
      <>
        <Brand />
        <main>
          <p className="error">
            {misconfigured} Set it in <code>apps/admin/.env.local</code> and restart the dev
            server.
          </p>
        </main>
      </>
    );
  }

  // The token lives in memory only, so a refresh returns here. That is the
  // deliberate cost of not putting a bearer token in localStorage.
  if (principal === null) {
    return (
      <>
        <Brand />
        <main>
          <SignIn onSignedIn={setPrincipal} />
        </main>
      </>
    );
  }

  return (
    <>
      <header className="holler-header">
        <div className="holler-header__brand">
          <img src="/holler_no_bg.png" alt="Holler" className="holler-logo" />
          <span>Holler Admin</span>
        </div>
        <div className="holler-header__spacer" />
        {/* The signed-in person's name only — never the outlet's UUID
            (CLAUDE.md §Money/time/identifiers). This build serves one outlet
            per environment, so there is nothing to disambiguate here today;
            a friendly outlet name is `AuthenticatedPrincipal.outlet_id`
            resolved, which the contract does not yet carry. */}
        <span className="muted">{principal.full_name}</span>
        <button
          type="button"
          className="btn"
          onClick={() => {
            signOut();
            setPrincipal(null);
          }}
        >
          Sign out
        </button>
      </header>
      <main>
        <nav>
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              className={t.id === tab ? "btn btn--primary" : "btn"}
              onClick={() => setTab(t.id)}
            >
              {t.label}
            </button>
          ))}
        </nav>
        {tab === "orders" && <OrdersScreen />}
        {tab === "menu" && <MenuScreen />}
        {tab === "suppliers" && <SuppliersScreen />}
        {tab === "receipts" && <GoodsReceiptsScreen />}
      </main>
    </>
  );
}
