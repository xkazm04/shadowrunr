// The shell's print texture and entrance motion in jsdom, over stand-in pages: no grain on the
// light posters or the spread, and the layer-2 rise replaying on every tab and list-to-detail
// move (the page root remounts) while a deeper segment re-renders in place.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { PAGE_IDS, PAGE_PALETTES } from "@shadowrunr/tokens";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createDataLayer, DataProvider } from "../data";
import { contractDoor } from "../data/contractDoor";
import { fakeSockets, manualTimers, scriptedFetch } from "../data/testing";
import { createHashRouter, RouterProvider, type RouterStore } from "../router";
import { LIGHT_FACES } from "./posters";
import { RISE_DEPTH, riseKey } from "./rise";
import { Shell } from "./Shell";
import type { PageModule, PageRegistry } from "./types";

/** A page that renders its id and route, with a head and two blocks for the stagger. */
const stand = (id: string): PageModule => ({
  Layer1: () => <p>{id}</p>,
  Layer2: ({ sub }) => (
    <div data-role={`${id}-layer2`} data-sub={sub.join("/")}>
      <h2>{id}</h2>
      <p>{sub.join(" / ")}</p>
      <p>the rest</p>
    </div>
  ),
});
const PAGES: PageRegistry = {
  onair: stand("onair"),
  characters: stand("characters"),
  brain: stand("brain"),
  segments: stand("segments"),
  simulator: stand("simulator"),
  review: stand("review"),
  reconcile: stand("reconcile"),
};

const css = (f: string) => readFileSync(join(process.cwd(), "src/shell", f), "utf8");

let router: RouterStore;

function mount(hash: string) {
  window.location.hash = hash;
  router = createHashRouter(window);
  const data = createDataLayer({
    baseUrl: "http://127.0.0.1:7717",
    door: contractDoor,
    fetch: scriptedFetch(() => ({ network: true })),
    socket: fakeSockets().factory,
    timers: manualTimers(),
  });
  return render(
    <RouterProvider router={router}>
      <DataProvider layer={data}>
        <Shell pages={PAGES} playIntro={false} />
      </DataProvider>
    </RouterProvider>,
  );
}

async function go(hash: string) {
  await act(async () => {
    window.location.hash = hash;
    window.dispatchEvent(new HashChangeEvent("hashchange"));
  });
}

/** The open spread's page root once it has rendered. */
const pageRoot = (role: string, sub: string) =>
  waitFor(() => {
    const el = document.querySelector(`[data-role='spread'][data-open='true'] [data-role='${role}']`);
    expect(el?.getAttribute("data-sub"), `${role} at ${sub}`).toBe(sub);
    return el as Element;
  });

beforeEach(() => {
  window.location.hash = "";
});
afterEach(() => {
  cleanup();
  router.dispose();
});

/** WCAG relative luminance of a #rrggbb colour. */
function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  }) as [number, number, number];
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

describe("the print grain", () => {
  it("is off the light posters and on the dark ones", () => {
    mount("#/wall");
    const grain = Object.fromEntries([...document.querySelectorAll("[data-role='poster']")].map((p) => [p.getAttribute("data-page"), p.getAttribute("data-grain")]));
    expect(grain).toEqual({ onair: "true", characters: "false", brain: "true", segments: "false", simulator: "true", review: "false", reconcile: "true" });
  });

  it("calls light exactly the faces printed on light stock (luminance .4 and up)", () => {
    const light = PAGE_IDS.filter((id) => luminance(PAGE_PALETTES[id].face) >= 0.4);
    expect([...LIGHT_FACES].sort()).toEqual([...light].sort());
  });

  it("prints nothing on a light face, sits under the layer-1 body on an open dark poster, and is gone from the spread", () => {
    const poster = css("Poster.module.css");
    expect(poster).toContain('.poster[data-grain="false"]::after { opacity: 0; }');
    expect(poster).toContain('.poster[data-view="l1"][data-active="true"]::after { z-index: var(--sr-z-under); }');
    expect(css("Spread.module.css")).not.toMatch(/--sr-grain|::before/);
  });

  it("keeps the halftone from under body text on a light face: off the open layer 1, masked to the motif band on the spine", () => {
    const poster = css("Poster.module.css");
    expect(poster).toContain('.poster[data-grain="false"][data-view="l1"][data-active="true"] .halftoneB { opacity: 0; }');
    expect(poster).toMatch(/\.poster\[data-grain="false"\]\[data-view="l2"\]\[data-active="true"\]:not\(\[data-narrow="true"\]\) \.face \{\s*mask-image: linear-gradient\(transparent 15\.5rem, /);
  });
});

describe("the layer-2 entrance", () => {
  it("keys the page root on the route down to its view's depth", () => {
    expect(riseKey("brain", ["transcripts", "r1", "12"])).toBe("transcripts/r1");
    expect(riseKey("reconcile", ["report", "held"])).toBe("report");
    expect(riseKey("characters", ["malakor", "package", "situations", "s1"])).toBe("malakor/package/situations");
    expect(riseKey("onair", ["clips", "clip-1"])).toBe("clips/clip-1");
    expect(Object.keys(RISE_DEPTH).sort()).toEqual([...PAGE_IDS].sort());
  });

  it("replays on a tab change: the page root remounts", async () => {
    mount("#/brain/engine");
    const before = await pageRoot("brain-layer2", "engine");
    await go("#/brain/preflight");
    const after = await pageRoot("brain-layer2", "preflight");
    expect(after).not.toBe(before);
    expect(before.isConnected).toBe(false);
  });

  it("replays from a list to its detail", async () => {
    mount("#/simulator/tournaments");
    const list = await pageRoot("simulator-layer2", "tournaments");
    await go("#/simulator/tournaments/t1");
    const detail = await pageRoot("simulator-layer2", "tournaments/t1");
    expect(detail).not.toBe(list);
  });

  it("re-renders in place below the view's depth: a transcript line, a report filter", async () => {
    mount("#/brain/transcripts/r1");
    const run = await pageRoot("brain-layer2", "transcripts/r1");
    await go("#/brain/transcripts/r1/2");
    expect(await pageRoot("brain-layer2", "transcripts/r1/2")).toBe(run);

    await go("#/reconcile/report");
    const report = await pageRoot("reconcile-layer2", "report");
    await go("#/reconcile/report/held");
    expect(await pageRoot("reconcile-layer2", "report/held")).toBe(report);
  });

  it("staggers the blocks .08s apart up to .32s, nested containers after the head, and keeps the reduced-motion rule", () => {
    const spread = css("Spread.module.css");
    expect(spread).toMatch(/\.inner > :not\(:has\(\[data-rise\]\)\) > \*,\n\.spread\[data-open="true"\] \.inner \[data-rise\] > :not\(\[data-rise\]\) \{ animation: rise \.7s var\(--sr-ease-settle\) var\(--rise-at, 0s\) backwards; \}/);
    expect(spread).toMatch(/:nth-child\(2\) \{ animation-delay: calc\(var\(--rise-at, 0s\) \+ \.08s\); \}/);
    expect(spread).toMatch(/:nth-child\(n \+ 5\) \{ animation-delay: calc\(var\(--rise-at, 0s\) \+ \.32s\); \}/);
    expect(spread).toContain(".spread .inner [data-rise] [data-rise] { --rise-at: .16s; }");
    expect(spread).toMatch(/prefers-reduced-motion: reduce\) \{\s*\.spread,\s*\.spread \* \{ animation-duration: \.01s !important;/);
  });
});
