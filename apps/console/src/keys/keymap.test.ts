import { describe, expect, it, vi } from "vitest";
import { conflictReport } from "./conflicts";
import { KEYMAP, chordOf, resolve, type Binding, type KeyLike } from "./keymap";
import { installKeymap, type KeyTarget } from "./listener";

const key = (k: string, mods: Partial<KeyLike> = {}): KeyLike => ({ key: k, ctrlKey: false, metaKey: false, altKey: false, ...mods });

describe("keymap conflict report", () => {
  const report = conflictReport(KEYMAP);

  it("has no chord that means two things in one mode", () => {
    expect(report.conflicts).toEqual([]);
    expect(report.bindings).toBeGreaterThan(0);
  });

  it("lists every chord whose meaning changes between modes, for the owner", () => {
    const changed = Object.fromEntries(report.meaningChanges.map((c) => [c.chord, c.meanings]));
    expect(Object.keys(changed).sort()).toEqual(["ArrowLeft", "ArrowRight"]);
    expect(changed["ArrowLeft"]).toEqual({ wall: "Previous poster", l1: "Previous page" });
  });

  it("catches a seeded conflict", () => {
    const seeded: Binding[] = [...KEYMAP, { chord: "m", modes: ["l2"], action: { type: "up" }, label: "Seeded" }];
    expect(conflictReport(seeded).conflicts).toEqual([
      { mode: "l2", chord: "m", typing: "never", labels: ["Toggle the season wall", "Seeded"] },
    ]);
  });

  it("treats the intro's any-key wildcard as clashing with anything else in that mode", () => {
    const seeded: Binding[] = [...KEYMAP, { chord: "1", modes: ["intro"], action: { type: "up" }, label: "Seeded" }];
    expect(conflictReport(seeded).conflicts.map((c) => c.mode)).toContain("intro");
  });
});

describe("bindings", () => {
  it("jumps to a page with bare 1-7 at every level", () => {
    for (const mode of ["wall", "l1", "l2"] as const) {
      for (let i = 1; i <= 7; i++) expect(resolve(KEYMAP, mode, String(i), false)?.action).toEqual({ type: "page", index: i - 1 });
    }
    expect(resolve(KEYMAP, "l1", "8", false)).toBeUndefined();
  });

  it("goes up on Esc, except on the wall (the owner's router fix)", () => {
    expect(resolve(KEYMAP, "l1", "Escape", false)?.action).toEqual({ type: "up" });
    expect(resolve(KEYMAP, "l2", "Escape", false)?.action).toEqual({ type: "up" });
    expect(resolve(KEYMAP, "wall", "Escape", false)).toBeUndefined();
  });

  it("only leaves the field on Esc while typing, and ignores page keys there", () => {
    expect(resolve(KEYMAP, "l2", "Escape", true)?.action).toEqual({ type: "blur" });
    expect(resolve(KEYMAP, "l2", "1", true)).toBeUndefined();
  });

  it("opens Go to on Ctrl+K at every level, never while typing, without a conflict", () => {
    for (const mode of ["wall", "l1", "l2"] as const) expect(resolve(KEYMAP, mode, "Ctrl+k", false)?.action).toEqual({ type: "goto" });
    expect(resolve(KEYMAP, "l2", "Ctrl+k", true)).toBeUndefined();
    expect(chordOf(key("K", { ctrlKey: true }))).toBe("Ctrl+k");
    expect(conflictReport(KEYMAP).conflicts).toEqual([]);
  });

  it("skips the intro on any key", () => {
    expect(resolve(KEYMAP, "intro", "q", false)?.action).toEqual({ type: "skip-intro" });
  });

  it("normalises chords: Ctrl and Cmd are one modifier, letters are lower-case", () => {
    expect(chordOf(key("M"))).toBe("m");
    expect(chordOf(key("1", { metaKey: true }))).toBe("Ctrl+1");
    expect(chordOf(key("Escape"))).toBe("Escape");
    expect(resolve(KEYMAP, "l1", "Ctrl+2", false)).toBeUndefined();
  });
});

describe("the one listener", () => {
  function harness(mode: "wall" | "l1" = "l1") {
    let listener: ((e: KeyboardEvent) => void) | null = null;
    const target: KeyTarget = {
      addEventListener: (_t, l) => (listener = l),
      removeEventListener: () => (listener = null),
    };
    const dispatch = vi.fn();
    const off = installKeymap(target, { keymap: KEYMAP, getMode: () => mode, dispatch });
    const fire = (init: KeyboardEventInit, target?: EventTarget, handled = false) => {
      const e = new KeyboardEvent("keydown", { cancelable: true, ...init });
      if (handled) e.preventDefault();
      if (target) Object.defineProperty(e, "target", { value: target });
      listener?.(e);
      return e;
    };
    return { dispatch, off, fire, installed: () => listener !== null };
  }

  it("dispatches a bound key and prevents its default", () => {
    const h = harness();
    const e = h.fire({ key: "3" });
    expect(h.dispatch).toHaveBeenCalledWith({ type: "page", index: 2 }, e);
    expect(e.defaultPrevented).toBe(true);
  });

  it("leaves keys a widget already handled", () => {
    const h = harness();
    h.fire({ key: "ArrowRight" }, undefined, true);
    expect(h.dispatch).not.toHaveBeenCalled();
    h.fire({ key: "ArrowRight" });
    expect(h.dispatch).toHaveBeenCalledWith({ type: "page-step", delta: 1 }, expect.anything());
  });

  it("ignores page keys typed into a field", () => {
    const h = harness();
    const input = document.createElement("input");
    h.fire({ key: "1" }, input);
    expect(h.dispatch).not.toHaveBeenCalled();
    h.fire({ key: "Escape" }, input);
    expect(h.dispatch).toHaveBeenCalledWith({ type: "blur" }, expect.anything());
  });

  it("uninstalls", () => {
    const h = harness();
    h.off();
    expect(h.installed()).toBe(false);
  });
});
