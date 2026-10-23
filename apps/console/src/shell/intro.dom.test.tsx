// The intro's landing: it ends on On Air only when the route is still the boot route (the wall
// an empty hash decodes to). A person or an agent who moved during it stays where they went
// (the live pilot's go_to("#/segments") landed on #/onair before this).
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createDataLayer, DataProvider } from "../data";
import { contractDoor } from "../data/contractDoor";
import { fakeSockets, manualTimers, scriptedFetch } from "../data/testing";
import { createHashRouter, pageRoute, RouterProvider, type RouterStore } from "../router";
import { INTRO } from "./Intro";
import { Shell } from "./Shell";
import type { PageModule, PageRegistry } from "./types";

const stand = (id: string): PageModule => ({ Layer1: () => <p>{id}</p>, Layer2: () => <div data-role={`${id}-layer2`} /> });
const PAGES: PageRegistry = {
  onair: stand("onair"),
  characters: stand("characters"),
  brain: stand("brain"),
  segments: stand("segments"),
  simulator: stand("simulator"),
  review: stand("review"),
  reconcile: stand("reconcile"),
};

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
        <Shell pages={PAGES} playIntro={true} />
      </DataProvider>
    </RouterProvider>,
  );
}

/** Moves the console the way go_to does, then delivers the hashchange jsdom queues on a (faked) timer. */
const move = (route: Parameters<RouterStore["navigate"]>[0]) =>
  act(() => {
    router.navigate(route);
    window.dispatchEvent(new HashChangeEvent("hashchange"));
  });

/** Lets the intro run out. */
const endIntro = () =>
  act(() => {
    vi.advanceTimersByTime(INTRO.endMs + 10);
  });

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  window.location.hash = "";
});
afterEach(() => {
  cleanup();
  router.dispose();
  vi.useRealTimers();
});

describe("the intro", () => {
  it("lands on On Air from the boot route", async () => {
    mount("");
    expect(router.getRoute().view).toBe("wall");
    await endIntro();
    expect(router.getRoute()).toEqual(pageRoute("onair"));
  });

  it("ends where an agent moved the console during it, not on On Air", async () => {
    mount("");
    move(pageRoute("segments"));
    expect(router.getRoute()).toEqual(pageRoute("segments"));
    await endIntro();
    expect(router.getRoute()).toEqual(pageRoute("segments"));
    expect(window.location.hash).toBe("#/segments");
  });

  it("keeps a deep view a person opened during it", async () => {
    mount("#/onair");
    move(pageRoute("review", "sbx-1", "timeline", "12"));
    await endIntro();
    expect(router.getRoute()).toEqual(pageRoute("review", "sbx-1", "timeline", "12"));
  });
});
