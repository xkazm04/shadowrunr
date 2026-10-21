// The timeline tab in jsdom, mounted through layer 2 on the route so deep links are real:
// rows and folding, the kind filter, voted rings, the drawer (row list, moment, Esc, Close, focus),
// the optimistic feedback save, and the DOM budget on a 2,500-event session.
import { act, cleanup, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ReviewSession } from "@shadowrunr/contract";
import DOM_BUDGETS from "../../../budgets/dom.json";
import { mountPage, nodes, ok, param, role, roles, settle, type Script } from "../../kit/testing";
import { encodeRoute, pageRoute, useRoute } from "../../router";
import { bigSession, feedbackRow, SID, smallSession } from "./fixture";
import { Layer2 } from "./Layer2";

const L2_BUDGET: number = DOM_BUDGETS.routes["wall+l2"];
const WALL: number = DOM_BUDGETS.routes.wall;

afterEach(cleanup);

/** Layer 2 at the current route, as the shell mounts it. */
function Routed() {
  const r = useRoute();
  return r.view === "page" && r.sub.length > 0 ? <Layer2 sub={r.sub} /> : null;
}

const daemon =
  (rs: ReviewSession, set: (sent: string) => ReturnType<Script> = (sent) => ok(feedbackRow(Number(/"line":(\d+)/.exec(sent)?.[1]), ["landed"], param(sent, "session_id")))): Script =>
  (method, sent) =>
    method === "review.session" ? ok(rs) : method === "review.feedback.set" ? set(sent) : ok([]);

async function mount(rs: ReviewSession, line?: number, script = daemon(rs)) {
  const m = mountPage(<Routed />, script, encodeRoute(pageRoute("review", rs.row.session_id, "timeline", ...(line === undefined ? [] : [String(line)]))));
  await settle(8);
  return m;
}

const dots = () => [...document.querySelectorAll<HTMLElement>("[data-role='review-tl-row'] [data-line]")];
const dot = (line: number) => document.querySelector<HTMLElement>(`[data-role='review-tl-row'] [data-line='${line}']`);
const rowButton = (i: number) => document.querySelector<HTMLButtonElement>(`[data-role='review-tl-row'][data-row='${i}']`);
const drawer = () => role("review-drawer");
/** The drawer stays mounted through its exit wipe; closed is `data-open="false"`. */
const drawerOpen = () => drawer()?.getAttribute("data-open") === "true";
const press = (name: string) => act(async () => fireEvent.click(screen.getByRole("button", { name })));

describe("review timeline", () => {
  it("lists one-line rows and folds a quiet stretch", async () => {
    await mount(smallSession().rs);
    expect(roles("review-tl-row")).toHaveLength(6);
    const folds = roles("review-tl-fold");
    expect(folds).toHaveLength(1);
    expect(folds[0]?.textContent).toBe("01:0002:00 with nothing of the shown types, folded");
    expect(dots()).toHaveLength(10);
    expect(rowButton(1)?.textContent).toContain("Engine down");
    expect(rowButton(7)?.textContent).toContain("Reveal ×2");
    expect(rowButton(7)?.textContent).toContain("Judge fail");
    expect(rowButton(7)?.textContent).toContain("Persona");
    expect(rowButton(6)?.textContent).toContain("Care");
    expect(rowButton(1)?.textContent).toContain("Alleged");
  });

  it("changes the row size", async () => {
    await mount(smallSession().rs);
    await press("2 min");
    expect(roles("review-tl-row")).toHaveLength(3);
    await press("10 s");
    expect(roles("review-tl-row").length + roles("review-tl-fold").length).toBeGreaterThan(6);
    expect(dots()).toHaveLength(10);
  });

  it("filters dots by kind, with All types and Humour only", async () => {
    const { rs, lines } = smallSession();
    await mount(rs);
    const filter = role("review-tl-filter") as HTMLElement;
    await act(async () => fireEvent.click(within(filter).getByRole("button", { name: "Humour only" })));
    expect(dots().map((d) => Number(d.dataset["line"]))).toEqual([lines.roast, lines.joke, lines.failing]);
    expect(within(filter).getByRole("button", { name: "Welcome" }).getAttribute("aria-pressed")).toBe("false");
    await act(async () => fireEvent.click(within(filter).getByRole("button", { name: "Joke" })));
    expect(dots()).toHaveLength(2);
    await act(async () => fireEvent.click(within(filter).getByRole("button", { name: "All types" })));
    expect(dots()).toHaveLength(10);
    // The legend lists only the kinds this session has.
    expect(within(filter).queryByRole("button", { name: "Tease" })).toBeNull();
    expect(within(filter).getByRole("button", { name: "Stall line" })).toBeTruthy();
  });

  it("rings a voted dot and counts your marks per row", async () => {
    const { rs, lines } = smallSession();
    await mount({ ...rs, feedback: [feedbackRow(lines.roast, ["landed"])] });
    expect(dot(lines.roast)?.dataset["voted"]).toBe("true");
    expect(dot(lines.joke)?.dataset["voted"]).toBeUndefined();
    expect(rowButton(0)?.querySelector("[data-role='review-tl-you']")?.textContent).toBe("✓ 1/4");
    expect(rowButton(1)?.querySelector("[data-role='review-tl-you']")?.textContent).toBe("– /2");
  });

  it("opens the drawer from a row, then a moment, and closes on Esc with focus back on the row", async () => {
    const { rs, lines } = smallSession();
    await mount(rs);
    const row = rowButton(0) as HTMLButtonElement;
    row.focus();
    await act(async () => fireEvent.click(row));
    expect(row.getAttribute("aria-current")).toBe("true");
    expect(document.activeElement).toBe(drawer());
    expect(drawer()?.textContent).toContain("4 of 4 lines shown · 0 marked");
    const items = drawer()?.querySelectorAll("[data-role='review-drawer-lines'] button[data-line]") ?? [];
    expect(items).toHaveLength(4);
    await act(async () => fireEvent.click(drawer()?.querySelector(`button[data-line='${lines.roast}']`) as HTMLElement));
    expect(role("review-moment-kicker")?.textContent).toBe(`00:08 · Improv room · line ${lines.roast}`);
    expect(role("review-moment-tag")?.textContent).toContain("Roast");
    expect(role("review-moment-tag")?.textContent).toContain("aimed at a viewer · sharp edge · “mocks the viewer's question”");
    expect(role("review-moment-judge")?.textContent).toContain("✓ Humour landed");
    expect(role("review-moment-judge")?.textContent).toContain("✗ Stayed in character");
    expect(role("review-script")?.textContent).toContain("DuskPigeon59 asks if I am real");
    await act(async () => fireEvent.click(role("review-drawer-back") as HTMLElement));
    expect(drawer()?.textContent).toContain("4 of 4 lines shown");
    await act(async () => fireEvent.keyDown(drawer() as HTMLElement, { key: "Escape" }));
    expect(drawerOpen()).toBe(false);
    expect(document.activeElement).toBe(rowButton(0));
  });

  it("leaves the note on the first Esc and closes on the second", async () => {
    const { rs, lines } = smallSession();
    await mount(rs, lines.joke);
    const note = drawer()?.querySelector("textarea") as HTMLTextAreaElement;
    note.focus();
    await act(async () => fireEvent.keyDown(note, { key: "Escape" }));
    expect(drawerOpen()).toBe(true);
    expect(document.activeElement).toBe(drawer());
    await act(async () => fireEvent.keyDown(drawer() as HTMLElement, { key: "Escape" }));
    expect(drawerOpen()).toBe(false);
  });

  it("opens the moment from the route's line with its row selected, and Close drops the line from the route", async () => {
    const { rs, lines } = smallSession();
    const m = await mount(rs, lines.failing);
    expect(role("review-moment-kicker")?.textContent).toBe(`03:40 · Investigation · line ${lines.failing}`);
    expect(role("review-moment-judge")?.textContent).toContain("Failed: Viewer asks to be roasted hard");
    expect(rowButton(7)?.getAttribute("aria-current")).toBe("true");
    // The row the link landed on flashes once in the page accent.
    expect(rowButton(7)?.dataset["flash"]).toBe("true");
    expect(rowButton(0)?.dataset["flash"]).toBeUndefined();
    expect(document.activeElement).toBe(drawer());
    expect(screen.getByRole("button", { name: "All lines in 03:30–04:00" })).toBeTruthy();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Close details" })));
    await settle();
    expect(drawerOpen()).toBe(false);
    expect(m.router.getRoute()).toEqual(pageRoute("review", SID, "timeline"));
    // The same deep link opens it again.
    await act(async () => m.router.navigate(pageRoute("review", SID, "timeline", String(lines.failing))));
    await settle();
    expect(role("review-moment-kicker")?.textContent).toContain(`line ${lines.failing}`);
  });

  it("flips the dot's check at once on a save, without reloading the session", async () => {
    const { rs, lines } = smallSession();
    const m = await mount(rs, lines.joke);
    expect(dot(lines.joke)?.dataset["voted"]).toBeUndefined();
    await press("Landed");
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Save" })));
    expect(dot(lines.joke)?.dataset["voted"]).toBe("true");
    await settle();
    expect(screen.getByRole("status").textContent).toBe("Saved");
    expect(m.calls.filter((c) => c === "review.feedback.set")).toHaveLength(1);
    expect(m.calls.filter((c) => c === "review.session")).toHaveLength(1);
  });

  it("sends a host line to the character's Tone Lab", async () => {
    const { rs, lines } = smallSession();
    await mount(rs, lines.joke);
    const link = role("review-to-tone-lab");
    expect(link?.textContent).toBe("Send to Tone Lab");
    expect(link?.getAttribute("href")).toBe(encodeRoute(pageRoute("characters", "malakor", "package", "tone-lab", SID, String(lines.joke))));
  });

  it("rolls the check back when the save fails", async () => {
    const { rs, lines } = smallSession();
    await mount(rs, lines.joke, daemon(rs, () => ({ status: 500, body: { error: { code: "io", message: "disk gone" } } })));
    await press("Flat");
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Save" })));
    await settle();
    expect(dot(lines.joke)?.dataset["voted"]).toBeUndefined();
    expect(screen.getByRole("status").textContent).toContain("was not saved");
  });

  it("renders no enum id", async () => {
    const { rs, lines } = smallSession();
    await mount(rs, lines.stall);
    const text = document.body.textContent ?? "";
    for (const id of ["SEG-01", "SEG-02A", "situation:", "humor_landed", "engine_stall", "start_poll", "reveal_hex", "untagged", "judge_fail"]) expect(text).not.toContain(id);
    expect(role("review-moment-tag")?.textContent).toContain("Stall line");
  });

  it("virtualises a 30-minute, 2,500-event session at 10 s rows and stays under the DOM budget", async () => {
    const rs = bigSession({ events: 2_500, minutes: 30 });
    await mount(rs);
    await press("10 s");
    await settle();
    const viewport = document.querySelector("[data-role='review-tl-rows']");
    expect(viewport?.getAttribute("role")).toBe("list");
    const shownRows = roles("review-tl-row").length;
    expect(shownRows).toBeGreaterThan(0);
    expect(shownRows).toBeLessThan(60);
    await act(async () => fireEvent.click(rowButton(0) as HTMLElement));
    const n = nodes();
    console.info(`dom-count review.l2 timeline at 2,500 events, 10 s rows, drawer open: ${n}`);
    expect(n + WALL).toBeLessThan(L2_BUDGET);
  });
});
