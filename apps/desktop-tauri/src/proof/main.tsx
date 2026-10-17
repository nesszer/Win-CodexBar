import React from "react";
import ReactDOM from "react-dom/client";
import { installProofMocks } from "./install";
import { parseProofParams } from "./params";
import "../styles.css";
import "../surfaces/settings/settings-layout.css";
import "../surfaces/menu-surface-layout.css";
import "../surfaces/tray-card.css";

installProofMocks(parseProofParams(window.location.search));

// App is loaded only after the mocks exist so no module sees the bare IPC.
void import("../App").then(({ default: App }) => {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
});
