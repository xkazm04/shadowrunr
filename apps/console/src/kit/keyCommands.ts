// Page commands a key can trigger. The shell's one keymap listener (src/keys) resolves a chord
// to a command, or to a command of a focus scope, and emits it here; the page on screen that owns
// it subscribes. Pages never listen to the keyboard themselves, and the keymap stays the one
// place chords live: a page with keys of its own declares scoped bindings there, marks its
// element `data-key-scope`, and subscribes with `useKeyScope`.
//
// kit may not import keys (eslint.config.js boundaries), so the vocabulary below mirrors
// keys/scopes.ts; shell/keyActions.ts ties the two at compile time, so neither can drift.
import { useEffect, useRef } from "react";

/** `rerun`: run the page's check again (Brain's pre-flight board). */
export type KeyCommand = "rerun";

/** Each focus scope's commands and the argument each one takes (`undefined`: none). */
export interface ScopeCommands {
  readonly "review.player": { readonly toggle: number; readonly save: undefined; readonly skip: undefined; readonly back: undefined };
  readonly "characters.roster": { readonly step: 1 | -1 };
}

export type KeyScope = keyof ScopeCommands;

type ArgOf<A> = undefined extends A ? { readonly arg?: undefined } : { readonly arg: A };

/** One scope command, as the keymap emits it. */
export type ScopedAction = {
  [S in KeyScope]: {
    [C in keyof ScopeCommands[S]]: { readonly type: "scoped"; readonly scope: S; readonly command: C } & ArgOf<ScopeCommands[S][C]>;
  }[keyof ScopeCommands[S]];
}[KeyScope];

/** What an owner hands `useKeyScope`: one handler per command of its scope. */
export type ScopeHandlers<S extends KeyScope> = {
  readonly [C in keyof ScopeCommands[S]]: (arg: ScopeCommands[S][C]) => void;
};

/** The part of a keymap action the owners read (keys/keymap.ts KeyAction fits it). */
export interface OwnableAction {
  readonly type: string;
  readonly command?: string;
  readonly scope?: string;
  readonly arg?: unknown;
}

const handlers = new Map<KeyCommand, Set<() => void>>();
/** Each scope's mounted owners, newest last: the newest one runs the scope's keys. */
const owners = new Map<KeyScope, Array<{ readonly current: unknown }>>();

/** Runs every handler subscribed to `command`; false when no page on screen owns it. */
export function emitKeyCommand(command: KeyCommand): boolean {
  const set = handlers.get(command);
  if (!set || set.size === 0) return false;
  for (const h of [...set]) h();
  return true;
}

/** Runs a scope command on the scope's newest owner; false when none is mounted. */
export function emitScoped(action: ScopedAction): boolean {
  const list = owners.get(action.scope);
  const owner = list?.[list.length - 1];
  if (!owner) return false;
  const run = (owner.current as Readonly<Record<string, ((arg: unknown) => void) | undefined>>)[action.command];
  run?.(action.arg);
  return true;
}

/** Whether a keymap action has an owner on screen (the listener's `owns`): only commands and scopes can lack one. */
export function ownsKeyAction(action: OwnableAction): boolean {
  if (action.type === "command") return (handlers.get(action.command as KeyCommand)?.size ?? 0) > 0;
  if (action.type === "scoped") return (owners.get(action.scope as KeyScope)?.length ?? 0) > 0;
  return true;
}

/** Runs an owned action (a command or a scope command); false for every other action, which the shell runs. */
export function dispatchOwned(action: OwnableAction): boolean {
  if (action.type === "command") return emitKeyCommand(action.command as KeyCommand);
  if (action.type === "scoped") return emitScoped(action as ScopedAction);
  return false;
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

/**
 * Owns `scope` while the component is mounted: its keys (declared in KEYMAP with this scope) run
 * these handlers when focus is inside the component's `data-key-scope={scope}` element.
 */
export function useKeyScope<S extends KeyScope>(scope: S, scopeHandlers: ScopeHandlers<S>): void {
  const latest = useRef<unknown>(scopeHandlers);
  useEffect(() => {
    latest.current = scopeHandlers;
  });
  useEffect(() => {
    const owner = latest;
    const list = owners.get(scope) ?? [];
    list.push(owner);
    owners.set(scope, list);
    return () => {
      const i = list.indexOf(owner);
      if (i >= 0) list.splice(i, 1);
    };
  }, [scope]);
}
