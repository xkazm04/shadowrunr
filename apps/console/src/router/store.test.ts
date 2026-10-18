import { describe, expect, it } from "vitest";
import { pageRoute, WALL } from "./route";
import { createHashRouter, type HashHost } from "./store";

function fakeHost(initial: string) {
  const listeners = new Set<() => void>();
  const pushes: string[] = [];
  const replaces: string[] = [];
  const location = {
    _hash: initial,
    get hash() {
      return this._hash;
    },
    set hash(h: string) {
      this._hash = h;
      pushes.push(h);
      for (const l of listeners) l();
    },
  };
  const host: HashHost = {
    location,
    history: {
      replaceState: (_d, _u, url) => {
        location._hash = String(url);
        replaces.push(String(url));
      },
    },
    addEventListener: (_t, l) => listeners.add(l),
    removeEventListener: (_t, l) => listeners.delete(l),
  };
  return { host, pushes, replaces, listeners };
}

describe("hash router store", () => {
  it("reads the initial route and leaves an empty hash for the intro", () => {
    const f = fakeHost("");
    const r = createHashRouter(f.host);
    expect(r.getRoute()).toEqual(WALL);
    expect(f.replaces).toEqual([]);
  });

  it("rewrites an unknown hash to the wall in place, never pushing", () => {
    const f = fakeHost("#/nowhere/7");
    const r = createHashRouter(f.host);
    expect(r.getRoute()).toEqual(WALL);
    expect(f.replaces).toEqual(["#/wall"]);
    expect(f.pushes).toEqual([]);
  });

  it("navigates through the hash and notifies once", () => {
    const f = fakeHost("#/wall");
    const r = createHashRouter(f.host);
    let n = 0;
    r.subscribe(() => n++);
    r.navigate(pageRoute("simulator", "heatmap", "a/b"));
    expect(f.pushes).toEqual(["#/simulator/heatmap/a%2Fb"]);
    expect(r.getRoute()).toEqual(pageRoute("simulator", "heatmap", "a/b"));
    expect(n).toBe(1);
    r.navigate(pageRoute("simulator", "heatmap", "a/b"));
    expect(n).toBe(1); // same route: no push, no render
  });

  it("replaces without a history entry", () => {
    const f = fakeHost("");
    const r = createHashRouter(f.host);
    r.navigate(pageRoute("onair"), { replace: true });
    expect(f.replaces).toEqual(["#/onair"]);
    expect(r.getRoute()).toEqual(pageRoute("onair"));
  });

  it("hands the push-in origin out once", () => {
    const f = fakeHost("#/brain");
    const r = createHashRouter(f.host);
    r.navigate(pageRoute("brain", "transcripts"), { origin: { left: 1, top: 2, width: 3, height: 4 } });
    expect(r.takeOrigin()).toEqual({ left: 1, top: 2, width: 3, height: 4 });
    expect(r.takeOrigin()).toBeNull();
  });

  it("removes its listener on dispose", () => {
    const f = fakeHost("#/wall");
    const r = createHashRouter(f.host);
    expect(f.listeners.size).toBe(1);
    r.dispose();
    expect(f.listeners.size).toBe(0);
  });
});
