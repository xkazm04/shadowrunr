import { describe, expect, it } from "vitest";
import { conflictReport, keymapFor, resolve } from "../keys";
import { PAGE_IDS, WALL, pageRoute } from "../router";
import { effectOf, KEYMAP, modeOf, type KeyContext } from "./keyActions";

const ctx = (over: Partial<KeyContext> = {}): KeyContext => ({
  route: pageRoute("brain"),
  intro: false,
  lastPage: "brain",
  hover: null,
  ...over,
});

describe("key actions", () => {
  it("derives the keymap mode from the route", () => {
    expect(modeOf(ctx({ route: WALL }))).toBe("wall");
    expect(modeOf(ctx())).toBe("l1");
    expect(modeOf(ctx({ route: pageRoute("brain", "transcripts") }))).toBe("l2");
    expect(modeOf(ctx({ intro: true }))).toBe("intro");
  });

  it("goes up one level, and nowhere from the wall", () => {
    expect(effectOf({ type: "up" }, ctx({ route: pageRoute("brain", "transcripts", "r1") }))).toEqual({
      kind: "navigate",
      route: pageRoute("brain", "transcripts"),
    });
    expect(effectOf({ type: "up" }, ctx())).toEqual({ kind: "navigate", route: WALL });
    expect(effectOf({ type: "up" }, ctx({ route: WALL }))).toEqual({ kind: "none" });
  });

  it("toggles the wall back to the last page", () => {
    expect(effectOf({ type: "toggle-wall" }, ctx())).toEqual({ kind: "navigate", route: WALL });
    expect(effectOf({ type: "toggle-wall" }, ctx({ route: WALL, lastPage: "simulator" }))).toEqual({
      kind: "navigate",
      route: pageRoute("simulator"),
    });
  });

  it("wraps arrow movement on the wall and between pages", () => {
    expect(effectOf({ type: "wall-focus", delta: -1 }, ctx({ route: WALL, hover: 0 }))).toEqual({ kind: "hover", index: 6 });
    expect(effectOf({ type: "page-step", delta: 1 }, ctx({ route: pageRoute("reconcile") }))).toEqual({
      kind: "navigate",
      route: pageRoute("onair"),
    });
  });

  it("opens the go-to palette on its key", () => {
    expect(effectOf({ type: "goto" }, ctx({ route: pageRoute("brain", "transcripts") }))).toEqual({ kind: "open-goto" });
    expect(effectOf({ type: "goto" }, ctx({ route: WALL }))).toEqual({ kind: "open-goto" });
  });
});

// Card "Page keys bypass the one keymap" (challenge-2026-10-21): the live keymap.
describe("the live keymap", () => {
  it("derives a page jump per page in PAGE_IDS: an eighth page is one router line", () => {
    expect(KEYMAP).toEqual(keymapFor(PAGE_IDS));
    expect(resolve(keymapFor([...PAGE_IDS, "x"]), "l1", "8", false)?.action).toEqual({ type: "page", index: 7 });
    expect(resolve(KEYMAP, "l1", String(PAGE_IDS.length + 1), false)).toBeUndefined();
  });

  it("has no conflict, and lists the page scopes' shadows of the global keys", () => {
    const report = conflictReport(KEYMAP);
    expect(report.conflicts).toEqual([]);
    expect(report.shadows.map((s) => `${s.scope} ${s.mode} ${s.chord}`).sort()).toEqual(
      [...[1, 2, 3, 4, 5, 6, 7].map((n) => `review.player l2 ${n}`), "characters.roster l1 ArrowLeft", "characters.roster l1 ArrowRight"].sort(),
    );
  });

  it("hands a scoped action to its scope's owner", () => {
    const action = { type: "scoped", scope: "review.player", command: "toggle", arg: 2 } as const;
    expect(effectOf(action, ctx())).toEqual({ kind: "scoped", action });
  });
});
