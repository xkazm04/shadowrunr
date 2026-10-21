// What each keymap action does to the shell, as a pure function of the current state.
import type { KeyAction, KeyCommand, Mode } from "../keys";
import { PAGE_IDS, WALL, encodeRoute, layerOf, pageRoute, up, type PageId, type Route } from "../router";

export interface KeyContext {
  readonly route: Route;
  readonly intro: boolean;
  /** The page the wall returns to (M, Season). */
  readonly lastPage: PageId;
  readonly hover: number | null;
}

export type Effect =
  | { readonly kind: "navigate"; readonly route: Route }
  | { readonly kind: "hover"; readonly index: number }
  | { readonly kind: "skip-intro" }
  | { readonly kind: "blur" }
  | { readonly kind: "open-goto" }
  | { readonly kind: "command"; readonly command: KeyCommand }
  | { readonly kind: "none" };

const NONE: Effect = { kind: "none" };
const wrap = (i: number, n: number) => ((i % n) + n) % n;

export function modeOf(ctx: Pick<KeyContext, "route" | "intro">): Mode {
  return ctx.intro ? "intro" : layerOf(ctx.route);
}

export function effectOf(action: KeyAction, ctx: KeyContext): Effect {
  const n = PAGE_IDS.length;
  switch (action.type) {
    case "page": {
      const id = PAGE_IDS[action.index];
      return id ? { kind: "navigate", route: pageRoute(id) } : NONE;
    }
    case "up": {
      const parent = up(ctx.route);
      return parent ? { kind: "navigate", route: parent } : NONE;
    }
    case "toggle-wall":
      return { kind: "navigate", route: ctx.route.view === "wall" ? pageRoute(ctx.lastPage) : WALL };
    case "wall-focus": {
      const from = ctx.hover ?? PAGE_IDS.indexOf(ctx.lastPage);
      return { kind: "hover", index: wrap(from + action.delta, n) };
    }
    case "page-step": {
      if (ctx.route.view !== "page") return NONE;
      const id = PAGE_IDS[wrap(PAGE_IDS.indexOf(ctx.route.page) + action.delta, n)];
      return id ? { kind: "navigate", route: pageRoute(id) } : NONE;
    }
    case "skip-intro":
      return { kind: "skip-intro" };
    case "blur":
      return { kind: "blur" };
    case "goto":
      return { kind: "open-goto" };
    case "command":
      return { kind: "command", command: action.command };
  }
}


/**
 * The places visited this session, newest first: a route moves to the front when revisited, the
 * wall is not a place, and the list keeps 8. Held in shell state only, never persisted.
 */
export function nextRecents(recents: readonly Route[], route: Route): readonly Route[] {
  if (route.view === "wall") return recents;
  const key = encodeRoute(route);
  return [route, ...recents.filter((r) => encodeRoute(r) !== key)].slice(0, 8);
}
