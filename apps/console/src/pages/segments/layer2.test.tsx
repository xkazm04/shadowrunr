import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { mountPage, nodes, ok, settle, type Script } from "../../kit/testing";
import { Layer2 } from "./Layer2";
import { genRanks, genReport, genTables } from "./testgen";

let dispose: (() => void) | undefined;
afterEach(() => {
  cleanup();
  dispose?.();
  dispose = undefined;
});

const role = (r: string) => document.querySelector(`[data-role='${r}']`);
const all = (r: string) => [...document.querySelectorAll(`[data-role='${r}']`)];

const base =
  (over: Partial<Record<string, unknown>> = {}): Script =>
  (method) => {
    if (method in over) return ok(over[method]);
    switch (method) {
      case "segments.tables":
        return ok(genTables());
      case "ranks.table":
        return ok(genRanks());
      case "reconcile.report":
        return ok(genReport());
      case "reconcile.list":
        return ok([]);
      default:
        return ok({});
    }
  };

function mount(sub: string[], script: Script = base()) {
  const m = mountPage(<Layer2 sub={sub} />, script, `#/segments/${sub.join("/")}`);
  dispose = () => m.router.dispose();
  return m;
}

describe("segments/tables", () => {
  it("shows the lifecycle with descoped on its own branch, the portfolio and every table", async () => {
    mount(["tables"]);
    await settle();
    const flow = role("segments-lcflow");
    expect([...(flow?.querySelectorAll("[data-status]") ?? [])].map((b) => b.textContent)).toEqual(["2pilot", "0trial", "0regular", "0flagship", "0retired"]);
    expect(role("segments-lcflow-descoped")?.textContent).toBe("2descoped");
    expect(screen.getByText(/2 were descoped: the runtime refuses to start them, and descoped is not retired\./)).toBeTruthy();
    expect(role("segments-portfolio")?.querySelectorAll("tbody tr[data-index]")).toHaveLength(4);
    expect(all("segments-card").map((c) => c.getAttribute("data-segment"))).toEqual(["SEG-01", "SEG-02A", "SEG-02B-I", "SEG-02B-II"]);
  });

  it("prints a segment's states, transitions, ops and handover clock", async () => {
    mount(["tables"]);
    await settle();
    const card = all("segments-card")[0];
    expect([...(card?.querySelectorAll("[data-role='segments-states'] [role='radio']") ?? [])].map((s) => s.textContent)).toEqual(["cold open", "opening", "foil vote", "foil live", "crowd game", "handover pending"]);
    expect(card?.querySelector("[title='Initial state']")?.textContent).toBe("cold open");
    expect(card?.querySelectorAll("[data-role='segments-transitions'] tbody tr")).toHaveLength(15);
    expect(card?.querySelector("[data-role='segments-transitions'] tbody tr:last-child td")?.textContent).toBe("any state");
    const ho = card?.querySelector("[data-role='segments-handover']");
    expect(ho?.textContent).toContain("when at least 15 viewers hold for 3:00 after the cold open, or at 25:00, whichever comes first. Either way the brain then has 1:00 to switch");
    expect(card?.querySelector("[data-role='segments-cold-open']")?.textContent).toContain("once 5 chatters (each wrote in the last 1:00) hold for 0:20, never before 1:00; at 3:00 it ends regardless.");
    expect(card?.querySelector("[data-role='segments-state-rules'] tbody tr")?.textContent).toBe("cold openhighlightshow_cardmark_momentsign_offhostproposeThe room is still filling.");
    expect([...(card?.querySelectorAll("[data-role='segments-state-rules'] th") ?? [])].map((th) => th.textContent)).toContain("Commands");
    const arc = all("segments-card")[1]?.querySelector("[data-role='segments-arc']");
    expect(arc?.textContent).toContain("planned for at least 60:00: Anomaly and filing map, then Shell and archive trail, then Dossier sealed. Sign-off waits for the last 10:00.");
    expect(all("segments-card")[1]?.textContent).toContain("2, then a 1:00 Plebiscite");
    expect(ho?.querySelector("a")?.getAttribute("href")).toBe("#/segments/tables/SEG-02A");
    expect(all("segments-card")[2]?.textContent).toContain("Cartography Legion: descoped from v0.1.");
  });

  it("deep-links one segment, and refuses an unknown one", async () => {
    mount(["tables", "SEG-02B-I"]);
    await settle();
    expect(all("segments-card").map((c) => c.getAttribute("data-segment"))).toEqual(["SEG-02B-I"]);
    expect(role("segments-portfolio")?.querySelector("[aria-selected='true']")?.textContent).toContain("Cartography Legion");
    cleanup();
    dispose?.();
    mount(["tables", "SEG-99"]);
    await settle();
    expect(screen.getByText("No segment SEG-99 in the tables")).toBeTruthy();
  });

  it("shows the offline state when the daemon is down", async () => {
    mount(["tables"], () => ({ network: true }));
    await settle();
    expect(screen.getAllByText(/offline/i).length).toBeGreaterThan(0);
    expect(all("segments-card")).toHaveLength(0);
  });

  it("stays under the layer-2 DOM budget with the portfolio virtualised", async () => {
    mount(["tables"]);
    await settle();
    expect(nodes()).toBeLessThan(1_600);
  });
});

describe("segments/tables/<SEG>/<state>: walking the state machine", () => {
  const checked = () => [...document.querySelectorAll("[data-role='segments-states'] [role='radio'][aria-checked='true']")].map((b) => b.textContent);
  const exits = () => all("segments-state-exit").map((li) => `${li.getAttribute("data-on")}${li.getAttribute("data-guard") ? ` [${li.getAttribute("data-guard")}]` : ""} -> ${li.querySelector("a")?.textContent}`);
  const entries = () => all("segments-state-entry").map((li) => `${li.querySelector("a")?.textContent} via ${li.getAttribute("data-on")}`);

  it("addresses a state by route and selects it", async () => {
    mount(["tables", "SEG-01", "foil_vote"]);
    await settle();
    expect(screen.queryByText(/There is no page/)).toBeNull();
    expect(all("segments-card").map((c) => c.getAttribute("data-segment"))).toEqual(["SEG-01"]);
    expect(checked()).toEqual(["foil vote"]);
    const radio = document.querySelector("[data-role='segments-states'] [aria-checked='true']");
    await act(async () => fireEvent.keyDown(radio as Element, { key: "ArrowRight" }));
    expect(checked()).toEqual(["foil live"]);
  });

  it("lists the cold open's narrowed ops, not the segment's", async () => {
    mount(["tables", "SEG-01", "cold_open"]);
    await settle();
    expect([...(role("segments-state-ops")?.querySelectorAll("code") ?? [])].map((c) => c.textContent)).toEqual(["highlight", "show_card", "mark_moment", "sign_off"]);
  });

  it("expands the '*' row into opening's exits, each a link that walks on", async () => {
    mount(["tables", "SEG-01", "opening"]);
    await settle();
    expect(exits()).toEqual(["foil_vote_opened -> foil vote", "foil_summoned -> foil live", "poll_opened -> crowd game", "handover_due -> handover pending"]);
    const link = all("segments-state-exit")[0]?.querySelector("a");
    expect(link?.getAttribute("href")).toBe("#/segments/tables/SEG-01/foil_vote");
    await act(async () => fireEvent.click(link as Element));
    expect(checked()).toEqual(["foil vote"]);
  });

  it("shows foil vote's guarded exits and its entries", async () => {
    mount(["tables", "SEG-01", "foil_vote"]);
    await settle();
    expect(exits()).toEqual(["foil_vote_closed [foil_active] -> foil live", "foil_vote_closed [no_foil] -> opening", "handover_due -> handover pending"]);
    expect(entries()).toEqual(["opening via foil_vote_opened", "foil live via foil_vote_opened"]);
  });

  it("draws the cold-open clock as an exit", async () => {
    mount(["tables", "SEG-01", "cold_open"]);
    await settle();
    const first = all("segments-state-exit")[0];
    expect(first?.getAttribute("data-clock")).toBe("true");
    expect(first?.textContent?.replace(/\s+/g, " ")).toContain("cold open clock → opening");
  });

  it("flags a state the table strands, and none on SEG-01 as shipped", async () => {
    mount(["tables", "SEG-01"]);
    await settle();
    expect(document.querySelectorAll("[data-unreachable]")).toHaveLength(0);
    cleanup();
    dispose?.();
    const t = genTables();
    const seg = t.segments[0]!;
    const stranded = { ...t, segments: [{ ...seg, states: [...seg.states, "encore"] }, ...t.segments.slice(1)] };
    mount(["tables", "SEG-01"], base({ "segments.tables": stranded }));
    await settle();
    expect([...document.querySelectorAll("[data-unreachable]")].map((b) => b.textContent)).toEqual(["encore"]);
    expect(role("segments-card")?.textContent).toContain("encore: unreachable from cold open");
  });

  it("refuses an unknown state and keeps the card on its initial state", async () => {
    mount(["tables", "SEG-01", "no_such_state"]);
    await settle();
    expect(screen.getByText("No state no_such_state in SEG-01")).toBeTruthy();
    expect(all("segments-card")).toHaveLength(1);
    expect(checked()).toEqual(["cold open"]);
  });
});

describe("segments/commands", () => {
  it("lists the commands with their segments and gates, and the op grid", async () => {
    mount(["commands"]);
    await settle();
    const rows = all("segments-command-row");
    expect(rows.map((r) => r.querySelector("code")?.textContent)).toEqual(["!summon", "!hint", "!grade", "!propose", "!vote"]);
    expect(rows[3]?.textContent).toContain("Warlock and up");
    expect(rows[0]?.textContent).toContain("any rank");
    expect([...(rows[4]?.querySelectorAll("a") ?? [])].map((a) => a.getAttribute("href"))).toEqual(["#/segments/tables/SEG-01", "#/segments/tables/SEG-02A"]);
    const grid = role("segments-op-grid");
    expect([...(grid?.querySelectorAll("thead th") ?? [])].map((t) => t.textContent)).toEqual(["Op", "SEG-01", "SEG-02A", "Segments"]);
    expect(all("segments-op-row")).toHaveLength(13);
    const reveal = all("segments-op-row").find((r) => r.textContent?.startsWith("reveal_hex"));
    expect([...(reveal?.querySelectorAll("[aria-label]") ?? [])].map((x) => x.getAttribute("aria-label"))).toEqual(["not allowed", "allowed"]);
  });
});

describe("segments/ranks", () => {
  it("prints the ladder apex first with the !propose gate", async () => {
    mount(["ranks"]);
    await settle();
    const rows = all("segments-ladder-row");
    expect(rows.map((r) => r.getAttribute("data-rank"))).toEqual(["dread_council", "lich", "warlock", "cultist", "acolyte", "thrall"]);
    expect(rows[2]?.textContent).toContain("5+ gifted");
    expect(rows[2]?.textContent).toContain("yes");
    expect(rows[3]?.textContent).toContain("vote only");
    expect(rows[5]?.textContent).toContain("base rank");
    expect(role("segments-assumptions")).toBeTruthy();
  });

  it("recounts the vote as the operator types", async () => {
    mount(["ranks"]);
    await settle();
    expect(role("segments-calc-verdict")?.textContent).toBe("Head-count: A wins. Weight: B wins.");
    await act(async () => fireEvent.change(screen.getByLabelText("Warlock voters for B"), { target: { value: "0" } }));
    expect(role("segments-calc-verdict")?.textContent).toBe("Head-count: A wins. Weight: A wins.");
  });

  it("shows engagement from the newest stored report without an id in the route", async () => {
    const list = [{ report_id: "rec-1792284840627", created_ms: 1792284840627, source: { tournament_id: "t" }, proposals: 0 }];
    const m = mount(["ranks"], base({ "reconcile.list": list }));
    await settle();
    expect(all("segments-engagement-row").map((r) => r.getAttribute("data-rank"))).toEqual(["thrall", "warlock", "dread_council"]);
    expect(screen.getByText("rec-1792284840627 · newest")).toBeTruthy();
    expect(m.fetch.calls).toContain("reconcile.list");
  });

  it("asks for a report when none is stored, then shows engagement by rank with rates, n and bounds", async () => {
    const m = mount(["ranks"]);
    await settle();
    expect(role("segments-engagement-empty")).toBeTruthy();
    expect(m.fetch.calls).not.toContain("reconcile.report");
    await act(async () => fireEvent.change(screen.getByLabelText("Report id"), { target: { value: " rec-1792284840627 " } }));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Show engagement" })));
    expect(window.location.hash).toBe("#/segments/ranks/rec-1792284840627");
    cleanup();
    dispose?.();
    mount(["ranks", "rec-1792284840627"]);
    await settle();
    const rows = all("segments-engagement-row");
    expect(rows.map((r) => r.getAttribute("data-rank"))).toEqual(["thrall", "warlock", "dread_council"]);
    expect(rows[0]?.querySelector("[role='img']")?.getAttribute("aria-label")).toBe("Picked: 30% (24%–37%), n=212");
    expect(rows[2]?.textContent).toContain("Dread Council");
  });

  it("shows the designed empty state for a report without ranked chat", async () => {
    mount(["ranks", "rec-1"], base({ "reconcile.report": genReport([]) }));
    await settle();
    expect(screen.getByText("No ranked chat in this report")).toBeTruthy();
  });
});

it("refuses a page that does not exist", async () => {
  mount(["commands", "extra"]);
  await settle();
  expect(screen.getByText("There is no page at commands / extra")).toBeTruthy();
});
