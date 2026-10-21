// Focus scopes: a page that wants keys of its own declares them in `KEYMAP` with a `scope`, marks
// the owning element with `data-key-scope`, and subscribes with `useKeyScope` (kit/keyCommands.ts).
// The one listener resolves the innermost scope around the event's target first, then the global
// bindings, so a scoped key that shadows a global one is a declared fact the conflict report lists,
// never a private onKeyDown racing the window listener.
//
// The owner side (kit/keyCommands.ts) cannot import keys, nor keys kit (eslint.config.js
// boundaries), so kit mirrors this vocabulary and shell/keyActions.ts, which sees both, holds a
// compile-time tie: the two cannot drift.

/** A page command (kit/keyCommands.ts useKeyCommand): the page on screen that owns it runs it. */
export type KeyCommand = "rerun";

/** Each scope's commands and the argument each one takes (`undefined`: none). */
export interface ScopeCommands {
  /** The review player (pages/review/Player.tsx): marks, save, skip and back. */
  readonly "review.player": { readonly toggle: number; readonly save: undefined; readonly skip: undefined; readonly back: undefined };
  /** The cast poster's roster selector (pages/characters/Layer1.tsx). */
  readonly "characters.roster": { readonly step: 1 | -1 };
}

export type KeyScope = keyof ScopeCommands;

export const KEY_SCOPES: readonly KeyScope[] = ["review.player", "characters.roster"];

/** The attribute that marks a scope's owning element. */
export const SCOPE_ATTR = "data-key-scope";

type ArgOf<A> = undefined extends A ? { readonly arg?: undefined } : { readonly arg: A };

/** A keymap action that runs one command of one scope's owner. */
export type ScopedAction = {
  [S in KeyScope]: {
    [C in keyof ScopeCommands[S]]: { readonly type: "scoped"; readonly scope: S; readonly command: C } & ArgOf<ScopeCommands[S][C]>;
  }[keyof ScopeCommands[S]];
}[KeyScope];

/** What an owner hands `useKeyScope`: one handler per command of its scope. */
export type ScopeHandlers<S extends KeyScope> = {
  readonly [C in keyof ScopeCommands[S]]: (arg: ScopeCommands[S][C]) => void;
};

export function isKeyScope(s: string | null | undefined): s is KeyScope {
  return s != null && (KEY_SCOPES as readonly string[]).includes(s);
}

/** The scopes around `target`, innermost first; empty outside every scope. */
export function scopeChain(target: EventTarget | null): readonly KeyScope[] {
  const chain: KeyScope[] = [];
  let el: Element | null = typeof Element !== "undefined" && target instanceof Element ? target.closest(`[${SCOPE_ATTR}]`) : null;
  while (el) {
    const s = el.getAttribute(SCOPE_ATTR);
    if (isKeyScope(s) && !chain.includes(s)) chain.push(s);
    el = el.parentElement?.closest(`[${SCOPE_ATTR}]`) ?? null;
  }
  return chain;
}
