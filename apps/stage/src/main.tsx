import "@fontsource-variable/cinzel";
import "@fontsource/ibm-plex-sans-condensed/400.css";
import "@fontsource/ibm-plex-sans-condensed/500.css";
import "@fontsource/ibm-plex-sans-condensed/600.css";
import "@fontsource/ibm-plex-sans-condensed/700.css";
import "@fontsource/ibm-plex-mono/500.css";
import "@shadowrunr/tokens/tokens.css";
import "./styles.css";
import "./screens.css";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { ClipApp } from "./ClipStage";

// `?clip=<url>`: one clip candidate on the vertical canvas (clip mode) instead of the broadcast.
const clipMode = new URLSearchParams(window.location.search).has("clip");

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {clipMode ? <ClipApp /> : <App />}
  </StrictMode>,
);
