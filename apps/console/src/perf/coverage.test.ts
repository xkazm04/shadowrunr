// Gated: the perf gates cover exactly the registered page set, both ways. The measured set is
// PERF_PAGES (pages.tsx), keyed by PageId; the compile-time `satisfies Record<PageId, PerfPage>`
// refuses a page with no entry, and this runtime check refuses what the type cannot see: a
// measured name with no line in budgets/perf.json, a line no page measures, an empty exemption.
// The same two-way rule check-bundle.mjs applies to bundle routes.
import { describe, expect, it } from "vitest";
import { fail } from "../kit/testing";
import { PAGE_IDS, type PageId } from "../router/route";
import { BUDGETS } from "./harness";
import { PERF_PAGES, perfCoverage, type PerfPage, type Probe } from "./pages";

/** `o` without key `k` (a copy). */
function without<T extends object, K extends keyof T & string>(o: T, k: K): Omit<T, K> {
  return Object.fromEntries(Object.entries(o).filter(([key]) => key !== k)) as Omit<T, K>;
}

const probe: Probe = { what: "a probe", ui: () => null, script: () => () => fail("method_not_found", "unused"), hash: "#/" };

describe("perf coverage", () => {
  it("PERF_PAGES holds exactly the registered pages", () => {
    expect(new Set(Object.keys(PERF_PAGES))).toEqual(new Set(PAGE_IDS));
  });

  it("every measured name has its line and every line is measured (budgets/perf.json)", () => {
    expect(perfCoverage(PERF_PAGES, BUDGETS)).toEqual([]);
  });

  it("review is measured: a layer-1 line and the timeline at 100k lines", () => {
    expect(BUDGETS.dom["review.l1"]).toBeDefined();
    expect(BUDGETS.dom["review.l2.timeline"]).toBeDefined();
    expect(BUDGETS.rpc["review.l1"]).toBeDefined();
  });

  it("a budget line no page measures fails", () => {
    const budgets = { ...BUDGETS, dom: { ...BUDGETS.dom, "ghost.l1": { max: 10, why: "seeded" } } };
    expect(perfCoverage(PERF_PAGES, budgets)).toContain("ghost.l1: budgeted but no page measures it");
  });

  it("a measured name with no line fails", () => {
    expect(perfCoverage(PERF_PAGES, { ...BUDGETS, dom: without(BUDGETS.dom, "review.l1") })).toContain("review.l1: measured but no line in budgets/perf.json");
    expect(perfCoverage(PERF_PAGES, { ...BUDGETS, rpc: without(BUDGETS.rpc, "review.l1") })).toContain("review.l1: measured but no rpc line in budgets/perf.json");
  });

  it("a registry key that is not a page, or a page with no entry, fails", () => {
    expect(perfCoverage(without(PERF_PAGES, "review"), BUDGETS)).toContain("review: a registered page with no PERF_PAGES entry");
    expect(perfCoverage({ ...PERF_PAGES, ghost: { l1: probe } }, BUDGETS)).toContain("ghost: not a registered page");
  });

  it("an exemption must say why", () => {
    const pages: Record<string, PerfPage> = { ...PERF_PAGES, review: { exempt: " " } };
    const problems = perfCoverage(pages, BUDGETS);
    expect(problems).toContain("review: an exemption must say why");
    // An exempt page measures nothing, so its lines are orphans until they go too.
    expect(problems).toContain("review.l1: budgeted but no page measures it");
  });

  it("the registry type is exhaustive over PageId (typecheck refuses a missing page)", () => {
    const six = without(PERF_PAGES, "review");
    // @ts-expect-error -- 'review' is missing: a page without a perf entry does not compile.
    const partial: Record<PageId, PerfPage> = six;
    expect(Object.keys(partial)).not.toContain("review");
  });
});
