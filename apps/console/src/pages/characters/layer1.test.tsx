import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import DOM_BUDGETS from "../../../budgets/dom.json";
import { fail, mountPage, nodes, ok, role, roles, settle, type Script } from "./harness";
import { Layer1 } from "./Layer1";
import { Glance, Motif } from "./Poster";
import { genRoster, malakorDetail, malakorSummary } from "./testgen";

const BUDGET: number = DOM_BUDGETS.routes["wall+l1"];

let dispose: (() => void) | undefined;
afterEach(() => {
  cleanup();
  dispose?.();
});

const daemon =
  (roster = [malakorSummary()]): Script =>
  (method) =>
    method === "character.list" ? ok(roster) : method === "character.get" ? ok(malakorDetail()) : fail("not_found", method);

describe("layer 1: the cast poster", () => {
  it("renders nothing while hidden", () => {
    const m = mountPage(<Layer1 active={false} />, daemon());
    dispose = m.dispose;
    expect(role("characters-layer1")).toBeNull();
  });

  it("prints the name plate, portrait, tone, roster and foils from character.list and character.get", async () => {
    const m = mountPage(<Layer1 active />, daemon());
    dispose = m.dispose;
    await settle();
    const plate = role("characters-nameplate");
    expect(plate?.textContent).toContain("01 of 1 · the first character");
    expect([...(plate?.querySelectorAll("i") ?? [])].map((i) => i.textContent)).toEqual(["Arch-Lich", "Malakor"]);
    expect(plate?.getAttribute("href")).toBe("#/characters/malakor");
    expect(role("characters-portrait")?.querySelector("svg.mk")).toBeTruthy();
    expect(role("characters-portrait-arch")).toBeTruthy();
    expect(role("characters-tone")?.textContent).toContain("Theatrical");
    expect(role("characters-tone")?.getAttribute("href")).toBe("#/characters/malakor/package/tones");
    const items = roles("characters-roster-item");
    expect(items).toHaveLength(1);
    expect(items[0]?.getAttribute("aria-selected")).toBe("true");
    expect(items[0]?.querySelector("svg.mk")).toBeTruthy();
    expect(role("characters-roster")?.textContent).toContain("Your character is a folder");
    const foils = role("characters-foils");
    expect(foils?.querySelectorAll("svg.foil-art")).toHaveLength(5);
    expect(foils?.textContent).toContain("Has-Been Hero");
    expect(foils?.getAttribute("href")).toBe("#/characters/malakor/package/foils");
    expect(role("characters-cta")?.getAttribute("href")).toBe("#/characters/malakor/package/constitution");
    expect(role("characters-state")).toBeNull();
  });

  it("shows the offline slip when the daemon is unreachable, and the empty cast when there is none", async () => {
    const off = mountPage(<Layer1 active />, () => ({ network: true }));
    await settle();
    expect(role("characters-state")?.textContent).toContain("not reachable");
    off.dispose();
    cleanup();
    const empty = mountPage(<Layer1 active />, daemon([]));
    dispose = empty.dispose;
    await settle();
    expect(role("characters-state")?.textContent).toContain("No characters yet");
    expect(role("characters-cta")?.getAttribute("href")).toBe("#/characters/all");
    expect(roles("characters-roster-item")).toHaveLength(0);
  });

  it("holds 2,000 characters in the DOM budget: a window of 19, a +N tile, arrows move the window", async () => {
    const roster = genRoster(2000);
    const m = mountPage(<Layer1 active />, daemon(roster));
    dispose = m.dispose;
    await settle();
    expect(roles("characters-roster-item")).toHaveLength(19);
    expect(role("characters-roster-more")?.textContent).toContain("+1,981");
    expect(role("characters-roster-more")?.getAttribute("href")).toBe("#/characters/all");
    expect(nodes()).toBeLessThan(BUDGET);
    const box = role("characters-roster");
    if (!box) throw new Error("no roster");
    for (let i = 0; i < 12; i++) fireEvent.keyDown(box, { key: "ArrowLeft" });
    await settle(1);
    expect(role("characters-nameplate")?.textContent).toContain("1,989 of 2,000");
    const sel = roles("characters-roster-item").filter((b) => b.getAttribute("aria-selected") === "true");
    expect(sel).toHaveLength(1);
    expect(document.activeElement).toBe(sel[0]);
    expect(nodes()).toBeLessThan(BUDGET);
  });

  it("selects a character by click and points every link at it", async () => {
    const m = mountPage(<Layer1 active />, daemon(genRoster(3)));
    dispose = m.dispose;
    await settle();
    const second = roles("characters-roster-item")[1];
    await act(async () => {
      second?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(role("characters-nameplate")?.getAttribute("href")).toBe("#/characters/char-0001");
    expect(role("characters-portrait")?.querySelector("svg.mk")).toBeNull();
    expect(role("characters-portrait")?.querySelector("svg[aria-label='Stylised avatar']")).toBeTruthy();
  });

  it("draws the poster motif and the glance", async () => {
    const m = mountPage(
      <>
        <Motif />
        <Glance />
      </>,
      daemon(genRoster(20)),
    );
    dispose = m.dispose;
    expect(role("characters-motif")).toBeTruthy();
    await settle();
    expect(screen.getByText("20 characters")).toBeTruthy();
    expect(screen.getByText("5 foils")).toBeTruthy();
  });
});
