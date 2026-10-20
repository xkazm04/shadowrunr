// The review player in jsdom on the data layer's doubles: it walks one kind skipping marked
// lines, Enter saves through `review.feedback.set` and moves on, ← revisits a marked line, the
// done state sums the marks, and its keys never reach the shell's page jumps or the note.
import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ReviewSession } from "@shadowrunr/contract";
import { mountPage, ok, settle } from "../../kit/testing";
import { pageRoute } from "../../router";
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
const press = async (key: string, target: Element | null = player(), mods: { ctrlKey?: boolean } = {}) => {
  let passed = true;
  await act(async () => {
    passed = fireEvent.keyDown(target ?? document.body, { key, ...mods });
  });
  await settle(4);
  return passed;
};

describe("the review player", () => {
  it("starts on the first unmarked line, skips marked ones, and focuses itself for keys", async () => {
    const base = reviewSession();
    const all = roasts(base);
    expect(all.length).toBeGreaterThan(3);
    await mount(reviewSession({ marked: [all[0] ?? 0, all[2] ?? 0] }));
    expect(current()).toBe(all[1]);
    expect(document.activeElement).toBe(player());
    expect(screen.getByText(`Roast 2 of ${all.length}`)).toBeTruthy();
    expect(screen.getByText(`2 marked · ${all.length - 2} to go`)).toBeTruthy();
    await press("ArrowRight");
    expect(current()).toBe(all[3]);
  });

  it("toggles marks with digits and saves with Enter through review.feedback.set, then moves on", async () => {
    const all = roasts(reviewSession());
    const { sent } = await mount(reviewSession());
    expect(current()).toBe(all[0]);
    await press("1");
    await press("4");
    expect(screen.getByRole("button", { name: /Landed/ }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByRole("button", { name: /Broke character/ }).getAttribute("aria-pressed")).toBe("true");
    await press("Enter");
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
    await press("ArrowLeft");
    expect(current()).toBe(all[0]);
    expect(screen.getByRole("button", { name: /Landed/ }).getAttribute("aria-pressed")).toBe("true");
    expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(true);
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
    expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(false);
    await press("ArrowRight");
    expect(current()).toBe(all[1]);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Open on the timeline" })));
    expect(router.getRoute()).toEqual(pageRoute("review", SESSION, "timeline", String(all[1])));
  });

  it("finishes after the last unmarked line is saved", async () => {
    const all = roasts(reviewSession());
    await mount(reviewSession({ marked: all.slice(1) }));
    expect(current()).toBe(all[0]);
    await press("2");
    await press("Enter");
    expect(document.querySelector("[data-role='review-player-done']")?.textContent).toContain("Every line has your mark");
  });

  it("consumes its keys so the shell's page jumps never fire, and leaves the note alone", async () => {
    const all = roasts(reviewSession());
    const { sent } = await mount(reviewSession());
    // The shell's keymap skips an event a widget already prevented (keys/listener.ts).
    expect(await press("3")).toBe(false);
    expect(screen.getByRole("button", { name: /Too mean/ }).getAttribute("aria-pressed")).toBe("true");
    const note = document.querySelector("textarea");
    expect(await press("1", note)).toBe(true);
    expect(await press("Enter", note)).toBe(true);
    expect(await press("ArrowRight", note)).toBe(true);
    expect(screen.getByRole("button", { name: /Landed/ }).getAttribute("aria-pressed")).toBe("false");
    expect(current()).toBe(all[0]);
    expect(sent).toHaveLength(0);
    // Ctrl+Enter in the note saves and moves on.
    expect(await press("Enter", note, { ctrlKey: true })).toBe(false);
    expect(sent).toHaveLength(1);
    expect(current()).toBe(all[1]);
    // A modifier other than Ctrl+Enter is left to the shell.
    expect(await press("2", player(), { ctrlKey: true })).toBe(true);
  });
});
