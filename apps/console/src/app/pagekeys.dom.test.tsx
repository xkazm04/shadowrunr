// Page keys through the real shell: the review player's marks and the roster's arrows are scoped
// bindings in the one keymap (keys/keymap.ts), resolved innermost-scope-first by the shell's one
// listener, and a page command key ('r') is taken only while a page owns it. Card "Page keys
// bypass the one keymap: scoped bindings the conflict report sees" (challenge-2026-10-21).
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { createDataLayer } from "../data";
import { contractDoor } from "../data/contractDoor";
import { fakeSockets, manualTimers, scriptedFetch } from "../data/testing";
import { fail, ok, settle, type Script } from "../kit/testing";
import { genRoster, malakorDetail } from "../pages/characters/testgen";
import { derive } from "../pages/review/derive";
import { reviewSession, SESSION } from "../pages/review/tabs.fixture";
import { createHashRouter, pageRoute, type RouterStore } from "../router";
import { App } from "./App";

let router: RouterStore | undefined;
afterEach(() => {
  cleanup();
  router?.dispose();
  router = undefined;
});

function mount(hash: string, script: Script) {
  window.location.hash = hash;
  router = createHashRouter(window);
  const data = createDataLayer({
    baseUrl: "http://127.0.0.1:7717",
    door: contractDoor,
    fetch: scriptedFetch((method, sent) => script(method, typeof sent === "string" ? sent : "")),
    socket: fakeSockets().factory,
    timers: manualTimers(),
  });
  render(<App router={router} data={data} playIntro={false} />);
  return router;
}

/** Presses a key on `target` (bubbling to the shell's window listener); true when nothing prevented it. */
async function press(key: string, target: Element | null, mods: { ctrlKey?: boolean } = {}): Promise<boolean> {
  let passed = true;
  await act(async () => {
    passed = fireEvent.keyDown(target ?? document.body, { key, ...mods });
    await new Promise((r) => setTimeout(r, 0));
  });
  await settle(4);
  return passed;
}

const route = () => router?.getRoute();

describe("the review player's keys, through the shell", () => {
  const rs = reviewSession();
  const roasts = derive(rs).says.filter((s) => s.kind === "roast").map((s) => s.line);
  const sent: string[] = [];
  const script: Script = (method, body) => {
    if (method === "review.session") return ok(rs);
    if (method === "review.sessions" || method === "review.feedback.list") return ok([]);
    if (method === "review.feedback.set") {
      sent.push(body);
      const line = Number(/"line":(\d+)/.exec(body)?.[1]);
      return ok({ session_id: SESSION, line, tags: [], at_ms: 1 });
    }
    return fail("method_not_found", method);
  };
  const PLAYER = pageRoute("review", SESSION, "player", "roast");
  const player = () => document.querySelector<HTMLElement>("[data-role='review-player']");
  const current = () => Number(player()?.getAttribute("data-line"));
  const pressed = (name: RegExp) => screen.getByRole("button", { name }).getAttribute("aria-pressed");
  const inPlayer = <E extends HTMLElement>(el: E): E => {
    player()?.querySelector("[data-role='review-player-step']")?.append(el);
    return el;
  };
  async function open() {
    sent.length = 0;
    mount(`#/review/${SESSION}/player/roast`, script);
    await waitFor(() => expect(player()).not.toBeNull(), { timeout: 4000 });
    await settle(4);
  }

  it("toggles a mark for a key inside the player, and jumps pages for the same key outside it", async () => {
    await open();
    expect(player()?.getAttribute("data-key-scope")).toBe("review.player");
    expect(document.activeElement).toBe(player());
    expect(await press("3", player())).toBe(false);
    expect(pressed(/Too mean/)).toBe("true");
    expect(route()).toEqual(PLAYER);
    expect(await press("3", document.body)).toBe(false);
    expect(route()).toEqual(pageRoute("brain"));
  });

  it("decides typing with the one rule: a checkbox takes marks, the video and a text field do not", async () => {
    await open();
    const box = inPlayer(Object.assign(document.createElement("input"), { type: "checkbox" }));
    expect(await press("1", box)).toBe(false);
    expect(pressed(/Landed/)).toBe("true");
    const video = inPlayer(document.createElement("video"));
    expect(await press("2", video)).toBe(true);
    expect(pressed(/Flat/)).toBe("false");
    expect(route()).toEqual(PLAYER);
    const text = inPlayer(Object.assign(document.createElement("input"), { type: "text" }));
    expect(await press("4", text)).toBe(true);
    expect(pressed(/Broke character/)).toBe("false");
    expect(route()).toEqual(PLAYER);
    expect(sent).toHaveLength(0);
  });

  it("keeps every player key: Enter saves and moves on, arrows skip and go back, the note takes only Ctrl+Enter", async () => {
    await open();
    expect(current()).toBe(roasts[0]);
    expect(await press("ArrowRight", player())).toBe(false);
    expect(current()).toBe(roasts[1]);
    expect(await press("ArrowLeft", player())).toBe(false);
    expect(current()).toBe(roasts[0]);
    const note = document.querySelector("textarea");
    expect(await press("1", note)).toBe(true);
    expect(await press("Enter", note)).toBe(true);
    expect(await press("ArrowRight", note)).toBe(true);
    expect(pressed(/Landed/)).toBe("false");
    expect(current()).toBe(roasts[0]);
    expect(sent).toHaveLength(0);
    expect(await press("Enter", note, { ctrlKey: true })).toBe(false);
    expect(sent).toHaveLength(1);
    expect(current()).toBe(roasts[1]);
    expect(await press("Enter", player())).toBe(false);
    expect(sent).toHaveLength(2);
    // A modifier other than Ctrl+Enter has no binding: left alone.
    expect(await press("2", player(), { ctrlKey: true })).toBe(true);
    expect(route()).toEqual(PLAYER);
  });
});

describe("the roster's arrows, through the shell", () => {
  const script: Script = (method) => (method === "character.list" ? ok(genRoster(3)) : method === "character.get" ? ok(malakorDetail()) : fail("method_not_found", method));
  const roster = () => document.querySelector<HTMLElement>("[data-role='characters-roster']");

  it("step the roster inside its scope; outside it the arrows still step pages", async () => {
    mount("#/characters", script);
    await waitFor(() => expect(roster()?.querySelectorAll("[data-role='characters-roster-item']")).toHaveLength(3), { timeout: 4000 });
    await settle(4);
    expect(roster()?.getAttribute("data-key-scope")).toBe("characters.roster");
    const item = roster()?.querySelector("[data-role='characters-roster-item']") ?? null;
    expect(await press("ArrowRight", item)).toBe(false);
    expect(document.querySelector("[data-role='characters-nameplate']")?.getAttribute("href")).toBe("#/characters/char-0001");
    expect(route()).toEqual(pageRoute("characters"));
    expect(await press("ArrowRight", document.body)).toBe(false);
    expect(route()).toEqual(pageRoute("brain"));
  });
});

describe("a page command key", () => {
  it("is left alone where no page owns it, and re-runs Brain's pre-flight where it does", async () => {
    let runs = 0;
    const script: Script = (method) => {
      if (method !== "preflight.run") return fail("method_not_found", method);
      runs += 1;
      return ok([{ check: "PF-01", state: "green", detail: "ok" }]);
    };
    mount("#/segments/tables", script);
    await settle(8);
    expect(await press("r", document.body)).toBe(true);
    expect(route()).toEqual(pageRoute("segments", "tables"));
    cleanup();
    router?.dispose();

    mount("#/brain/preflight", script);
    await waitFor(() => expect(document.querySelector("[data-role='brain-preflight-last']")?.textContent).toContain("Last run"), { timeout: 4000 });
    const before = runs;
    expect(await press("r", document.body)).toBe(false);
    await settle(8);
    expect(runs).toBe(before + 1);
  });
});
