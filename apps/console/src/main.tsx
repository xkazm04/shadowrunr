import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ApiProvider, readConfig } from "./api/context";
import { App } from "./App";
import { DeskProvider, VIEWS, type ViewId } from "./state";
import "./styles/tokens.css";
import "./styles/app.css";

const config = readConfig();
const hashView = window.location.hash.slice(1) as ViewId;
const initial = VIEWS.some((v) => v.id === hashView) ? hashView : "preflight";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ApiProvider config={config}>
      <DeskProvider initial={initial}>
        <App />
      </DeskProvider>
    </ApiProvider>
  </StrictMode>,
);
