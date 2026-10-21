// The composition root: real browser dependencies are chosen here and nowhere else.
import { createDataLayer, readDataConfig, shellDaemonUrl, type DataLayer, type SocketFactory, type Timers } from "../data";
import { createHashRouter, decodeRoute, type RouterStore } from "../router";

const browserTimers: Timers = {
  setTimeout: (fn, ms) => window.setTimeout(fn, ms),
  clearTimeout: (h) => window.clearTimeout(h),
};

const browserSocket: SocketFactory = (url) => new WebSocket(url);

export function composeDataLayer(): DataLayer {
  const env: Readonly<Record<string, string | undefined>> = import.meta.env;
  // The desktop shell's daemon wins (its window CSP admits no other); a browser uses ?daemon=.
  const config = readDataConfig(window.location.search, env, shellDaemonUrl());
  // The door's compiled validators load in parallel with the first paint (their own chunk).
  const door = import("../data/contractDoor").then((m) => m.contractDoor);
  return createDataLayer({ baseUrl: config.daemonUrl, door, socket: browserSocket, timers: browserTimers });
}

export function composeRouter(): RouterStore {
  return createHashRouter(window);
}

/** The intro plays on a fresh boot of the default route, unless motion is reduced. */
export function shouldPlayIntro(hash: string, reducedMotion: boolean): boolean {
  if (reducedMotion) return false;
  const d = decodeRoute(hash);
  return hash === "" || hash === "#/" || (d.ok && d.route.view === "page" && d.route.page === "onair" && d.route.sub.length === 0);
}
