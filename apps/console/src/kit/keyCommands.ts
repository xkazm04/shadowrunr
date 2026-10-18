// Page commands a key can trigger. The shell's one keymap listener (src/keys) resolves a chord
// to a command and emits it here; the page on screen that owns the command subscribes. Pages
// never listen to the keyboard themselves, and the keymap stays the one place chords live.
import { useEffect, useRef } from "react";

/** `rerun`: run the page's check again (Brain's pre-flight board). */
export type KeyCommand = "rerun";

const handlers = new Map<KeyCommand, Set<() => void>>();

/** Runs every handler subscribed to `command`; false when no page on screen owns it. */
export function emitKeyCommand(command: KeyCommand): boolean {
  const set = handlers.get(command);
  if (!set || set.size === 0) return false;
  for (const h of [...set]) h();
  return true;
}

/** Subscribes `handler` to `command` while the component is mounted. */
export function useKeyCommand(command: KeyCommand, handler: () => void): void {
  const latest = useRef(handler);
  useEffect(() => {
    latest.current = handler;
  });
  useEffect(() => {
    const h = () => latest.current();
    const set = handlers.get(command) ?? new Set<() => void>();
    set.add(h);
    handlers.set(command, set);
    return () => {
      set.delete(h);
    };
  }, [command]);
}
