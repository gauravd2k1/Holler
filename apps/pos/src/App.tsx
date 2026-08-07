import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { router } from "./routes/router";

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

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}
