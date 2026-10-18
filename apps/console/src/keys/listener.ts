// The only keydown listener in the console. Everything else declares bindings in `KEYMAP`.
import { chordOf, isModifierOnly, resolve, type Binding, type KeyAction, type Mode } from "./keymap";

export interface KeyTarget {
  addEventListener(type: "keydown", listener: (e: KeyboardEvent) => void): void;
  removeEventListener(type: "keydown", listener: (e: KeyboardEvent) => void): void;
}

export interface KeymapOptions {
  readonly keymap: readonly Binding[];
  readonly getMode: () => Mode;
  readonly dispatch: (action: KeyAction, e: KeyboardEvent) => void;
}

export function isTypingTarget(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  return t.isContentEditable || t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT";
}

export function installKeymap(target: KeyTarget, opts: KeymapOptions): () => void {
  const onKey = (e: KeyboardEvent) => {
    // A widget that handled the key itself (a listbox's arrows, say) calls preventDefault.
    if (e.defaultPrevented || isModifierOnly(e)) return;
    const binding = resolve(opts.keymap, opts.getMode(), chordOf(e), isTypingTarget(e.target));
    if (!binding) return;
    e.preventDefault();
    opts.dispatch(binding.action, e);
  };
  target.addEventListener("keydown", onKey);
  return () => target.removeEventListener("keydown", onKey);
}
