import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import "@holler/ui/tokens.css";
import "@holler/ui/base.css";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
