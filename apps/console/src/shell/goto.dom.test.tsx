// Ctrl+K in the mounted shell: the palette mounts only while open, lands a pasted ref through the
// router, and Escape closes it without also going up a level.
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createDataLayer, DataProvider } from "../data";
import { contractDoor } from "../data/contractDoor";
import { fakeSockets, manualTimers, scriptedFetch } from "../data/testing";
import { createHashRouter, RouterProvider, type RouterStore } from "../router";
import { BRAIN_ROUTES } from "../router/brain";
import { Shell } from "./Shell";
import type { PageModule, PageRegistry } from "./types";

const stand = (id: string, routes?: PageModule["routes"]): PageModule => ({
  Layer1: () => <p>{id}</p>,
  Layer2: ({ sub }) => <div data-role={`${id}-layer2`} data-sub={sub.join("/")} />,
  ...(routes ? { routes } : {}),
});
const PAGES: PageRegistry = {
  onair: stand("onair"),
  characters: stand("characters"),
  brain: stand("brain", BRAIN_ROUTES),
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
        <Shell pages={PAGES} playIntro={false} />
      </DataProvider>
    </RouterProvider>,
  );
}

const palette = () => document.querySelector("[data-role='goto']");
const field = () => document.querySelector<HTMLInputElement>("[data-role='goto'] input");
const ctrlK = () =>
  act(async () => {
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    await new Promise((r) => setTimeout(r, 0));
  });
const openPalette = async () => {
  await ctrlK();
  await waitFor(() => expect(field()).not.toBeNull());
  return field() as HTMLInputElement;
};

beforeEach(() => {
  window.location.hash = "";
});
afterEach(() => {
  cleanup();
  router.dispose();
});

describe("Go to (Ctrl+K)", () => {
  it("lands a pasted qualified ref at its line, closes, and focuses the spread", async () => {
    mount("#/review/s1");
    await waitFor(() => expect(document.querySelector("[data-role='review-layer2']")).not.toBeNull());
    const input = await openPalette();
    expect(document.activeElement).toBe(input);
    await act(async () => {
      fireEvent.change(input, { target: { value: "t2/r7#3" } });
    });
    await act(async () => {
      fireEvent.keyDown(input, { key: "Enter" });
      await new Promise((r) => setTimeout(r, 0));
    });
    await waitFor(() => expect(window.location.hash).toBe("#/brain/transcripts/t2%2Fr7/3"));
    await waitFor(() => expect(palette()).toBeNull());
    await waitFor(() => expect(document.activeElement?.getAttribute("data-role")).toBe("spread"));
  });

  it("closes on Escape without also going up a level", async () => {
    mount("#/review/s1");
    const input = await openPalette();
    await act(async () => {
      fireEvent.keyDown(input, { key: "Escape" });
      await new Promise((r) => setTimeout(r, 0));
    });
    expect(palette()).toBeNull();
    expect(window.location.hash).toBe("#/review/s1");
  });

  it("goes back to the previous place on Ctrl+K, Enter", async () => {
    mount("#/brain/engine");
    await act(async () => {
      window.location.hash = "#/review/s1";
      window.dispatchEvent(new HashChangeEvent("hashchange"));
    });
    const input = await openPalette();
    await act(async () => {
      fireEvent.keyDown(input, { key: "Enter" });
      await new Promise((r) => setTimeout(r, 0));
    });
    await waitFor(() => expect(window.location.hash).toBe("#/brain/engine"));
  });

  it("(guard) renders no palette node while closed", async () => {
    mount("#/review/s1");
    await waitFor(() => expect(document.querySelector("[data-role='review-layer2']")).not.toBeNull());
    expect(palette()).toBeNull();
  });
});
