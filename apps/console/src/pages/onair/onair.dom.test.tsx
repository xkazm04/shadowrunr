// On Air in jsdom, on the data-layer test doubles: the screen's fidelity roles, its designed
// states, and DOM counts at the 10× shape (48k runs paged 500 at a time, a 100k-line
// transcript). Counts are gated; timings are not.
import type { RunRow, StreamEvent } from "@shadowrunr/contract";
import { foldEvents } from "@shadowrunr/stage-kit";
import { act, cleanup, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it } from "vitest";
import DOM_BUDGETS from "../../../budgets/dom.json";
import type { FetchReply } from "../../data/testing";
import { mountPage, nodes } from "../../kit/testing";
import { pageRoute } from "../../router";
import { Layer1 } from "./Layer1";
import { Layer2 } from "./Layer2";
import type { FeedSnapshot } from "./player";
import { Screen } from "./Screen";
import { TABLES, runRow, transcript } from "./testdata";

const BUDGETS: { readonly routes: Readonly<Record<string, number | undefined>> } = DOM_BUDGETS;
/** The shell alone may use the wall budget; a page gets the rest of its layer's line. */
const PAGE_L1 = (BUDGETS.routes["wall+l1"] ?? 0) - (BUDGETS.routes["wall"] ?? 0);
const PAGE_L2 = (BUDGETS.routes["wall+l2"] ?? 0) - (BUDGETS.routes["wall"] ?? 0);

const FIDELITY_ROLES = [
  "onair-layer1",
  "onair-title",
  "onair-screen",
  "onair-set",
  "onair-hud",
  "onair-rundown",
  "onair-malakor",
  "onair-foil",
  "onair-room",
  "onair-hex",
  "onair-hexmap",
  "onair-grade-legend",
  "onair-slip",
  "onair-vote",
  "onair-chat",
  "onair-lower-third",
  "onair-slate",
  "onair-progress",
  "onair-transport",
];

let dispose: (() => void) | null = null;

function mount(ui: ReactNode, reply: (method: string, body: string) => FetchReply) {
  const m = mountPage(ui, reply, "#/onair");
  dispose = m.dispose;
  return m;
}

function daemon(runs: RunRow[], events: StreamEvent[], next?: string) {
  return (method: string): FetchReply => {
    switch (method) {
      case "runs.page":
        return { body: { result: next === undefined ? { items: runs } : { items: runs, next_cursor: next } } };
      case "run.transcript":
        return { body: { result: events } };
      case "segments.tables":
        return { body: { result: TABLES } };
      case "directive.list":
      case "reconcile.list":
        return { body: { result: [] } };
      default:
        return { status: 404, body: { error: { code: "method_not_found", message: method } } };
    }
  };
}

afterEach(() => {
  cleanup();
  dispose?.();
  dispose = null;
});

describe("On Air layer 1", () => {
  it("replays the newest run with every fidelity role on screen", async () => {
    mount(<Layer1 active />, daemon(Array.from({ length: 40 }, (_, i) => runRow(i)), transcript(400)));
    await screen.findByText(/Replay 4× · theatrical · haiku · 300 simulated viewers/);
    for (const role of FIDELITY_ROLES) expect(document.querySelector(`[data-role='${role}']`), role).not.toBeNull();
    expect(screen.getByText("2 more descoped")).toBeTruthy();
    // A/1's selector: the live feed plus the first page of real runs.
    expect(screen.getByLabelText(/Choose a real run/).querySelectorAll("option")).toHaveLength(41);
    expect(nodes()).toBeLessThanOrEqual(PAGE_L1);
  });

  it("holds its DOM count on a 100k-line transcript", async () => {
    mount(<Layer1 active />, daemon([runRow(0)], transcript(100_000)));
    await screen.findByText(/Replay 4×/, undefined, { timeout: 20_000 });
    expect(nodes()).toBeLessThanOrEqual(PAGE_L1);
  }, 60_000);

  it("draws the busiest scene of a 100k-line transcript within the budget", () => {
    const scene = foldEvents(transcript(100_000));
    const snap: FeedSnapshot = { scene, playing: false, position: scene.lastTs ?? 0, start: 0, end: scene.lastTs ?? 0, speed: 4, epoch: 0, live: false };
    mount(<Screen snapshot={snap} pills={[]} descoped={0} chatRoute={pageRoute("brain", "transcripts")} />, daemon([], []));
    expect(document.querySelectorAll("[data-role='onair-chat'] [data-rank]")).toHaveLength(6);
    expect(nodes()).toBeLessThanOrEqual(PAGE_L1);
  }, 60_000);

  it("goes off air with the daemon's own words when it is unreachable", async () => {
    mount(<Layer1 active />, () => ({ network: true }));
    await screen.findByText(/^Off air ·/);
    expect(document.querySelector("[data-role='onair-slate'] b")?.textContent).toBe("Off air");
  });

  it("says so when no run has been recorded yet", async () => {
    mount(<Layer1 active />, daemon([], []));
    await screen.findByText("No runs yet", { selector: "span" });
  });

  it("renders nothing while hidden behind a nested page", () => {
    mount(<Layer1 active={false} />, daemon([], []));
    expect(document.querySelector("[data-role='onair-layer1']")).toBeNull();
  });
});

describe("On Air layer 2", () => {
  it("virtualises a 500-run page of a 48k-run season, with a pager", async () => {
    const runs = Array.from({ length: 500 }, (_, i) => runRow(i));
    mount(<Layer2 sub={["rundown"]} />, daemon(runs, [], "cursor-500"));
    await screen.findByText("500 of 500");
    const rows = document.querySelectorAll("[data-role='onair-runs'] tbody tr[data-index]");
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(80);
    expect(screen.getByRole("button", { name: "Next 500" }).hasAttribute("disabled")).toBe(false);
    await screen.findByText("Cartography Legion");
    expect(nodes()).toBeLessThanOrEqual(PAGE_L2);
  });

  it("shows one run's spans and virtualised beats on a 100k-line transcript", async () => {
    mount(<Layer2 sub={["rundown", "t-0", "run-0"]} />, daemon([runRow(0)], transcript(100_000)));
    await screen.findByText(/Beats ·/, undefined, { timeout: 20_000 });
    expect(document.querySelector("[data-role='onair-run-spans']")).not.toBeNull();
    const beats = document.querySelectorAll("[role='list'][aria-label='Beats'] [role='listitem']");
    expect(beats.length).toBeGreaterThan(0);
    expect(beats.length).toBeLessThan(80);
    expect(nodes()).toBeLessThanOrEqual(PAGE_L2);
  }, 60_000);

  it("shows the dispatch's designed empty state and the standing directives", async () => {
    mount(<Layer2 sub={["dispatch"]} />, daemon([], []));
    await screen.findByText("No reconcile report yet");
    await screen.findByText("None active");
  });

  it("shows what Malakor receives: the plan in force, on the slip and the dispatch page", async () => {
    const plan = {
      plan_id: "plan-1",
      created_ms: 1,
      lord_dispatch: "Audit the ledgers before the moon sets.",
      segments: [{ segment: "SEG-01", minutes: 20, notes: "open warm" }],
      topics: ["the missing 4%"],
      directives: [],
      report_id: "rec-1",
    };
    const base = daemon([runRow(0)], transcript(40));
    const withPlan = (method: string): FetchReply =>
      method === "plan.current"
        ? { body: { result: plan } }
        : method === "character.list"
          ? { body: { result: [{ id: "malakor", name: "Arch-Lich Malakor", default_tone: "theatrical", active_tone: "weary-middle-manager", tones: ["theatrical", "weary-middle-manager"], foils: [], rules_enabled: 0, rules_total: 0 }] } }
          : base(method);
    mount(<Layer1 active />, withPlan);
    await screen.findByText("Audit the ledgers before the moon sets.");
    expect(document.querySelector("[data-role='onair-screen']")?.getAttribute("data-slip")).toBe("true");
    await screen.findByText(/next stream's tone: weary middle manager/);
    cleanup();
    dispose?.();
    mount(<Layer2 sub={["dispatch"]} />, withPlan);
    await screen.findByText("What Malakor receives");
    expect(document.querySelector("[data-role='onair-dispatch-text']")?.textContent).toBe("Audit the ledgers before the moon sets.");
    expect(document.querySelector("[data-role='onair-dispatch-segments']")?.textContent).toContain("SEG-01 · 20 min · open warm");
  });

  it("names an unknown nested page instead of going blank", async () => {
    await act(async () => {
      mount(<Layer2 sub={["nope"]} />, daemon([], []));
    });
    expect(screen.getByText(/On Air has no page/)).toBeTruthy();
  });
});
