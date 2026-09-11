import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { App } from "./App";
import "@holler/ui/tokens.css";
import "@holler/ui/base.css";
import "./index.css";

// One client for the app. Retries are OFF for mutations: a PATCH that failed
// with a 4xx will fail again the same way, and retrying a write nobody asked
// to retry is how a single edit becomes several.
const queryClient = new QueryClient({
  defaultOptions: {
    mutations: { retry: false },
    queries: { retry: 1, refetchOnWindowFocus: false },
  },
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </React.StrictMode>,
);
