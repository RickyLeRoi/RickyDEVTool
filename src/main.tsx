import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { post } from "./lib/api";
import "./styles.css";

// Gli errori JS del frontend finiscono nel log del backend.
window.addEventListener("error", (e) => {
  post("/api/log", {
    level: "error",
    message: e.message,
    stack: e.error instanceof Error ? e.error.stack : undefined,
  });
});
window.addEventListener("unhandledrejection", (e) => {
  post("/api/log", { level: "error", message: `unhandled rejection: ${e.reason}` });
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
