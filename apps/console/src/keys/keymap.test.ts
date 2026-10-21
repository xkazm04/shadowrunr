import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it, vi } from "vitest";
import { conflictReport } from "./conflicts";
import { chordOf, keymapFor, resolve, type Binding, type KeyAction, type KeyLike } from "./keymap";
import { installKeymap, isTypingTarget, type KeyTarget } from "./listener";

/** Seven pages, as the router has today (the shell's live keymap is pinned in shell/keyActions.test.ts). */
const PAGES = ["p1", "p2", "p3", "p4", "p5", "p6", "p7"];
const KEYMAP = keymapFor(PAGES);

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

// Card "Page keys bypass the one keymap: scoped bindings the conflict report sees"
// (challenge-2026-10-21, console-app-bootstrap A): page keys are scoped registry entries.
describe("scoped bindings", () => {
  const scoped = (scope: string) => KEYMAP.filter((b) => b.scope === scope);

  it("reports every scope shadow of a global binding, and nothing else", () => {
    const shadows = conflictReport(KEYMAP).shadows.map((s) => `${s.scope} ${s.mode} ${s.chord}: ${s.scoped} over ${s.global}`);
    expect(shadows.sort()).toEqual(
      [
        ...[1, 2, 3, 4, 5, 6, 7].map((n) => `review.player l2 ${n}: Toggle mark ${n} over Jump to page ${n}`),
        "characters.roster l1 ArrowLeft: Previous character over Previous page",
        "characters.roster l1 ArrowRight: Next character over Next page",
      ].sort(),
    );
    expect(conflictReport(KEYMAP).conflicts).toEqual([]);
  });

  it("catches a seeded duplicate inside a scope", () => {
    const seeded: Binding[] = [...KEYMAP, { chord: "1", modes: ["l2"], scope: "review.player", action: { type: "scoped", scope: "review.player", command: "save" }, label: "Seeded" }];
    expect(conflictReport(seeded).conflicts).toEqual([expect.objectContaining({ scope: "review.player", mode: "l2", chord: "1" })]);
  });

  it("declares the note's Ctrl+Enter in the player scope, typing only", () => {
    const save = scoped("review.player").filter((b) => b.chord === "Ctrl+Enter");
    expect(save.map((b) => b.typing ?? "never").sort()).toEqual(["never", "only"]);
  });

  it("derives the page jumps from the page list", () => {
    const eight = keymapFor([...PAGES, "x"]);
    expect(resolve(eight, "l1", "8", false)?.action).toEqual({ type: "page", index: 7 });
    expect(resolve(KEYMAP, "l1", "8", false)).toBeUndefined();
    expect(readFileSync(join(process.cwd(), "src/keys/keymap.ts"), "utf8")).not.toContain("PAGE_COUNT = 7");
  });
});

describe("one typing rule", () => {
  const el = (tag: string, type?: string) => {
    const e = document.createElement(tag);
    if (type) e.setAttribute("type", type);
    return e;
  };

  it("counts text fields and the video, never a checkbox or a radio", () => {
    expect(isTypingTarget(el("input"))).toBe(true);
    expect(isTypingTarget(el("input", "text"))).toBe(true);
    expect(isTypingTarget(el("textarea"))).toBe(true);
    expect(isTypingTarget(el("video"))).toBe(true);
    expect(isTypingTarget(el("input", "checkbox"))).toBe(false);
    expect(isTypingTarget(el("input", "radio"))).toBe(false);
    expect(isTypingTarget(el("div"))).toBe(false);
  });

  it("is the only one: no page keeps a typing rule or a keydown handler of its own for the moved keys", () => {
    const pages = join(process.cwd(), "src/pages");
    const typingRules = sources(pages).filter((f) => /function typing\b/.test(readFileSync(f, "utf8")));
    expect(typingRules).toEqual([]);
    for (const f of ["review/Player.tsx", "characters/Layer1.tsx"]) expect(readFileSync(join(pages, f), "utf8")).not.toContain("onKeyDown");
  });
});

describe("the listener with owners", () => {
  function fireAt(mode: "l1" | "l2", key: string, owns: (a: KeyAction) => boolean, target?: EventTarget) {
    const held: { listener?: (e: KeyboardEvent) => void } = {};
    const t: KeyTarget = { addEventListener: (_t, l) => (held.listener = l), removeEventListener: () => delete held.listener };
    const dispatch = vi.fn();
    installKeymap(t, { keymap: KEYMAP, getMode: () => mode, dispatch, owns });
    const e = new KeyboardEvent("keydown", { key, cancelable: true });
    if (target) Object.defineProperty(e, "target", { value: target });
    held.listener?.(e);
    return { e, dispatch };
  }

  it("leaves 'r' alone when no page owns the re-run", () => {
    const { e, dispatch } = fireAt("l2", "r", (a) => a.type !== "command");
    expect(e.defaultPrevented).toBe(false);
    expect(dispatch).not.toHaveBeenCalled();
    const owned = fireAt("l2", "r", () => true);
    expect(owned.e.defaultPrevented).toBe(true);
    expect(owned.dispatch).toHaveBeenCalledWith({ type: "command", command: "rerun" }, owned.e);
  });

  it("resolves the innermost scope first, the global binding outside it", () => {
    const box = document.createElement("div");
    box.setAttribute("data-key-scope", "review.player");
    const inner = document.createElement("button");
    box.append(inner);
    document.body.append(box);
    try {
      expect(fireAt("l2", "3", () => true, inner).dispatch).toHaveBeenCalledWith({ type: "scoped", scope: "review.player", command: "toggle", arg: 2 }, expect.anything());
      expect(fireAt("l2", "3", () => true, document.body).dispatch).toHaveBeenCalledWith({ type: "page", index: 2 }, expect.anything());
      // A scope with no owner mounted falls through to the global binding.
      expect(fireAt("l2", "3", (a) => a.type !== "scoped", inner).dispatch).toHaveBeenCalledWith({ type: "page", index: 2 }, expect.anything());
    } finally {
      box.remove();
    }
  });
});

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return sources(p);
    return /\.tsx?$/.test(f) && !/\.test\.tsx?$/.test(f) ? [p] : [];
  });
}
