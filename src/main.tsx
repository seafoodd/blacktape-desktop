import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

import "./styles/reset.css";
import "./styles/variables.css";
import "./styles/globals.css";
import "./styles/utilities.css";
import { ThemeProvider } from "./shared/providers/theme-provider/ThemeProvider.tsx";
import { ModalHost } from "@/shared/ui/modal";
import { setupWindowFrame } from "./windowFrame.ts";

setupWindowFrame().catch((err) => console.error("Failed to set up the window frame:", err));

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider>
      <App />
      <ModalHost />
    </ThemeProvider>
  </React.StrictMode>,
);

// disable default webview right click menu
document.addEventListener("DOMContentLoaded", () => {
  document.addEventListener("contextmenu", (e) => {
    e.preventDefault();
  });
});
