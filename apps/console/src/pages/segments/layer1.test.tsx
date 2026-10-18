import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { Glance, Mini, Motif } from "./Face";
import { mountPage, nodes, ok, settle, type Script } from "./harness";
import { Layer1 } from "./Layer1";
import { DESCOPED_SHOWN, TICKS_PER_COLUMN } from "./model";
import { genRanks, genTables } from "./testgen";

let dispose: (() => void) | undefined;
afterEach(() => {
  cleanup();
  dispose?.();
  dispose = undefined;
});

const daemon =
  (tables = genTables()): Script =>
  (method) =>
    method === "segments.tables" ? ok(tables) : method === "ranks.table" ? ok(genRanks()) : ok({});

const role = (r: string) => document.querySelector(`[data-role='${r}']`);
const all = (r: string) => [...document.querySelectorAll(`[data-role='${r}']`)];

function mount(script: Script, active = true) {
  const m = mountPage(<Layer1 active={active} />, script);
  dispose = () => m.router.dispose();
  return m;
}

describe("layer 1: the programme poster", () => {
  it("renders nothing while hidden", () => {
    mount(daemon(), false);
    expect(role("segments-layer1")).toBeNull();
  });

  it("prints the lifecycle columns, with the pilots ticked and descoped on the siding, not retired", async () => {
    mount(daemon());
    await settle();
    const cols = all("segments-lc");
    expect(cols.map((c) => c.getAttribute("data-status"))).toEqual(["pilot", "trial", "regular", "flagship", "retired"]);
    expect(cols.map((c) => c.getAttribute("data-empty"))).toEqual(["false", "true", "true", "true", "true"]);
    expect(all("segments-tick").map((t) => t.textContent)).toEqual(["SEG-01Improvised Room", "SEG-02AHexmap Investigation"]);
    expect(cols[4]?.textContent).toBe("retirednone yet");
    expect(all("segments-descoped-hex").map((h) => h.getAttribute("aria-label"))).toEqual(["SEG-02B-I Cartography Legion: descoped", "SEG-02B-II Dependency Doctrine: descoped"]);
    expect(role("segments-descoped")?.textContent).toContain("descoped · 2 · the runtime refuses to start them");
    expect(all("segments-station").map((s) => s.getAttribute("data-lit"))).toEqual(["true", "false", "false", "false", "false"]);
    expect(role("segments-state")).toBeNull();
  });

  it("prints the commands and the pyramid, apex first, with the !propose ledge", async () => {
    mount(daemon());
    await settle();
    expect(all("segments-command").map((c) => c.textContent)).toEqual(["!summon", "!hint", "!grade", "!propose", "!vote"]);
    const steps = all("segments-step");
    expect(steps.map((s) => s.getAttribute("data-rank"))).toEqual(["dread_council", "lich", "warlock", "cultist", "acolyte", "thrall"]);
    expect(steps.map((s) => (s as HTMLElement).style.getPropertyValue("--w"))).toEqual(["28%", "42%", "56%", "70%", "84%", "98%"]);
    expect(steps[0]?.textContent).toBe("Dread Council×2.4");
    expect(steps[2]?.textContent).toBe("Warlock×2.0!propose from Warlock up");
    expect(steps.map((s) => s.getAttribute("data-proposes"))).toEqual(["true", "true", "true", "false", "false", "false"]);
    expect(role("segments-propose-ledge")?.textContent).toBe("!propose from Warlock up");
    expect(role("segments-propose-ledge")?.closest("[data-role='segments-step']")?.getAttribute("data-rank")).toBe("warlock");
  });

  it("links each lit block into its layer-2 page", async () => {
    mount(daemon());
    await settle();
    expect(role("segments-lifecycle")?.getAttribute("href")).toBe("#/segments/tables");
    expect(role("segments-commands")?.getAttribute("href")).toBe("#/segments/commands");
    expect(role("segments-pyramid")?.getAttribute("href")).toBe("#/segments/ranks");
    expect(role("segments-cta")?.getAttribute("href")).toBe("#/segments/tables");
  });

  it("says it is reading, then offline with Retry, then recovers", async () => {
    let up = false;
    const m = mount((method, sent) => (up ? daemon()(method, sent) : { network: true }));
    expect(role("segments-state")?.textContent).toBe("Reading the segment tables…");
    await settle();
    expect(role("segments-state")?.textContent).toContain("Retry");
    expect(all("segments-tick")).toHaveLength(0);
    expect(all("segments-lc").every((c) => c.getAttribute("data-empty") === "false")).toBe(true);
    up = true;
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Retry" })));
    await settle();
    expect(m.fetch.calls.filter((c) => c === "segments.tables").length).toBeGreaterThan(1);
    expect(role("segments-state")).toBeNull();
    expect(all("segments-tick")).toHaveLength(2);
  });

  it("holds its DOM flat at scale: capped ticks, capped siding", async () => {
    mount(daemon(genTables(2_000)));
    await settle();
    expect(all("segments-tick").length).toBeLessThanOrEqual(5 * TICKS_PER_COLUMN);
    expect(all("segments-tick-more")).toHaveLength(5);
    expect(all("segments-descoped-hex")).toHaveLength(DESCOPED_SHOWN);
    expect(nodes()).toBeLessThan(260);
  });
});

describe("the poster face", () => {
  it("draws the motif and the mini, and glances at the lifecycle", async () => {
    const m = mountPage(
      <>
        <Motif />
        <Glance />
      </>,
      daemon(),
    );
    dispose = () => m.router.dispose();
    expect(role("segments-motif")).toBeTruthy();
    expect(screen.getByText("The programme")).toBeTruthy();
    await settle();
    expect(screen.getByText("2 pilot")).toBeTruthy();
    expect(screen.getByText("2 descoped")).toBeTruthy();
    cleanup();
    render(<Mini />);
    expect(role("segments-mini")?.querySelectorAll("rect")).toHaveLength(7);
  });
});
