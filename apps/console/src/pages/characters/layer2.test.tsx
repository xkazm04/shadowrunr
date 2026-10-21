import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import DOM_BUDGETS from "../../../budgets/dom.json";
import { fail, mountPage, nodes, ok, role, roles, settle, type Script } from "../../kit/testing";
import { Layer2 } from "./Layer2";
import { genResults, genRoster, malakorDetail, malakorParams, malakorRules, malakorSummary } from "./testgen";

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
  /** The daemon's active.json tone, changed by character.tone.set. */
  tone?: { value: string };
  /** character.get serves the v2 tables (malakorParams) instead of the v1 detail. */
  v2?: boolean;
}

const daemon =
  (o: Daemon = {}): Script =>
  (method, sent) => {
    switch (method) {
      case "character.list":
        return ok(o.roster ?? [malakorSummary()]);
      case "character.get":
        return /"id":"malakor"/.test(sent) ? ok({ ...(o.v2 ? malakorParams() : malakorDetail()), ...(o.tone ? { active_tone: o.tone.value } : {}) }) : fail("not_found", "no such character");
      case "character.tone.set": {
        const tone = /"tone":"([^"]+)"/.exec(sent)?.[1] ?? "";
        if (o.tone) o.tone.value = tone;
        return ok(malakorSummary({ active_tone: tone }));
      }
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

describe("characters/<id>/package/parameters", () => {
  it("shows the v2 tables read-only and marks the tuning key that differs from the default", async () => {
    mount(["malakor", "package", "parameters"], { v2: true });
    await settle();
    expect(role("characters-identity")?.textContent).toContain("Lord of the Darkness (Lord)");
    expect(role("characters-identity")?.textContent).toContain("minion / minions");
    expect(role("characters-voice")?.textContent).toContain("Cut-out portrait");
    const ranks = role("characters-rank-overrides")?.textContent ?? "";
    expect(ranks).toContain("Warlock or higher");
    expect(ranks).toContain("Lesser Lich");
    const rows = roles("characters-tuning-row");
    expect(rows).toHaveLength(3);
    const differs = rows.filter((r) => r.getAttribute("data-differs") === "true");
    expect(differs).toHaveLength(1);
    expect(differs[0]?.textContent).toContain("How long an unanswered message is carried into the next beats");
    expect(differs[0]?.textContent).toContain("30 s");
    expect(differs[0]?.textContent).toContain("25 s");
    expect(screen.getByText("1 of 3 differ from the default")).toBeTruthy();
    expect(role("characters-flavour")?.textContent).toContain("Improv room");
    expect(role("characters-flavour")?.textContent).toContain("cold open");
  });

  it("never renders a raw enum id", async () => {
    mount(["malakor", "package", "parameters"], { v2: true });
    await settle();
    const text = role("characters-package")?.textContent ?? "";
    for (const id of ["SEG-01", "dread_council", "propose_min_rank", "cold_open", "carry_ms", "assumption", "measured"]) expect(text).not.toContain(id);
    expect(text).not.toMatch(/\bwarlock\b|\bcut\b/);
  });

  it("shows an empty state for every v2 table a v1 package leaves out", async () => {
    mount(["malakor", "package", "parameters"]);
    await settle();
    for (const r of ["characters-identity", "characters-voice", "characters-rank-overrides", "characters-tuning", "characters-flavour"]) expect(role(r)).toBeNull();
    const empties = [...document.querySelectorAll("[data-state='empty']")].map((e) => e.textContent ?? "");
    expect(empties).toHaveLength(5);
    expect(empties[0]).toContain("No identity table");
  });
});

describe("characters/<id>/package", () => {
  it("opens on the constitution: nine locked red lines, then the prose", async () => {
    mount(["malakor"]);
    await settle();
    expect(role("characters-package")?.getAttribute("data-tab")).toBe("constitution");
    expect(role("characters-redlines")?.children).toHaveLength(9);
    expect(screen.getByRole("heading", { name: "Who he is" })).toBeTruthy();
    expect(screen.getAllByRole("tab")).toHaveLength(8);
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

describe("tones", () => {
  it("makes another tone active through character.tone.set and shows it as the hero", async () => {
    const tone = { value: "theatrical" };
    const m = mount(["malakor", "package", "tones"], { tone });
    await settle();
    expect(role("characters-tone-hero")?.textContent).toContain("Theatrical");
    const card = roles("characters-tone-card").find((c) => c.textContent?.includes("Weary middle manager"));
    await act(async () => {
      card?.querySelector<HTMLButtonElement>("[data-role='characters-tone-set']")?.click();
    });
    await settle();
    expect(m.fetch.calls).toContain("character.tone.set");
    expect(role("characters-tone-hero")?.textContent).toContain("Weary middle manager");
    expect(role("characters-tone-hero")?.textContent).toContain("The operator's choice (the package default is Theatrical)");
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
    const rule = () => document.querySelector("[data-rule-id='no-financial-promotion']");
    expect(rule()?.getAttribute("data-enabled")).toBe("true");
    expect(role("characters-instrument")?.textContent).toContain("These match the package defaults.");
    const sw = rule()?.querySelector("[role='switch']");
    await act(async () => {
      sw?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    await settle();
    expect(sent).toHaveLength(1);
    expect(sent[0]).toContain('"params":{"character":"malakor","id":"no-financial-promotion","enabled":false}');
    expect(rule()?.getAttribute("data-enabled")).toBe("false");
    expect(rule()?.textContent).toContain("changed by the operator");
    expect(role("characters-instrument")?.textContent).toContain("1 of 2 rules differ from the package defaults.");
    // The data layer refetched what the switch changed (MUTATION_INVALIDATES), the page did not.
    expect(m.fetch.calls.filter((c) => c === "character.rules").length).toBeGreaterThanOrEqual(2);
    expect(m.fetch.calls.filter((c) => c === "character.get").length).toBeGreaterThanOrEqual(2);
    expect(screen.getByRole("switch", { name: /No financial promotion: enabled/ })).toBeTruthy();
  });

  it("renders a rail locked, with no switch, and leaves it out of the differs count", async () => {
    mount(["malakor", "package", "rules"]);
    await settle();
    const rail = document.querySelector("[data-rule-id='crisis-handoff']");
    expect(rail?.getAttribute("data-locked")).toBe("true");
    expect(rail?.getAttribute("data-enabled")).toBe("true");
    expect(rail?.querySelector("[role='switch']")).toBeNull();
    expect(rail?.textContent).toContain("always on");
    expect(screen.queryByRole("switch", { name: /Crisis handoff/ })).toBeNull();
    const optional = document.querySelector("[data-rule-id='no-financial-promotion']");
    expect(optional?.getAttribute("data-locked")).toBe("false");
    expect(optional?.querySelector("[role='switch']")).not.toBeNull();
    expect(role("characters-instrument")?.textContent).toContain("These match the package defaults.");
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
    expect(links).toEqual(["#/simulator/verdicts/run-0", "#/brain/transcripts/t-scale%2Frun-0/10", "#/simulator/verdicts/run-1", "#/brain/transcripts/t-scale%2Frun-1/10"]);
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
