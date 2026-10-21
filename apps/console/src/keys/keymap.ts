// The one keymap registry. Every key binding the console has is declared here, per mode, and
// `conflictReport` (run as a test) proves no chord means two things in one mode and scope, and
// lists every place a page's scoped binding shadows a global one.
//
// Bindings kept from the winner and A/1: bare 1-7 jump to a page at every level, M toggles the
// wall, Esc goes up one segment, arrows move along the wall or between posters. Owner's router
// fix: Esc on the wall does nothing (the winner opened a poster). Page keys (the review player's
// marks, the roster's arrows) are scoped bindings (keys/scopes.ts), resolved innermost-first.
//
// keys imports nothing else in the console (eslint.config.js boundaries), so the page list comes
// in: the shell builds the live keymap as `keymapFor(PAGE_IDS)` (shell/keyActions.ts KEYMAP).
import type { KeyCommand, KeyScope, ScopedAction } from "./scopes";

export const MODES = ["intro", "wall", "l1", "l2"] as const;
export type Mode = (typeof MODES)[number];

export type KeyAction =
  | { readonly type: "page"; readonly index: number }
  | { readonly type: "up" }
  | { readonly type: "toggle-wall" }
  | { readonly type: "wall-focus"; readonly delta: 1 | -1 }
  | { readonly type: "page-step"; readonly delta: 1 | -1 }
  | { readonly type: "skip-intro" }
  | { readonly type: "blur" }
  /** Open the Go to palette (shell/GoToPalette.tsx): a page, a recent place or a pasted ref. */
  | { readonly type: "goto" }
  | { readonly type: "command"; readonly command: KeyCommand }
  /** A command of the scope's owner (kit/keyCommands.ts useKeyScope). */
  | ScopedAction;

/**
 * When a binding fires relative to text entry: `never` (the default) does not fire while an
 * input, textarea, select or contenteditable has focus; `only` fires only then.
 */
export type TypingScope = "never" | "only";

export interface Binding {
  /** A normalised chord: `1`, `m`, `Escape`, `ArrowLeft`, `Ctrl+1`; `*` is any key. */
  readonly chord: string;
  readonly modes: readonly Mode[];
  readonly action: KeyAction;
  /** What the binding does, as shown in help and in the conflict report. */
  readonly label: string;
  readonly typing?: TypingScope;
  /** Set: the binding lives only while focus is inside `[data-key-scope=<scope>]` and its owner is mounted. */
  readonly scope?: KeyScope;
}

const NAV: readonly Mode[] = ["wall", "l1", "l2"];

/** The review player's marks: 1-8 in the FEEDBACK order (pages/review/names.ts). */
const MARK_COUNT = 8;

/** A review-player binding in layer 2. */
const player = (chord: string, action: ScopedAction & { scope: "review.player" }, label: string, typing?: TypingScope): Binding => ({
  chord,
  modes: ["l2"],
  scope: "review.player",
  action,
  label,
  ...(typing ? { typing } : {}),
});
const SAVE = { type: "scoped", scope: "review.player", command: "save" } as const;

const PLAYER: readonly Binding[] = [
  ...Array.from({ length: MARK_COUNT }, (_, i) => player(String(i + 1), { type: "scoped", scope: "review.player", command: "toggle", arg: i }, `Toggle mark ${i + 1}`)),
  player("Enter", SAVE, "Save and next"),
  player("Ctrl+Enter", SAVE, "Save and next"),
  player("Ctrl+Enter", SAVE, "Save the note and next", "only"),
  player("ArrowRight", { type: "scoped", scope: "review.player", command: "skip" }, "Skip the line"),
  player("ArrowLeft", { type: "scoped", scope: "review.player", command: "back" }, "Back one line"),
];

const ROSTER: readonly Binding[] = ([-1, 1] as const).map((arg): Binding => ({
  chord: arg < 0 ? "ArrowLeft" : "ArrowRight",
  modes: ["l1"],
  scope: "characters.roster",
  action: { type: "scoped", scope: "characters.roster", command: "step", arg },
  label: arg < 0 ? "Previous character" : "Next character",
}));

/** The keymap for a page list: one bare digit per page (at most nine), then everything else. */
export function keymapFor(pages: readonly string[]): readonly Binding[] {
  return [
    { chord: "*", modes: ["intro"], action: { type: "skip-intro" }, label: "Skip the intro" },
    ...pages.slice(0, 9).map(
      (_, i): Binding => ({
        chord: String(i + 1),
        modes: NAV,
        action: { type: "page", index: i },
        label: `Jump to page ${i + 1}`,
      }),
    ),
    { chord: "Ctrl+k", modes: NAV, action: { type: "goto" }, label: "Go to anything" },
    { chord: "m", modes: NAV, action: { type: "toggle-wall" }, label: "Toggle the season wall" },
    { chord: "Escape", modes: ["l1", "l2"], action: { type: "up" }, label: "Up one level" },
    { chord: "Escape", modes: NAV, action: { type: "blur" }, label: "Leave the text field", typing: "only" },
    { chord: "ArrowLeft", modes: ["wall"], action: { type: "wall-focus", delta: -1 }, label: "Previous poster" },
    { chord: "ArrowUp", modes: ["wall"], action: { type: "wall-focus", delta: -1 }, label: "Previous poster" },
    { chord: "ArrowRight", modes: ["wall"], action: { type: "wall-focus", delta: 1 }, label: "Next poster" },
    { chord: "ArrowDown", modes: ["wall"], action: { type: "wall-focus", delta: 1 }, label: "Next poster" },
    { chord: "ArrowLeft", modes: ["l1"], action: { type: "page-step", delta: -1 }, label: "Previous page" },
    { chord: "ArrowRight", modes: ["l1"], action: { type: "page-step", delta: 1 }, label: "Next page" },
    { chord: "r", modes: ["l2"], action: { type: "command", command: "rerun" }, label: "Re-run the page's check" },
    ...PLAYER,
    ...ROSTER,
  ];
}

/** The fields of a KeyboardEvent the keymap reads. */
export interface KeyLike {
  readonly key: string;
  readonly ctrlKey: boolean;
  readonly metaKey: boolean;
  readonly altKey: boolean;
}

const MODIFIER_KEYS = new Set(["Control", "Meta", "Alt", "Shift", "AltGraph", "CapsLock"]);

export function isModifierOnly(e: KeyLike): boolean {
  return MODIFIER_KEYS.has(e.key);
}

/** Ctrl and Cmd are one modifier. Single characters are lower-cased (Shift is not a chord). */
export function chordOf(e: KeyLike): string {
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  return `${e.ctrlKey || e.metaKey ? "Ctrl+" : ""}${e.altKey ? "Alt+" : ""}${key}`;
}

const matches = (b: Binding, mode: Mode, chord: string, typing: boolean) =>
  b.modes.includes(mode) && (b.typing ?? "never") === (typing ? "only" : "never") && (b.chord === chord || b.chord === "*");

/** The global binding for a chord (scoped bindings are left out). */
export function resolve(keymap: readonly Binding[], mode: Mode, chord: string, typing: boolean): Binding | undefined {
  return keymap.find((b) => b.scope === undefined && matches(b, mode, chord, typing));
}

/**
 * The binding a key press means: the innermost scope around the target first, then the global
 * bindings. A scoped or command binding counts only when `owns` says its owner is mounted, so a
 * key nobody owns falls through (and, with no global meaning, is left un-prevented).
 */
export function resolveIn(
  keymap: readonly Binding[],
  mode: Mode,
  chord: string,
  typing: boolean,
  scopes: readonly KeyScope[],
  owns: (action: KeyAction) => boolean,
): Binding | undefined {
  for (const scope of scopes) {
    const hit = keymap.find((b) => b.scope === scope && matches(b, mode, chord, typing) && owns(b.action));
    if (hit) return hit;
  }
  return keymap.find((b) => b.scope === undefined && matches(b, mode, chord, typing) && (b.action.type !== "command" || owns(b.action)));
}
