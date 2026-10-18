import { describe, expect, it } from "vitest";
import { WALL, pageRoute } from "../router";
import { effectOf, modeOf, type KeyContext } from "./keyActions";

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
    expect(effectOf({ type: "wall-focus", delta: -1 }, ctx({ route: WALL, hover: 0 }))).toEqual({ kind: "hover", index: 5 });
    expect(effectOf({ type: "page-step", delta: 1 }, ctx({ route: pageRoute("reconcile") }))).toEqual({
      kind: "navigate",
      route: pageRoute("onair"),
    });
  });
});
