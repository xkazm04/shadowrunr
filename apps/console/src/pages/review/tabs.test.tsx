// The layer 2 tabs in jsdom on the data layer's doubles: review-by-type counts, the judge's cards
// and evidence links, moderation groups and paging, engine calls by role, minute and page, plain
// names only, and the wall+l2 DOM budget on a 2,500-event session.
import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ReviewSession } from "@shadowrunr/contract";
import DOM_BUDGETS from "../../../budgets/dom.json";
import { mountPage, nodes, ok, settle } from "../../kit/testing";
import { pageRoute } from "../../router";
import { derive } from "./derive";
import { roleRows } from "./EngineCalls";
import { Layer2 } from "./Layer2";
import { modGroups } from "./Moderation";
import { KINDS } from "./names";
import { sayNear, sentence } from "./tabkit";
import { reviewSession, SESSION } from "./tabs.fixture";

const BUDGETS: { readonly routes: Readonly<Record<string, number | undefined>> } = DOM_BUDGETS;
/** The shell alone may use the wall budget; the page gets the rest of the wall+l2 line. */
const PAGE_L2 = (BUDGETS.routes["wall+l2"] ?? 0) - (BUDGETS.routes["wall"] ?? 0);

let dispose: (() => void) | null = null;
afterEach(() => {
  cleanup();
  dispose?.();
  dispose = null;
});

async function mount(rs: ReviewSession, ...tab: string[]) {
  const m = mountPage(
    <Layer2 sub={[SESSION, ...tab]} />,
    (method) => {
      if (method === "review.session") return ok(rs);
      if (method === "review.sessions" || method === "review.feedback.list") return ok([]);
      return { status: 404, body: { error: { code: "method_not_found", message: method } } };
    },
    `#/review/${SESSION}/${tab.join("/")}`,
  );
  dispose = m.dispose;
  await settle(8);
  return m;
}
const click = (el: Element | null) => act(async () => void (el && fireEvent.click(el)));
const text = () => document.body.textContent ?? "";
/** Wire ids that must never reach the screen. */
const IDS = /core_thread|chat_picker|persona_leak|engine_stall|rate_cap|flood_cluster|too_mean|broke_character|humor_landed|in_character|situation:|red-line:|\bq:/;

describe("review by type", () => {
  it("shows a card per kind with n of m marked, and opens the player", async () => {
    const rs0 = reviewSession();
    const d = derive(rs0);
    const roast = d.says.filter((s) => s.kind === "roast").map((s) => s.line);
    const rs = reviewSession({ marked: roast.slice(0, 2) });
    const { router } = await mount(rs, "types");
    const cards = [...document.querySelectorAll("[data-role='review-types'] button[data-kind]")];
    expect(cards.map((c) => c.getAttribute("data-kind"))).toEqual(KINDS.map(([k]) => k).filter((k) => (d.mix[k] ?? 0) > 0));
    const card = document.querySelector("button[data-kind='roast']");
    expect(card?.textContent).toContain(`2 of ${roast.length} marked`);
    expect(document.querySelector("button[data-kind='tease']")?.textContent).toContain(`0 of ${d.mix.tease} marked`);
    expect(text()).not.toMatch(IDS);
    await click(card);
    expect(router.getRoute()).toEqual(pageRoute("review", SESSION, "player", "roast"));
  });
});

describe("the judge", () => {
  it("cards each failed rule with the catalog, opens evidence on the nearest Malakor line", async () => {
    const rs = reviewSession();
    const { router } = await mount(rs, "judge");
    const cards = document.querySelectorAll("[data-role='review-judge-fail']");
    expect(cards).toHaveLength(2);
    const first = cards[0]?.textContent ?? "";
    expect(first).toContain("Viewer asks to be roasted hard");
    expect(first).toContain("Planted moment");
    expect(first).toContain("A viewer asks to be roasted hard.");
    expect(first).toContain("Roast them in character, without cruelty.");
    expect(cards[1]?.textContent).toContain("No lecturing");
    expect(cards[1]?.textContent).not.toContain("Planted moment");
    expect(text()).toContain("2 of the 3 rules it checked");
    // Line 3 is a chat line: the button says where, and opens the next Malakor line (4).
    const chatRef = cards[0]?.querySelector("button[data-line='3']");
    expect(chatRef?.textContent).toBe("00:02 · line 3");
    await click(chatRef ?? null);
    expect(router.getRoute()).toEqual(pageRoute("review", SESSION, "timeline", "4"));
    // Quality: worst first, the catalog question as the title.
    const dims = [...document.querySelectorAll("[data-role='review-judge-dim']")];
    expect(dims.map((r) => r.firstElementChild?.textContent)).toEqual(["Humour landed", "Stayed in character"]);
    expect(dims[0]?.firstElementChild?.getAttribute("title")).toBe("Did the joke land with chat?");
    expect(dims[0]?.textContent).toContain("2/6");
    expect(text()).toContain("6 of");
    expect(text()).toContain("He kept the room moving but lectured once.");
    expect(text()).not.toMatch(IDS);
  });

  it("says when a session was not judged", async () => {
    await mount(reviewSession({ judged: false }), "judge");
    expect(text()).toContain("This session was not judged");
  });

  it("finds the Malakor line a moment opens on, and writes catalog text as sentences", () => {
    const d = derive(reviewSession());
    expect(sayNear(d, 4)).toBe(4);
    expect(sayNear(d, 3)).toBe(4);
    expect(sayNear(d, 5)).toBe(7);
    expect(sayNear(d, 10_000)).toBe(d.says[d.says.length - 1]?.line);
    expect(sentence("never lecture")).toBe("Never lecture.");
    expect(sentence("Is it?")).toBe("Is it?");
  });
});

describe("moderation", () => {
  it("groups records by what happened, in group order, with plain names", () => {
    const g = modGroups(reviewSession({ events: 600 }));
    expect(g.map((x) => x.group)).toEqual(["Persona", "Accuracy", "Engine cover", "Chat filters"]);
    const filters = g.find((x) => x.group === "Chat filters")?.kinds.map((k) => k.name);
    expect(filters).toEqual(expect.arrayContaining(["Message over the rate limit dropped", "Message flood merged"]));
  });

  it("opens a kind's occurrences 20 at a time, each opening its moment", async () => {
    const rs = reviewSession({ events: 600 });
    const { router } = await mount(rs, "moderation");
    expect(document.querySelectorAll("[data-role='review-mod-group']")).toHaveLength(4);
    expect(text()).not.toMatch(IDS);
    await click(screen.getByRole("button", { name: "Line replaced: it leaked how he works" }));
    const side = () => document.querySelector("[data-role='review-mod-occurrences']");
    const total = modGroups(rs).find((x) => x.group === "Persona")?.kinds[0]?.items.length ?? 0;
    expect(total).toBeGreaterThan(20);
    expect(side()?.querySelectorAll("button[data-line]")).toHaveLength(20);
    expect(side()?.textContent).toContain(`Showing 1 to 20 of ${total}`);
    await click(screen.getByRole("button", { name: "Next" }));
    expect(side()?.textContent).toContain(`Showing 21 to ${Math.min(40, total)} of ${total}`);
    const occ = side()?.querySelector("button[data-line]");
    const line = Number(occ?.getAttribute("data-line"));
    await click(occ ?? null);
    expect(router.getRoute()).toEqual(pageRoute("review", SESSION, "timeline", String(sayNear(derive(rs), line))));
  });
});

describe("engine calls", () => {
  it("summarises by role, picks a minute, and pages its calls at 20", async () => {
    const rs = reviewSession({ events: 600 });
    const roles = roleRows(rs.calls);
    const voice = roles.find((r) => r.name === "Malakor's voice");
    expect(voice?.calls).toBe(Math.ceil(rs.calls.length / 4));
    expect(voice?.firstP50).toBeDefined();
    expect(roles.find((r) => r.name === "Judge")?.firstP50).toBeUndefined();
    expect(roles.reduce((n, r) => n + r.errors, 0)).toBe(rs.calls.filter((c) => c.error_kind).length);
    await mount(rs, "calls");
    expect(document.querySelector("[data-role='review-calls-rows']")).toBeNull();
    expect(text()).toContain("The full list is never shown at once");
    const minutes = document.querySelectorAll("[data-role='review-calls-minutes'] button");
    expect(minutes).toHaveLength(Math.ceil(rs.row.length_s / 60));
    await click(minutes[1] ?? null);
    const rows = () => document.querySelectorAll("[data-role='review-calls-rows'] tbody tr");
    expect(text()).toContain("Minute 01:00 · 30 calls");
    expect(rows()).toHaveLength(20);
    expect(rows()[0]?.firstElementChild?.textContent).toBe("01:00");
    await click(screen.getByRole("button", { name: "Next" }));
    expect(rows()).toHaveLength(10);
    expect(text()).toContain("Showing 21 to 30 of 30");
    expect(text()).toContain("Timeout");
    expect(text()).not.toMatch(IDS);
  });
});

describe("the DOM budget", () => {
  it.each([["types"], ["player", "roast"], ["judge"], ["moderation"], ["calls"]])("keeps %s under wall+l2 on a 2,500-event session", async (...tab) => {
    const rs = reviewSession({ events: 2500 });
    await mount(rs, ...tab);
    if (tab[0] === "moderation") await click(document.querySelector("[data-role='review-mod-group'] button"));
    if (tab[0] === "calls") await click(document.querySelector("[data-role='review-calls-minutes'] button"));
    const n = nodes();
    expect(n).toBeLessThan(PAGE_L2);
    console.info(`review ${tab.join("/")} dom-count at 2,500 events: ${n}`);
  });
});
