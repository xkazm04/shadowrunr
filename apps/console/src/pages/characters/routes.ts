// The Characters page's nested routes (the winner's l2.js P3.L2.characters), one reader:
//   characters/all
//   characters/<id>[/package[/<tab>]]            tab defaults to the constitution
//   characters/<id>/package/situations/<situation>
import { pageRoute, type Route } from "../../router";

export const PKG_TABS = [
  { id: "constitution", label: "Constitution" },
  { id: "rules", label: "Optional rules" },
  { id: "bible", label: "Bible" },
  { id: "tones", label: "Tones" },
  { id: "foils", label: "Foils" },
  { id: "situations", label: "Situations" },
  { id: "parameters", label: "Parameters" },
] as const;

export type PkgTab = (typeof PKG_TABS)[number]["id"];

export const ALL = "all";
export const PACKAGE = "package";

export const isPkgTab = (s: string | undefined): s is PkgTab => PKG_TABS.some((t) => t.id === s);

export type CharRoute =
  | { readonly kind: "all" }
  | { readonly kind: "package"; readonly id: string; readonly tab: PkgTab; readonly situation?: string }
  | { readonly kind: "invalid" };

export function readSub(sub: readonly string[]): CharRoute {
  const [id, pkg, tab, extra, ...rest] = sub;
  if (id === undefined) return { kind: "invalid" };
  if (id === ALL) return sub.length === 1 ? { kind: "all" } : { kind: "invalid" };
  if (pkg === undefined) return { kind: "package", id, tab: "constitution" };
  if (pkg !== PACKAGE) return { kind: "invalid" };
  if (tab === undefined) return { kind: "package", id, tab: "constitution" };
  if (!isPkgTab(tab) || rest.length > 0) return { kind: "invalid" };
  if (extra === undefined) return { kind: "package", id, tab };
  return tab === "situations" ? { kind: "package", id, tab, situation: extra } : { kind: "invalid" };
}

export const pkgRoute = (id: string, tab: PkgTab, ...more: string[]): Route => pageRoute("characters", id, PACKAGE, tab, ...more);
export const allRoute = (): Route => pageRoute("characters", ALL);

/** Breadcrumb labels below the page id; character ids fall back to the humanised id. */
export function crumbLabel(segment: string, depth: number, parts: readonly string[]): string | undefined {
  if (depth === 1 && segment === ALL) return "All characters";
  if (depth === 2 && segment === PACKAGE) return "Package";
  if (depth === 3) return PKG_TABS.find((t) => t.id === segment)?.label;
  if (depth === 4 && parts[3] === "situations") return segment;
  return undefined;
}
