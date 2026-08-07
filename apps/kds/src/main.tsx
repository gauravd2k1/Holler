// Milestone 0: empty shell only. No business logic — see CLAUDE.md EXCLUDES.
// KDS station-card UI (§13) arrives in Milestone 2.
import React from "react";
import ReactDOM from "react-dom/client";

function App() {
  return (
    <main>
      <h1>Holler KDS</h1>
      <p>Milestone 0 shell — no business logic yet.</p>
    </main>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
