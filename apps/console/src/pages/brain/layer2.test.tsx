// Layer 2 in jsdom: the engine floor, the run list at 48k runs and the transcript viewer at 100k
// lines, all virtualised under the wall+l2 DOM budget, plus deep links and the offline state.
import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import DOM_BUDGETS from "../../../budgets/dom.json";
import { pageRoute } from "../../router";
import { schemaLabel } from "../../router/schema";
import { BRAIN_ROUTES } from "../../router/brain";
import { page } from "./index";
import { Layer2, lineOf } from "./Layer2";
import { daemon, season, transcript } from "./testgen";
import { mountPage as mount, nodes, ok, settle } from "../../kit/testing";

const L2_BUDGET: number = DOM_BUDGETS.routes["wall+l2"];

afterEach(cleanup);

describe("brain/transcripts/<run>/<line>", () => {
  it("virtualises a 100k-line transcript and opens the deep-linked line", async () => {
    const s = season({ tournaments: 1, runs: 1, rowsPerRun: 1, transcriptLines: 10 });
    const big = new Map([["gen-t0000/r1", transcript(100_000, "r1")]]);
    mount(<Layer2 sub={["transcripts", "gen-t0000/r1", "50000"]} />, daemon(s, big), "#/brain/transcripts/gen-t0000%2Fr1/50000");
    await settle(8);
    expect(screen.getByText(/100,000 of 100,000 lines/)).toBeTruthy();
    const rows = document.querySelectorAll("[data-role='brain-lines'] [role='listitem']").length;
    expect(rows).toBeGreaterThan(0);
    expect(rows).toBeLessThan(60);
    expect(document.querySelector("[data-role='brain-line-detail']")?.textContent).toContain("Line 50000");
    const n = nodes();
    expect(n).toBeLessThan(L2_BUDGET);
    console.info(`brain transcript viewer dom-count at 100k lines: ${n}`);
  });

  it("filters by type and deep-links a picked line by route", async () => {
    const s = season({ tournaments: 1, runs: 1, rowsPerRun: 1, transcriptLines: 10 });
    const { router } = mount(<Layer2 sub={["transcripts", "r2"]} />, daemon(s, new Map([["r2", transcript(600, "r2")]])), "#/brain/transcripts/r2");
    await settle(8);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "say" })));
    await settle(8);
    expect(screen.getByText(/199 of 600 lines/)).toBeTruthy();
    const first = document.querySelector<HTMLButtonElement>("[data-role='brain-lines'] button[data-line]");
    expect(first).not.toBeNull();
    await act(async () => first?.click());
    expect(router.getRoute()).toEqual(pageRoute("brain", "transcripts", "r2", first?.getAttribute("data-line") ?? ""));
  });

  it("parses line segments and labels the breadcrumbs from ids", () => {
    expect(lineOf("12")).toBe(12);
    expect(lineOf("0")).toBeUndefined();
    expect(lineOf("x")).toBeUndefined();
    const parts = ["brain", "transcripts", "t/run.a", "7"];
    const label = schemaLabel(BRAIN_ROUTES);
    expect(label("transcripts", 1, parts)).toBe("Transcripts");
    expect(label("t/run.a", 2, parts)).toBe("t/run.a");
    expect(label("7", 3, parts)).toBe("Line 7");
  });

  it("declares its grammar as the page module's routes (no hand-written label switch)", () => {
    expect(page.routes).toBe(BRAIN_ROUTES);
    expect("label" in page).toBe(false);
  });
});

describe("brain/transcripts", () => {
  it("pages runs in by cursor and stays virtualised at 48k runs", async () => {
    const s = season({ tournaments: 1, runs: 48_000, rowsPerRun: 0, transcriptLines: 10 });
    const { calls } = mount(<Layer2 sub={["transcripts"]} />, daemon(s), "#/brain/transcripts");
    await settle(20);
    expect(calls.filter((c) => c === "runs.page").length).toBe(4);
    expect(screen.getByText(/Runs · 2,000\+/)).toBeTruthy();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: /Load 2,000 more/ })));
    await settle(20);
    expect(screen.getByText(/Runs · 4,000\+/)).toBeTruthy();
    const n = nodes();
    expect(n).toBeLessThan(L2_BUDGET);
    console.info(`brain run list dom-count with 4,000 of 48,000 runs loaded: ${n}`);
  });

  it("shows one offline state when the daemon is down", async () => {
    mount(<Layer2 sub={["transcripts"]} />, daemon(null), "#/brain/transcripts");
    await settle(8);
    expect(screen.getAllByText("The daemon is not reachable")).toHaveLength(1);
    expect(screen.getByText(/Layer 2 ·/)).toBeTruthy();
  });
});

describe("brain/engine", () => {
  it("derives routing, accounts and fallbacks from the newest tournament's ledger", async () => {
    const s = season({ tournaments: 3, runs: 6, rowsPerRun: 60, transcriptLines: 10 });
    mount(<Layer2 sub={["engine"]} />, daemon(s), "#/brain/engine");
    await settle(20);
    const routes = document.querySelector("[data-role='brain-routes']");
    expect(routes?.querySelectorAll("tbody tr").length).toBe(4);
    expect(document.querySelector("[data-role='brain-accounts']")?.textContent).toContain("spare");
    expect(screen.getByText("Fallbacks", { selector: "h2 *, h2" })).toBeTruthy();
    expect((screen.getByRole("combobox") as HTMLSelectElement).value).toBe("gen-t0002");
    expect(nodes()).toBeLessThan(L2_BUDGET);
  });

  it("shows the engine's live routes and accounts from engine.status, with the ledger's calls", async () => {
    const s = season({ tournaments: 1, runs: 6, rowsPerRun: 60, transcriptLines: 10 });
    const base = daemon(s);
    const status = {
      routes: [
        { role: "core_thread", tier: "sonnet", effort: "low", timeout_ms: 30_000, overridden: true },
        { role: "chatter", tier: "haiku", effort: "low", timeout_ms: 20_000, overridden: false },
        { role: "judge", tier: "opus", effort: "high", timeout_ms: 600_000, overridden: false },
      ],
      accounts: [{ label: "main" }, { label: "spare", cooling_until_ms: 1_800_000_600_000 }],
      fleet: { mode: "primary", since_ms: 1_800_000_000_000 },
      fallbacks: ["codex"],
      api_fallback: true,
    };
    mount(<Layer2 sub={["engine"]} />, (method, sent) => (method === "engine.status" ? ok(status) : base(method, sent)), "#/brain/engine");
    await settle(20);
    const routes = document.querySelector("[data-role='brain-routes']");
    expect(routes?.getAttribute("data-source")).toBe("engine.status");
    expect(routes?.querySelectorAll("tbody tr")).toHaveLength(3);
    expect(routes?.textContent).toContain("overridden");
    const accounts = document.querySelector("[data-role='brain-accounts']");
    expect(accounts?.querySelector("[data-account='main']")?.textContent).toContain("in rotation");
    expect(accounts?.querySelector("[data-account='spare']")?.textContent).toContain("cooling until");
    expect(document.querySelector("[data-role='brain-fleet']")?.textContent).toContain("API key, codex");
  });

  it("names an unknown floor instead of rendering blank", async () => {
    mount(<Layer2 sub={["nope"]} />, daemon(null), "#/brain/nope");
    await settle(8);
    expect(screen.getByText(/No brain page called/)).toBeTruthy();
  });
});
