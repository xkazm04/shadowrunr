// The fidelity harness (contest promotion step 3): the real page Layer1 components inside the
// real poster (shell/Wall at layer 1), with the real stylesheet and tokens, fed by the contest's
// real data through the data layer's own test doubles. Served by the console's Vite dev server
// (`node fidelity/run.mjs`), never built into the production bundle.
//
// The geometry is the winner's: no mixer bus under the posters (busRem 0), so the active poster
// is the full viewport tall like the winner's, and the root size is the one global.css computes.
import "@shadowrunr/tokens/tokens.css";
import "../src/app/global.css";
import contest from "virtual:shadowrunr-contest-data";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { PAGES } from "../src/app/pages";
import { contractDoor } from "../src/data/contractDoor";
import { createDataLayer, DataProvider, type FetchLike, type SocketFactory, type Timers } from "../src/data";
import { FakeSocket } from "../src/data/testing";
import { createHashRouter, RouterProvider, useRoute, type PageId } from "../src/router";
import { useViewport } from "../src/shell/useViewport";
import { Wall } from "../src/shell/Wall";
import { LIVE, selectSource } from "../src/pages/onair/feed";
import { contestRpc, liveScene } from "./contest";

const answer = contestRpc(contest);
/** A named state the page cannot reach from stored runs alone (`?scene=`, see roles.json). */
const scene = new URLSearchParams(window.location.search).get("scene");
const frames = scene === null ? [] : liveScene(contest, scene);
// The operator's Live pick: On Air plays the /events feed instead of a stored run.
if (frames.length > 0) selectSource(LIVE);

/** `POST /rpc` answered from the contest data; the reply text still goes through the door. */
const contestFetch: FetchLike = (_url, init) => {
  const sent: unknown = typeof init.body === "string" ? JSON.parse(init.body) : {};
  const body = typeof sent === "object" && sent !== null ? (sent as { method?: unknown; params?: unknown }) : {};
  const method = typeof body.method === "string" ? body.method : "?";
  const params = typeof body.params === "object" && body.params !== null ? (body.params as Record<string, unknown>) : {};
  const reply = answer(method, params);
  if ("network" in reply || "hang" in reply) return Promise.reject(new TypeError("Failed to fetch"));
  const status = reply.status ?? 200;
  return Promise.resolve({ ok: status < 400, status, text: () => Promise.resolve(JSON.stringify(reply.body)) });
};

/** Lets the lazily loaded page mount and subscribe before a scene's live frames arrive. */
const SCENE_DELAY_MS = 1200;

/**
 * The live stream: it opens, then plays the scene's frames (if any) once, and stays quiet. The
 * pages otherwise read the recorded runs, not a live show.
 */
const liveSocket: SocketFactory = (url) => {
  const s = new FakeSocket(url);
  window.setTimeout(() => {
    s.open();
    window.setTimeout(() => {
      if (s.closed === null) for (const f of frames) s.push(f);
    }, SCENE_DELAY_MS);
  }, 0);
  return s;
};

const timers: Timers = {
  setTimeout: (fn, ms) => window.setTimeout(fn, ms),
  clearTimeout: (h) => window.clearTimeout(h),
};

const noop = () => undefined;

function Harness() {
  const route = useRoute();
  const viewport = useViewport();
  const page: PageId = route.view === "page" ? route.page : "onair";
  return (
    <Wall
      pages={PAGES}
      view="l1"
      active={page}
      hover={null}
      onHover={noop}
      viewport={viewport}
      busRem={0}
      pre={false}
      still
      crumbs={[]}
      onOpen={noop}
      onBack={noop}
      registerHit={noop}
    />
  );
}

const root = document.getElementById("root");
if (root === null) throw new Error("harness.html has no #root");
const layer = createDataLayer({ baseUrl: "http://fidelity.invalid", door: contractDoor, fetch: contestFetch, socket: liveSocket, timers });
createRoot(root).render(
  <StrictMode>
    <RouterProvider router={createHashRouter(window)}>
      <DataProvider layer={layer}>
        <Harness />
      </DataProvider>
    </RouterProvider>
  </StrictMode>,
);
