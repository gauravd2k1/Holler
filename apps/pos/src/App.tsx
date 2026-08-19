import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { router } from "./routes/router";
import { ErrorBoundary } from "./components/ErrorBoundary";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Local edge SQLite via Tauri IPC, not a network fetch — no reason to
      // retry a failed read, and no reason to refetch a stale-not-really
      // read on every window focus.
      retry: false,
      refetchOnWindowFocus: false,
    },
  },
});

// The boundary wraps the provider, not the other way round: a throw inside
// QueryClientProvider's own subtree (which is everything) must still be
// caught, and the boundary itself must not depend on any context that the
// crash may have taken down with it.
export function App() {
  return (
    <ErrorBoundary>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </ErrorBoundary>
  );
}
