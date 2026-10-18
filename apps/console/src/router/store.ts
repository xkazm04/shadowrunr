// The hash router: one store over `location.hash`, read through `decodeRoute`, written through
// `encodeRoute`. It also carries a transient push-in origin (the rect of the element that was
// clicked) so the shell can grow the nested page out of it (A/1's push-in).
import { decodeRoute, encodeRoute, sameRoute, type Route } from "./route";

export interface OriginRect {
  readonly left: number;
  readonly top: number;
  readonly width: number;
  readonly height: number;
}

export interface NavigateOptions {
  /** Replace the history entry instead of pushing one. */
  readonly replace?: boolean;
  /** Where the click happened, for the push-in transition. */
  readonly origin?: OriginRect;
}

export interface RouterStore {
  subscribe(listener: () => void): () => void;
  getRoute(): Route;
  navigate(route: Route, opts?: NavigateOptions): void;
  /** Returns and clears the origin recorded by the last navigate. */
  takeOrigin(): OriginRect | null;
  dispose(): void;
}

/** The parts of `window` the router needs; tests pass a fake. */
export interface HashHost {
  readonly location: { hash: string };
  readonly history: { replaceState(data: unknown, unused: string, url?: string | URL | null): void };
  addEventListener(type: "hashchange", listener: () => void): void;
  removeEventListener(type: "hashchange", listener: () => void): void;
}

export function createHashRouter(host: HashHost): RouterStore {
  const listeners = new Set<() => void>();
  let origin: OriginRect | null = null;
  let current = canonicalise();

  function canonicalise(): Route {
    const decoded = decodeRoute(host.location.hash);
    if (!decoded.ok || host.location.hash !== encodeRoute(decoded.route)) {
      // Unknown or non-canonical hashes are rewritten in place, never pushed.
      if (host.location.hash !== "" || !decoded.ok) host.history.replaceState(null, "", encodeRoute(decoded.route));
    }
    return decoded.route;
  }

  function emit() {
    for (const l of listeners) l();
  }

  const onHash = () => {
    const next = canonicalise();
    if (sameRoute(next, current)) return;
    current = next;
    emit();
  };
  host.addEventListener("hashchange", onHash);

  return {
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    getRoute: () => current,
    navigate(route, opts = {}) {
      origin = opts.origin ?? null;
      const hash = encodeRoute(route);
      if (sameRoute(route, current)) return;
      if (opts.replace) {
        host.history.replaceState(null, "", hash);
        current = route;
        emit();
        return;
      }
      // Setting the hash fires `hashchange`, which updates `current` through one path.
      host.location.hash = hash;
    },
    takeOrigin() {
      const o = origin;
      origin = null;
      return o;
    },
    dispose() {
      host.removeEventListener("hashchange", onHash);
      listeners.clear();
    },
  };
}
