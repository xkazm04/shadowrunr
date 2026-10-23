import "@shadowrunr/tokens/tokens.css";
import "./global.css";
import { MEDIA } from "@shadowrunr/tokens";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { startAgent } from "./agent/boot";
import { App } from "./App";
import { composeDataLayer, composeRouter, shouldPlayIntro } from "./compose";

const playIntro = shouldPlayIntro(window.location.hash, window.matchMedia(MEDIA.reducedMotion).matches);
const router = composeRouter();
const data = composeDataLayer();
startAgent({ router, data });
const root = document.getElementById("root");
if (root === null) throw new Error("index.html has no #root");

createRoot(root).render(
  <StrictMode>
    <App router={router} data={data} playIntro={playIntro} />
  </StrictMode>,
);
