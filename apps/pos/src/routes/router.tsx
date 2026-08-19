import { createRootRoute, createRoute, createRouter, Outlet, redirect } from "@tanstack/react-router";
import { useAuthStore } from "../store/auth";
import { LoginScreen } from "../components/LoginScreen";
import { PosScreen } from "../components/PosScreen";
import { OrderListScreen } from "../components/OrderListScreen";
import { BillingScreen } from "../components/BillingScreen";
import { CrashScreen } from "../components/CrashScreen";

const rootRoute = createRootRoute({
  component: () => <Outlet />,
});

const loginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/login",
  component: LoginScreen,
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
  component: PosScreen,
});

const ordersRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/orders",
  beforeLoad: requireAuth,
  component: OrderListScreen,
});

const billingRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/orders/$orderId/billing",
  beforeLoad: requireAuth,
  component: BillingScreen,
});

const routeTree = rootRoute.addChildren([loginRoute, posRoute, ordersRoute, billingRoute]);

// TanStack Router catches a throw inside a route component in its own
// CatchBoundary, BEFORE any React error boundary of ours can see it, and its
// stock error component renders "Something went wrong!" with no message and
// nothing copyable. Every screen in this app is a route component, so
// without this override that stock text is what a cashier would actually get
// — the outer ErrorBoundary would never fire. Verified by throwing in
// LoginScreen and observing which component rendered.
export const router = createRouter({
  routeTree,
  defaultErrorComponent: ({ error }) => <CrashScreen error={error} />,
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
