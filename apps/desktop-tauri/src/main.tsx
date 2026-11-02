import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";
import "./surfaces/settings/settings-layout.css";
import "./surfaces/menu-surface-layout.css";
import "./surfaces/tray-card-modern.css";
import "./surfaces/tray-card.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
