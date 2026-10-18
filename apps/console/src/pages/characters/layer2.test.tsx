import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import DOM_BUDGETS from "../../../budgets/dom.json";
import { fail, mountPage, nodes, ok, role, roles, settle, type Script } from "./harness";
import { Layer2 } from "./Layer2";
import { genResults, genRoster, malakorDetail, malakorRules, malakorSummary } from "./testgen";

const BUDGET: number = DOM_BUDGETS.routes["wall+l2"];

let dispose: (() => void) | undefined;
afterEach(() => {
  cleanup();
  dispose?.();
});

interface Daemon {
  roster?: ReturnType<typeof genRoster>;
  results?: ReturnType<typeof genResults>;
  tournaments?: string[];
  onSet?: (sent: string) => ReturnType<Script>;
  /** The daemon's rules.json override, changed by character.rules.set. */
  state?: Record<string, boolean>;
}

const daemon =
  (o: Daemon = {}): Script =>
  (method, sent) => {
    switch (method) {
      case "character.list":
        return ok(o.roster ?? [malakorSummary()]);
      case "character.get":
        return /"id":"malakor"/.test(sent) ? ok(malakorDetail()) : fail("not_found", "no such character");
      case "character.rules":
        return ok(malakorRules(o.state));
      case "character.rules.set":
        return o.onSet ? o.onSet(sent) : ok(malakorRules(o.state));
      case "tournament.list":
        return ok((o.tournaments ?? ["t-pilot", "t-scale"]).map((id) => ({ tournament_id: id, runs: 4, completed: 4, instrument_hash: "h" })));
      case "tournament.results":
        return ok(o.results ?? genResults(2, 1));
      default:
        return fail("method_not_found", method);
    }
  };

function mount(sub: string[], o: Daemon = {}) {
  const m = mountPage(<Layer2 sub={sub} />, daemon(o), `#/characters/${sub.join("/")}`);
  dispose = m.dispose;
  return m;
}

describe("characters/all", () => {
  it("virtualises 2,000 characters inside the layer-2 DOM budget and opens a package", async () => {
    const m = mount(["all"], { roster: genRoster(2000) });
    await settle();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("2,000 characters");
    const table = role("characters-all-table");
    const rows = table?.querySelectorAll("tbody tr[data-index]") ?? [];
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(60);
    expect(nodes()).toBeLessThan(BUDGET);
    fireEvent.click(rows[0] ?? document.body);
    await settle(1);
    expect(m.router.getRoute()).toEqual({ view: "page", page: "characters", sub: ["malakor", "package", "constitution"] });
  });

  it("counts the package anatomy from the first character", async () => {
    mount(["all"]);
    await settle();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("One character, so far");
    expect(screen.getByText("31 sim situations")).toBeTruthy();
    expect(document.querySelectorAll("svg.foil-art").length).toBe(5);
  });
});

describe("characters/<id>/package", () => {
  it("opens on the constitution: nine locked red lines, then the prose", async () => {
    mount(["malakor"]);
    await settle();
    expect(role("characters-package")?.getAttribute("data-tab")).toBe("constitution");
    expect(role("characters-redlines")?.children).toHaveLength(9);
    expect(screen.getByRole("heading", { name: "Who he is" })).toBeTruthy();
    expect(screen.getAllByRole("tab")).toHaveLength(6);
  });

  it("switches tabs by route", async () => {
    const m = mount(["malakor", "package", "constitution"]);
    await settle();
    fireEvent.click(screen.getByRole("tab", { name: "Optional rules" }));
    await settle(1);
    expect(m.router.getRoute()).toEqual({ view: "page", page: "characters", sub: ["malakor", "package", "rules"] });
  });

  it("reads the bible with its table of contents, the tones with the default as hero, and the foils", async () => {
    mount(["malakor", "package", "bible"]);
    await settle();
    expect(screen.getByRole("navigation", { name: "Bible sections" }).querySelectorAll("button").length).toBe(9);
    cleanup();
    dispose?.();
    mount(["malakor", "package", "tones"]);
    await settle();
    expect(role("characters-tone-hero")?.textContent).toContain("Theatrical");
    expect(roles("characters-tone-card")).toHaveLength(4);
    cleanup();
    dispose?.();
    mount(["malakor", "package", "foils"]);
    await settle();
    expect(roles("characters-foil-card")).toHaveLength(5);
    expect(screen.getByText("The Coin Prophet")).toBeTruthy();
  });

  it("shows the not-found state for an unknown character and the invalid state for an unknown page", async () => {
    mount(["nobody"]);
    await settle();
    expect(document.querySelector("[data-state='empty']")?.textContent).toContain('No character package "nobody"');
    cleanup();
    dispose?.();
    mount(["malakor", "wardrobe"]);
    expect(document.querySelector("[data-state='invalid']")?.textContent).toContain("There is no page at malakor / wardrobe");
  });
});

describe("optional rules are real switches", () => {
  it("sends character.rules.set and shows the daemon's reply, with the instrument card", async () => {
    const sent: string[] = [];
    const state: Record<string, boolean> = {};
    const m = mount(["malakor", "package", "rules"], {
      state,
      onSet: (body) => {
        sent.push(body);
        const id = /"id":"([^"]+)"/.exec(body)?.[1] ?? "";
        state[id] = /"enabled":true/.test(body);
        return ok(malakorRules(state));
      },
    });
    await settle();
    const rule = () => document.querySelector("[data-rule-id='crisis-handoff']");
    expect(rule()?.getAttribute("data-enabled")).toBe("true");
    expect(role("characters-instrument")?.textContent).toContain("These match the package defaults.");
    const sw = rule()?.querySelector("[role='switch']");
    await act(async () => {
      sw?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    await settle();
    expect(sent).toHaveLength(1);
    expect(sent[0]).toContain('"params":{"character":"malakor","id":"crisis-handoff","enabled":false}');
    expect(rule()?.getAttribute("data-enabled")).toBe("false");
    expect(rule()?.textContent).toContain("changed by the operator");
    expect(role("characters-instrument")?.textContent).toContain("1 of 2 rules differ from the package defaults.");
    // The page refetched what the switch changed (no invalidation entry exists yet).
    expect(m.fetch.calls.filter((c) => c === "character.rules").length).toBeGreaterThanOrEqual(2);
    expect(m.fetch.calls.filter((c) => c === "character.list").length).toBeGreaterThanOrEqual(2);
  });

  it("keeps the switch and says why when the daemon refuses", async () => {
    mount(["malakor", "package", "rules"], { onSet: () => fail("invalid_params", "unknown rule") });
    await settle();
    const rule = () => document.querySelector("[data-rule-id='no-financial-promotion']");
    await act(async () => {
      rule()?.querySelector("[role='switch']")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    await settle();
    expect(rule()?.getAttribute("data-enabled")).toBe("true");
    expect(rule()?.querySelector("[data-state='invalid']")?.textContent).toContain("invalid_params: unknown rule");
  });
});

describe("situations link to their verdicts", () => {
  it("tallies each situation:* rule from the latest tournament and links the failing evidence", async () => {
    mount(["malakor", "package", "situations", "bait-slur-request"]);
    await settle(6);
    const table = role("characters-situations");
    expect(table?.querySelectorAll("tbody tr[data-index]").length).toBeGreaterThan(0);
    expect((document.querySelector("select") as HTMLSelectElement | null)?.value).toBe("t-scale");
    const detail = role("characters-situation");
    expect(detail?.textContent).toContain("Judged in 8 runs of t-scale: 2 failed");
    const links = [...(role("characters-situation-evidence")?.querySelectorAll("a") ?? [])].map((a) => a.getAttribute("href"));
    expect(links).toEqual(["#/simulator/verdicts/run-0", "#/brain/transcripts/run-0/10", "#/simulator/verdicts/run-1", "#/brain/transcripts/run-1/10"]);
  });

  it("selects a situation by route from the table", async () => {
    const m = mount(["malakor", "package", "situations"]);
    await settle(6);
    const first = role("characters-situations")?.querySelector("tbody tr[data-index]");
    fireEvent.click(first ?? document.body);
    await settle(1);
    const sub = m.router.getRoute();
    expect(sub.view === "page" && sub.sub.slice(0, 3)).toEqual(["malakor", "package", "situations"]);
    expect(sub.view === "page" && sub.sub.length).toBe(4);
  });

  it("says so when no tournament has run, and holds 2,000 failing refs in the budget", async () => {
    mount(["malakor", "package", "situations", "bait-slur-request"], { tournaments: [] });
    await settle(6);
    expect(role("characters-situation-results")?.textContent).toContain("No tournament has run yet");
    cleanup();
    dispose?.();
    mount(["malakor", "package", "situations", "bait-slur-request"], { results: genResults(40, 50) });
    await settle(6);
    expect(role("characters-situation-evidence")?.querySelectorAll("li")).toHaveLength(40);
    expect(role("characters-situation")?.textContent).toContain("1,960 more lines across 40 runs");
    expect(nodes()).toBeLessThan(BUDGET);
  });
});
