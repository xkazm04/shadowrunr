// @shadowrunr/tokens: design tokens generated from tokens.source.json.
// Styles: import "@shadowrunr/tokens/tokens.css" once at the app root; components read the
// --sr-* custom properties from CSS Modules. Scripts and canvases read the resolved TOKENS.

import { TOKENS, VAR_PREFIX } from "./tokens";

export * from "./tokens";

export type TokenGroup = keyof typeof TOKENS;
export type TokenName<G extends TokenGroup> = keyof (typeof TOKENS)[G] & string;

/** The CSS custom property that carries a role, e.g. cssVar("color", "surfacePaper"). */
export function cssVar<G extends TokenGroup>(group: G, name: TokenName<G>): string {
  return `${VAR_PREFIX[group]}${name.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}`;
}

/** A var() reference to a role, for inline styles. */
export function cssRef<G extends TokenGroup>(group: G, name: TokenName<G>): string {
  return `var(${cssVar(group, name)})`;
}
