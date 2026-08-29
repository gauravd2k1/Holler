import { createRootRoute, createRoute, createRouter, Outlet, redirect } from "@tanstack/react-router";
import { useAuthStore } from "../store/auth";
import { LoginScreen } from "../components/LoginScreen";
import { PosScreen } from "../components/PosScreen";
import { OrderListScreen } from "../components/OrderListScreen";
import { BillingScreen } from "../components/BillingScreen";
import { CrashScreen } from "../components/CrashScreen";
import { ErrorBoundary } from "../components/ErrorBoundary";
import { CurrentStockScreen } from "../components/CurrentStockScreen";
import { WastageScreen } from "../components/WastageScreen";
import { StockCountListScreen, StockCountScreen } from "../components/StockCountScreen";
import { StockDeductionGapsScreen } from "../components/StockDeductionGapsScreen";
import { ReceivingScreen } from "../components/ReceivingScreen";
import { PurchaseReturnScreen } from "../components/PurchaseReturnScreen";
import { GrnGapsScreen } from "../components/GrnGapsScreen";

/** Wraps a screen so OUR boundary sees the throw before the router's
 * CatchBoundary does.
 *
 * Why bother, when `defaultErrorComponent` below already renders the same
 * CrashScreen: the router's error component receives the error but NOT a
 * component stack. Its props type accepts `info?.componentStack`, and
 * TypeScript compiles it happily, but at runtime nothing arrives — verified
 * by throwing in LoginScreen and reading what actually rendered. Only
 * React's own `componentDidCatch` supplies the stack, so the only way to get
 * "which component died" onto the screen is a real boundary inside the
 * route. */
function withBoundary(Screen: () => JSX.Element) {
  return function BoundedScreen() {
    return (
      <ErrorBoundary>
        <Screen />
      </ErrorBoundary>
    );
  };
}

const rootRoute = createRootRoute({
  component: () => <Outlet />,
});

const loginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/login",
  component: withBoundary(LoginScreen),
});

/** A trained cashier's screen must not be reachable without an offline
 * login having succeeded (task requirement #1). */
function requireAuth(): void {
  if (!useAuthStore.getState().principal) {
    throw redirect({ to: "/login" });
  }
}

const posRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  beforeLoad: requireAuth,
  component: withBoundary(PosScreen),
});

const ordersRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/orders",
  beforeLoad: requireAuth,
  component: withBoundary(OrderListScreen),
});

const billingRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/orders/$orderId/billing",
  beforeLoad: requireAuth,
  component: withBoundary(BillingScreen),
});

// ------------------------------------------------------------ inventory (M4) --

const currentStockRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/inventory/stock",
  beforeLoad: requireAuth,
  component: withBoundary(CurrentStockScreen),
});

const wastageRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/inventory/wastage",
  beforeLoad: requireAuth,
  component: withBoundary(WastageScreen),
});

const stockCountListRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/inventory/counts",
  beforeLoad: requireAuth,
  component: withBoundary(StockCountListScreen),
});

const stockCountRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/inventory/counts/$stockCountId",
  beforeLoad: requireAuth,
  component: withBoundary(StockCountScreen),
});

const stockDeductionGapsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/inventory/gaps",
  beforeLoad: requireAuth,
  component: withBoundary(StockDeductionGapsScreen),
});

// --------------------------------------------------------- procurement (M5) --
// ADR-019, track T4. Receiving, returns and the human-visible GRN gap report.
// `/procurement/gaps` is deliberately its OWN route rather than a section of
// `/inventory/gaps`: a stock-deduction gap and a delivery gap are different
// events with different audiences and different next steps, and folding them
// into one screen is how eight distinct reasons end up under one heading —
// the filed M4 defect this milestone must not repeat.

const receivingRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/procurement/receive",
  beforeLoad: requireAuth,
  component: withBoundary(ReceivingScreen),
});

const purchaseReturnRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/procurement/returns",
  beforeLoad: requireAuth,
  component: withBoundary(PurchaseReturnScreen),
});

const grnGapsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/procurement/gaps",
  beforeLoad: requireAuth,
  component: withBoundary(GrnGapsScreen),
});

const routeTree = rootRoute.addChildren([
  loginRoute,
  posRoute,
  ordersRoute,
  billingRoute,
  currentStockRoute,
  wastageRoute,
  stockCountListRoute,
  stockCountRoute,
  stockDeductionGapsRoute,
  receivingRoute,
  purchaseReturnRoute,
  grnGapsRoute,
]);

// TanStack Router catches a throw inside a route component in its own
// CatchBoundary, BEFORE any React error boundary of ours can see it, and its
// stock error component renders "Something went wrong!" with no message and
// nothing copyable. Every screen in this app is a route component, so
// without this override that stock text is what a cashier would actually get
// — the outer ErrorBoundary would never fire. Verified by throwing in
// LoginScreen and observing which component rendered.
export const router = createRouter({
  routeTree,
  defaultErrorComponent: ({ error, info }) => (
    <CrashScreen error={error} componentStack={info?.componentStack ?? null} />
  ),
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
