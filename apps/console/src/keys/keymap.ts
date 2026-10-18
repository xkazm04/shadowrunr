// The one keymap registry. Every global key binding the console has is declared here, per mode,
// and `conflictReport` (run as a test) proves no chord means two things in one mode.
//
// Bindings kept from the winner and A/1: bare 1-6 jump to a page at every level, M toggles the
// wall, Esc goes up one segment, arrows move along the wall or between posters. Owner's router
// fix: Esc on the wall does nothing (the winner opened a poster).

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
  | { readonly type: "command"; readonly command: KeyCommand };

/** A page command (kit/keyCommands.ts): the page on screen that owns it runs it. */
export type KeyCommand = "rerun";

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
}

const NAV: readonly Mode[] = ["wall", "l1", "l2"];
const PAGE_COUNT = 6;

export const KEYMAP: readonly Binding[] = [
  { chord: "*", modes: ["intro"], action: { type: "skip-intro" }, label: "Skip the intro" },
  ...Array.from({ length: PAGE_COUNT }, (_, i): Binding => ({
    chord: String(i + 1),
    modes: NAV,
    action: { type: "page", index: i },
    label: `Jump to page ${i + 1}`,
  })),
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
];

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

export function resolve(keymap: readonly Binding[], mode: Mode, chord: string, typing: boolean): Binding | undefined {
  const scope: TypingScope = typing ? "only" : "never";
  return keymap.find(
    (b) => b.modes.includes(mode) && (b.typing ?? "never") === scope && (b.chord === chord || b.chord === "*"),
  );
}
