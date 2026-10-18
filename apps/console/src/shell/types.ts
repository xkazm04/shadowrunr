import type { ComponentType } from "react";
import type { LabelOf, PageId } from "../router";

/**
 * What a page folder (`src/pages/<id>/`) plugs into the shell. The page builders own these
 * components; the shell owns where and when they render.
 */
export interface PageModule {
  /** The graphic status view inside the active poster. `active` is false while hidden. */
  readonly Layer1: ComponentType<{ readonly active: boolean }>;
  /** The nested page; `sub` is the route below the page id (never empty here). */
  readonly Layer2: ComponentType<{ readonly sub: readonly string[] }>;
  /** The mixer bus mini-render; a numbered slate when absent. */
  readonly Mini?: ComponentType;
  /** Poster-face art behind the title; plain halftone when absent. */
  readonly Motif?: ComponentType;
  /** Glance chips on the hovered wall poster. */
  readonly Glance?: ComponentType;
  /** Breadcrumb labels for this page's sub-segments; ids are humanised otherwise. */
  readonly label?: LabelOf;
}

export type PageRegistry = Readonly<Record<PageId, PageModule>>;

/** The poster copy (the winner's PAGES): name, short rail name, tag line, title lines. */
export interface PosterCopy {
  readonly name: string;
  readonly short: string;
  readonly tag: string;
  readonly lines: readonly string[];
}
