// The only keydown listener in the console. Everything else declares bindings in `KEYMAP`: global
// ones, and page keys scoped to a `data-key-scope` element (keys/scopes.ts).
import { chordOf, isModifierOnly, resolveIn, type Binding, type KeyAction, type Mode } from "./keymap";
import { scopeChain } from "./scopes";

export interface KeyTarget {
  addEventListener(type: "keydown", listener: (e: KeyboardEvent) => void): void;
  removeEventListener(type: "keydown", listener: (e: KeyboardEvent) => void): void;
}

export interface KeymapOptions {
  readonly keymap: readonly Binding[];
  readonly getMode: () => Mode;
  readonly dispatch: (action: KeyAction, e: KeyboardEvent) => void;
  /**
   * Whether a page command or a scope has an owner on screen (kit/keyCommands.ts ownsKeyAction).
   * An unowned binding does not resolve, so its key is never prevented. Default: everything owned.
   */
  readonly owns?: (action: KeyAction) => boolean;
}

const UNTYPED_INPUTS = new Set(["checkbox", "radio"]);

/**
 * The one typing rule: a text field, a select, contenteditable or a video (its own keys seek and
 * play) takes the key; a checkbox or a radio does not, so page keys still work beside one.
 */
export function isTypingTarget(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  if (t.isContentEditable || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.tagName === "VIDEO") return true;
  return t instanceof HTMLInputElement && !UNTYPED_INPUTS.has(t.type);
}

const ALL_OWNED = () => true;

export function installKeymap(target: KeyTarget, opts: KeymapOptions): () => void {
  const owns = opts.owns ?? ALL_OWNED;
  const onKey = (e: KeyboardEvent) => {
    // A widget that handled the key itself (a listbox's arrows, say) calls preventDefault.
    if (e.defaultPrevented || isModifierOnly(e)) return;
    const binding = resolveIn(opts.keymap, opts.getMode(), chordOf(e), isTypingTarget(e.target), scopeChain(e.target), owns);
    if (!binding) return;
    e.preventDefault();
    opts.dispatch(binding.action, e);
  };
  target.addEventListener("keydown", onKey);
  return () => target.removeEventListener("keydown", onKey);
}
