// The review player in jsdom on the data layer's doubles: it walks one kind skipping marked
// lines, save goes through `review.feedback.set` and moves on, back revisits a marked line, and
// the done state sums the marks. Its keys are the `review.player` scope of the one keymap: here the
// test runs the scope's commands as the keymap emits them (kit/keyCommands.ts emitScoped); which
// key means which command, and that the shell's page jumps never see them, is pinned end to end
// through the real shell in app/pagekeys.dom.test.tsx.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ReviewSession } from "@shadowrunr/contract";
import { contractDoor } from "../../data/contractDoor";
import { createDataLayer, DataProvider } from "../../data";
import { fakeSockets, manualTimers, scriptedFetch, type FetchReply } from "../../data/testing";
import { emitScoped, type ScopeCommands } from "../../kit/keyCommands";
import { fail, mountPage, ok, settle } from "../../kit/testing";
import { createHashRouter, pageRoute, RouterProvider } from "../../router";
import { derive } from "./derive";
import { Layer2 } from "./Layer2";
import { reviewSession, SESSION } from "./tabs.fixture";

let dispose: (() => void) | null = null;
afterEach(() => {
  cleanup();
  dispose?.();
  dispose = null;
});

/** The roast lines the player walks (derive decides stall lines, so ask it). */
const roasts = (rs: ReviewSession) => derive(rs).says.filter((s) => s.kind === "roast").map((s) => s.line);

async function mount(rs: ReviewSession) {
  const sent: string[] = [];
  const m = mountPage(
    <Layer2 sub={[SESSION, "player", "roast"]} />,
    (method, body) => {
      if (method === "review.session") return ok(rs);
      if (method === "review.feedback.set") {
        sent.push(body);
        const line = Number(/"line":(\d+)/.exec(body)?.[1]);
        const tags = [...body.matchAll(/"(landed|flat|too_mean|broke_character|off_tone|great_roast|repetitive|wrong_tag)"/g)].map((x) => x[1]);
        return ok({ session_id: SESSION, line, tags, at_ms: 1 });
      }
      if (method === "review.sessions" || method === "review.feedback.list") return ok([]);
      return { status: 404, body: { error: { code: "method_not_found", message: method } } };
    },
    `#/review/${SESSION}/player/roast`,
  );
  dispose = m.dispose;
  await settle(8);
  return { ...m, sent };
}

const player = () => document.querySelector<HTMLElement>("[data-role='review-player']");
const current = () => Number(player()?.getAttribute("data-line"));
type PlayerCommand = keyof ScopeCommands["review.player"];
/** Runs one of the player scope's commands, as its key would through the shell's keymap. */
const run = async (command: PlayerCommand, arg?: number) => {
  let owned = false;
  await act(async () => {
    owned = command === "toggle" ? emitScoped({ type: "scoped", scope: "review.player", command, arg: arg ?? 0 }) : emitScoped({ type: "scoped", scope: "review.player", command });
  });
  await settle(4);
  return owned;
};
const mark = (n: number) => run("toggle", n - 1);

describe("the review player", () => {
  it("starts on the first unmarked line, skips marked ones, and focuses itself so its key scope holds", async () => {
    const base = reviewSession();
    const all = roasts(base);
    expect(all.length).toBeGreaterThan(3);
    await mount(reviewSession({ marked: [all[0] ?? 0, all[2] ?? 0] }));
    expect(current()).toBe(all[1]);
    expect(document.activeElement).toBe(player());
    expect(screen.getByText(`Roast 2 of ${all.length}`)).toBeTruthy();
    expect(screen.getByText(`2 marked · ${all.length - 2} to go`)).toBeTruthy();
    await run("skip");
    expect(current()).toBe(all[3]);
  });

  it("toggles marks with digits and saves with Enter through review.feedback.set, then moves on", async () => {
    const all = roasts(reviewSession());
    const { sent } = await mount(reviewSession());
    expect(current()).toBe(all[0]);
    await mark(1);
    await mark(4);
    expect(screen.getByRole("button", { name: /Landed/ }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByRole("button", { name: /Broke character/ }).getAttribute("aria-pressed")).toBe("true");
    await run("save");
    expect(sent).toHaveLength(1);
    expect(sent[0]).toContain(`"line":${all[0]}`);
    expect(sent[0]).toContain('"landed"');
    expect(sent[0]).toContain('"broke_character"');
    expect(current()).toBe(all[1]);
    expect(screen.getByText(`1 marked · ${all.length - 1} to go`)).toBeTruthy();
  });

  it("goes back to a marked line with ←, which the skip filter would otherwise hide", async () => {
    const all = roasts(reviewSession());
    await mount(reviewSession({ marked: [all[0] ?? 0] }));
    expect(current()).toBe(all[1]);
    await run("back");
    expect(current()).toBe(all[0]);
    expect(screen.getByRole("button", { name: /Landed/ }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByRole("switch", { name: /^Skip lines I already marked/ }).getAttribute("aria-checked")).toBe("true");
  });

  it("shows the done state with mark totals, and steps through all of them again", async () => {
    const all = roasts(reviewSession());
    const { router } = await mount(reviewSession({ marked: all }));
    const done = document.querySelector("[data-role='review-player-done']");
    expect(done?.textContent).toContain("Every line has your mark");
    expect(done?.textContent).toMatch(/Landed\s*\d+/);
    expect(done?.textContent).toMatch(/Flat\s*\d+/);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: `Step through all ${all.length} again` })));
    expect(current()).toBe(all[0]);
    expect(screen.getByRole("switch", { name: /^Skip lines I already marked/ }).getAttribute("aria-checked")).toBe("false");
    await run("skip");
    expect(current()).toBe(all[1]);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Open on the timeline" })));
    expect(router.getRoute()).toEqual(pageRoute("review", SESSION, "timeline", String(all[1])));
  });

  it("finishes after the last unmarked line is saved", async () => {
    const all = roasts(reviewSession());
    await mount(reviewSession({ marked: all.slice(1) }));
    expect(current()).toBe(all[0]);
    await mark(2);
    await run("save");
    expect(document.querySelector("[data-role='review-player-done']")?.textContent).toContain("Every line has your mark");
  });
});

/** Feedback writes on hold: each `review.feedback.set` waits until the test answers it. */
async function mountHeld(rs: ReviewSession) {
  const sent: string[] = [];
  const held: Array<(reply: FetchReply) => void> = [];
  window.location.hash = `#/review/${SESSION}/player/roast`;
  const router = createHashRouter(window);
  const answer = (method: string): FetchReply => {
    if (method === "review.session") return ok(rs);
    if (method === "review.sessions" || method === "review.feedback.list") return ok([]);
    return { status: 404, body: { error: { code: "method_not_found", message: method } } };
  };
  const now = scriptedFetch((method) => answer(method));
  const fetch: typeof now = Object.assign(
    (url: string, init: RequestInit) => {
      const body = typeof init.body === "string" ? init.body : "";
      if (!body.includes('"method":"review.feedback.set"')) return now(url, init);
      sent.push(body);
      return new Promise<Awaited<ReturnType<typeof now>>>((resolve) => {
        held.push((reply) => resolve(scriptedFetch(() => reply)(url, init)));
      });
    },
    { calls: now.calls },
  );
  const layer = createDataLayer({ baseUrl: "http://127.0.0.1:7717", door: contractDoor, fetch, socket: fakeSockets().factory, timers: manualTimers() });
  render(
    <DataProvider layer={layer}>
      <RouterProvider router={router}>
        <Layer2 sub={[SESSION, "player", "roast"]} />
      </RouterProvider>
    </DataProvider>,
  );
  dispose = () => router.dispose();
  await settle(8);
  /** Answers the oldest held write. */
  const release = async (reply: FetchReply) => {
    const r = held.shift();
    expect(r).toBeTruthy();
    await act(async () => r?.(reply));
    await settle(6);
  };
  return { sent, held, release };
}

const setOk = (sent: string) => {
  const line = Number(/"line":(\d+)/.exec(sent)?.[1]);
  const tags = [...sent.matchAll(/"(landed|flat|too_mean|broke_character|off_tone|great_roast|repetitive|wrong_tag)"/g)].map((x) => x[1]);
  return ok({ session_id: SESSION, line, tags, at_ms: 1 });
};
const saveButton = () => screen.getByRole("button", { name: /Save and next/ });

describe("the review player with feedback writes in flight", () => {
  it("advances to the next roast before the daemon answers the write", async () => {
    const all = roasts(reviewSession());
    const { sent, held, release } = await mountHeld(reviewSession());
    expect(current()).toBe(all[0]);
    // Skip off: the render-time skip of marked lines must not be what moves the player on.
    await act(async () => fireEvent.click(screen.getByRole("switch", { name: /^Skip lines I already marked/ })));
    expect(current()).toBe(all[0]);
    await mark(1);
    await run("save");
    expect(sent).toHaveLength(1);
    expect(held).toHaveLength(1);
    expect(current()).toBe(all[1]);
    await release(setOk(sent[0] ?? ""));
    expect(current()).toBe(all[1]);
  });

  it("keeps Save enabled on another line while one line's write is in flight", async () => {
    const all = roasts(reviewSession());
    const { sent, release } = await mountHeld(reviewSession());
    await mark(1);
    await run("save");
    if (current() === all[0]) await run("skip");
    expect(current()).toBe(all[1]);
    expect((saveButton() as HTMLButtonElement).disabled).toBe(false);
    await release(setOk(sent[0] ?? ""));
  });

  it("sends exactly one write when Enter is pressed again on a line whose write is pending", async () => {
    const all = roasts(reviewSession());
    const { sent, release } = await mountHeld(reviewSession());
    await mark(1);
    await run("save");
    if (current() !== all[0]) await run("back");
    expect(current()).toBe(all[0]);
    await run("save");
    expect(sent).toHaveLength(1);
    await release(setOk(sent[0] ?? ""));
  });

  it("shows a failed write as a retryable notice naming the line; Retry resends the same mark", async () => {
    const all = roasts(reviewSession());
    const { sent, release } = await mountHeld(reviewSession());
    await mark(1);
    await mark(2);
    const note = document.querySelector("textarea");
    expect(note).toBeTruthy();
    await act(async () => fireEvent.change(note as HTMLTextAreaElement, { target: { value: "  sharper  " } }));
    await run("save");
    await release(fail("io", "disk full"));
    const notice = document.querySelector("[data-role='review-failed-marks']");
    expect(notice?.textContent).toMatch(/1 mark not saved/);
    expect(notice?.textContent).toContain(`line ${all[0]}`);
    expect(screen.getByText(`0 marked · ${all.length} to go`)).toBeTruthy();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: /Retry/ })));
    await settle(4);
    expect(sent).toHaveLength(2);
    expect(sent[1]).toContain(`"line":${all[0]}`);
    expect(sent[1]).toContain('"landed"');
    expect(sent[1]).toContain('"flat"');
    expect(sent[1]).toContain('"note":"sharper"');
    await release(setOk(sent[1] ?? ""));
    expect(document.querySelector("[data-role='review-failed-marks']")).toBeNull();
    expect(screen.getByText(`1 marked · ${all.length - 1} to go`)).toBeTruthy();
  });
});
