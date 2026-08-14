import { createRootRoute, createRoute, createRouter, Outlet, redirect } from "@tanstack/react-router";
import { useAuthStore } from "../store/auth";
import { LoginScreen } from "../components/LoginScreen";
import { PosScreen } from "../components/PosScreen";
import { OrderListScreen } from "../components/OrderListScreen";
import { BillingScreen } from "../components/BillingScreen";

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

export const router = createRouter({ routeTree });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
