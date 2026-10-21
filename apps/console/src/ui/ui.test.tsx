import { readFileSync } from "node:fs";
import { join } from "node:path";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { facetCounts, filterRows, nextSort } from "./filterRows";
import { FilterTable } from "./FilterTable";
import { RateBar } from "./RateBar";
import { State } from "./Chip";
import { Markdown } from "./Markdown";
import { parseBlocks } from "./doc";
import { Switch } from "./Switch";
import { VirtualList } from "./VirtualList";

afterEach(cleanup);

describe("filterRows", () => {
  const rows = [
    { id: "r1", text: "Weary middle manager", n: 3 },
    { id: "r2", text: "Theatrical villain", n: 10 },
    { id: "r3", text: "Weary theatrical", n: 1 },
  ];
  it("matches every term, case-insensitively", () => {
    expect(filterRows(rows, { haystack: (r) => r.text, query: "WEARY theat" }).map((r) => r.id)).toEqual(["r3"]);
  });
  it("sorts numerically and flips direction", () => {
    expect(filterRows(rows, { haystack: (r) => r.text, query: "", sortValue: (r) => r.n, dir: "desc" }).map((r) => r.id)).toEqual(["r2", "r1", "r3"]);
    expect(nextSort(null, "n")).toEqual({ column: "n", dir: "asc" });
    expect(nextSort({ column: "n", dir: "asc" }, "n")).toEqual({ column: "n", dir: "desc" });
  });
});

describe("virtualised lists at 10x scale", () => {
  it("renders a window of a 100,000-row list, not the list", () => {
    render(<VirtualList count={100_000} label="Transcript" renderRow={(i) => <span>line {i}</span>} />);
    const rows = screen.getAllByRole("listitem");
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(60);
    expect(screen.getByText("line 0")).toBeTruthy();
  });

  it("renders a window of a 48,000-row table and filters it", () => {
    const data = Array.from({ length: 48_000 }, (_, i) => ({ id: `run-${i}`, tone: i % 3 === 0 ? "weary" : "theatrical" }));
    render(
      <FilterTable
        rows={data}
        label="Runs"
        getKey={(r) => r.id}
        haystack={(r) => `${r.id} ${r.tone}`}
        columns={[
          { id: "id", header: "Run", cell: (r) => r.id, sortValue: (r) => r.id },
          { id: "tone", header: "Tone", cell: (r) => r.tone },
        ]}
      />,
    );
    expect(document.querySelectorAll("tbody tr").length).toBeLessThan(60);
    expect(screen.getByText("48,000 of 48,000")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Filter Runs"), { target: { value: "run-4799" } });
    expect(screen.getByText(/of 48,000/).textContent).toBe("11 of 48,000");
  });
});

describe("FilterTable empty filter", () => {
  it("shows a no-matches row when the filter empties the table, and clears it", () => {
    const data = [
      { id: "a", tone: "weary", on: true },
      { id: "b", tone: "theatrical", on: false },
    ];
    render(
      <FilterTable<(typeof data)[number], "on" | "all">
        rows={data}
        label="Runs"
        getKey={(r) => r.id}
        haystack={(r) => `${r.id} ${r.tone}`}
        columns={[{ id: "id", header: "Run", cell: (r) => r.id }]}
        facets={{
          label: "Show",
          initial: "all",
          options: [
            { id: "all", label: "All" },
            { id: "on", label: "On" },
          ],
          predicate: (r, f) => f === "all" || r.on,
        }}
      />,
    );
    const empty = () => document.querySelector("[data-role='filter-table-empty']");
    expect(empty()).toBeNull();
    fireEvent.change(screen.getByLabelText("Filter Runs"), { target: { value: "nobody" } });
    expect(empty()?.textContent).toContain("No matches");
    expect(empty()?.textContent).toContain("“nobody”");
    expect(screen.getByText("0 of 2")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Clear filter" }));
    expect(empty()).toBeNull();
    expect(screen.getByText("2 of 2")).toBeTruthy();
  });

  it("shows nothing extra for a table with no rows at all", () => {
    render(<FilterTable rows={[] as { id: string }[]} label="Runs" getKey={(r) => r.id} haystack={(r) => r.id} columns={[{ id: "id", header: "Run", cell: (r) => r.id }]} />);
    expect(document.querySelector("[data-role='filter-table-empty']")).toBeNull();
  });
});

describe("VirtualList scrollToIndex", () => {
  it("scrolls the viewport to the row it is given, and again when it changes", () => {
    const calls: number[] = [];
    // jsdom has no layout and no Element.scrollTo: give the viewport a scroll height and record
    // what the virtualiser asks for.
    Object.defineProperty(HTMLElement.prototype, "scrollTo", {
      configurable: true,
      value: (o?: ScrollToOptions | number) => {
        if (typeof o === "object" && o.top !== undefined) calls.push(o.top);
      },
    });
    Object.defineProperty(HTMLElement.prototype, "scrollHeight", { configurable: true, get: () => 10_000_000 });
    const row = (i: number) => <span>row {i}</span>;
    const view = render(<VirtualList label="Lines" count={100_000} estimateRem={2} renderRow={row} scrollToIndex={50_000} />);
    expect(calls.length).toBeGreaterThan(0);
    const first = calls[calls.length - 1] ?? 0;
    expect(first).toBeGreaterThan(0);
    view.rerender(<VirtualList label="Lines" count={100_000} estimateRem={2} renderRow={row} scrollToIndex={90_000} />);
    expect(calls[calls.length - 1] ?? 0).toBeGreaterThan(first);
    Reflect.deleteProperty(HTMLElement.prototype, "scrollTo");
    Reflect.deleteProperty(HTMLElement.prototype, "scrollHeight");
  });
});

describe("primitives", () => {
  it("maps states to tones through one table", () => {
    render(<State value="not_provoked" />);
    expect(screen.getByText("not provoked")).toBeTruthy();
  });

  it("shows a rate with n and its bound", () => {
    render(<RateBar label="Held" rate={0.62} n={40} bound={{ lo: 0.48, hi: 0.74 }} />);
    expect(screen.getByRole("img").getAttribute("aria-label")).toBe("Held: 62% (48%–74%), n=40");
  });

  it("keeps a switch's label for screen readers only when asked", () => {
    render(<Switch label="Crisis handoff" hideLabel checked onChange={() => undefined} />);
    const sw = screen.getByRole("switch", { name: /Crisis handoff/ });
    const label = [...sw.querySelectorAll("span")].find((s) => s.textContent === "Crisis handoff");
    expect(label?.className).toBeTruthy();
  });

  it("renders markdown as elements, never as HTML", () => {
    const src = ["# Title", "", "A **bold** <script>alert(1)</script> `code`", "", "- one", "  - nested"].join("\n");
    const { container } = render(<Markdown blocks={parseBlocks(src)} />);
    expect(container.querySelector("script")).toBeNull();
    expect(container.textContent).toContain("<script>alert(1)</script>");
    expect(container.querySelector("strong")?.textContent).toBe("bold");
    expect(container.querySelector("code")?.textContent).toBe("code");
    expect(container.querySelectorAll("ul ul li")).toHaveLength(1);
  });

  it("toggles a switch", () => {
    let on = false;
    render(<Switch label="Rule of three" checked={on} onChange={(v) => (on = v)} />);
    fireEvent.click(screen.getByRole("switch"));
    expect(on).toBe(true);
  });
});

// Facet counts carry the applied predicate: FilterTable derives each pill's count from the facet
// predicate over the text-filtered rows, so the pills and the "N of M" line answer one question.
describe("facet counts", () => {
  type Run = { id: string; tone: string; judged: boolean };
  type V = "all" | "judged" | "unjudged";
  const runs: Run[] = [
    { id: "run-1", tone: "weary", judged: true },
    { id: "run-2", tone: "theatrical", judged: true },
    { id: "run-3", tone: "theatrical", judged: true },
    { id: "run-4", tone: "weary", judged: false },
    { id: "run-5", tone: "theatrical", judged: false },
  ];
  const options = [
    { id: "all", label: "All" },
    { id: "judged", label: "Judged" },
    { id: "unjudged", label: "Not judged" },
  ] as const;
  const predicate = (r: Run, f: V) => f === "all" || (f === "judged") === r.judged;
  const pills = () =>
    [...document.querySelectorAll("[role='group'] button")].map((b) => `${b.firstChild?.textContent ?? ""} ${b.querySelector("span")?.textContent ?? "-"}`);

  it("counts every option in one pass over the rows", () => {
    let hay = 0;
    let pred = 0;
    const spec = {
      haystack: (r: Run) => {
        hay += 1;
        return `${r.id} ${r.tone}`;
      },
      predicate: (r: Run, f: V) => {
        pred += 1;
        return predicate(r, f);
      },
      options,
    };
    expect(facetCounts(runs, { ...spec, query: "" })).toEqual({ all: 5, judged: 3, unjudged: 2 });
    expect(pred).toBe(15);
    expect(hay).toBe(0);
    expect(facetCounts(runs, { ...spec, query: "WEARY" })).toEqual({ all: 2, judged: 1, unjudged: 1 });
    expect(hay).toBe(5);
  });

  const table = (counts?: "given") => (
    <FilterTable<Run, V>
      rows={runs}
      label="Runs"
      getKey={(r) => r.id}
      haystack={(r) => `${r.id} ${r.tone}`}
      columns={[{ id: "id", header: "Run", cell: (r) => r.id }]}
      facets={{
        label: "Verdict",
        initial: "all",
        options: counts === "given" ? [{ id: "all", label: "All", count: 412 }, { id: "judged", label: "Judged", count: 400 }, { id: "unjudged", label: "Not judged", count: 12 }] : options,
        predicate,
        ...(counts === undefined ? {} : { counts }),
      }}
    />
  );

  it("supplies the counts when the options carry none", () => {
    render(table());
    expect(pills()).toEqual(["All 5", "Judged 3", "Not judged 2"]);
  });

  it("counts under the typed query, agreeing with the count line", () => {
    render(table());
    fireEvent.change(screen.getByLabelText("Filter Runs"), { target: { value: "run-1" } });
    expect(pills()).toEqual(["All 1", "Judged 1", "Not judged 0"]);
    expect(screen.getByText(/of 5/).textContent).toBe("1 of 5");
  });

  it("prints given counts verbatim whatever the query (guard: the daemon's totals)", () => {
    render(table("given"));
    expect(pills()).toEqual(["All 412", "Judged 400", "Not judged 12"]);
    fireEvent.change(screen.getByLabelText("Filter Runs"), { target: { value: "run-1" } });
    expect(pills()).toEqual(["All 412", "Judged 400", "Not judged 12"]);
  });

  it("counts a 48,000-row table off the deferred query, in the same pass as the rows", () => {
    type T = "all" | "a" | "b" | "c";
    const data = Array.from({ length: 48_000 }, (_, i) => ({ id: `run-${i}`, tier: (["a", "b", "c"] as const)[i % 3] ?? "a" }));
    let hay = 0;
    render(
      <FilterTable<(typeof data)[number], T>
        rows={data}
        label="Runs"
        getKey={(r) => r.id}
        haystack={(r) => {
          hay += 1;
          return r.id;
        }}
        columns={[{ id: "id", header: "Run", cell: (r) => r.id }]}
        facets={{
          label: "Tier",
          initial: "all",
          options: [
            { id: "all", label: "all" },
            { id: "a", label: "a" },
            { id: "b", label: "b" },
            { id: "c", label: "c" },
          ],
          predicate: (r, f) => f === "all" || r.tier === f,
        }}
      />,
    );
    expect(pills()).toEqual(["all 48,000", "a 16,000", "b 16,000", "c 16,000"]);
    hay = 0;
    fireEvent.change(screen.getByLabelText("Filter Runs"), { target: { value: "run-4799" } });
    // run-4799 and run-47990..47999: 11 rows; 4799 % 3 = 2 -> c, 47990..47999 -> c a b c a b c a b c
    expect(pills()).toEqual(["all 11", "a 3", "b 3", "c 5"]);
    expect(screen.getByText(/of 48,000/).textContent).toBe("11 of 48,000");
    // One haystack pass per query change feeds both the rows and the counts.
    expect(hay).toBeLessThanOrEqual(48_000);
  });

  it("leaves no hand-computed facet count in the migrated pages (Incidents keeps the daemon's)", () => {
    const pages = [
      "src/pages/brain/Engine.tsx",
      "src/pages/brain/Runs.tsx",
      "src/pages/characters/SituationsTab.tsx",
      "src/pages/simulator/Runs.tsx",
      "src/pages/reconcile/DirectivesPage.tsx",
      "src/pages/segments/Tables.tsx",
      "src/pages/onair/Rundown.tsx",
    ];
    for (const f of pages) {
      const src = readFileSync(join(process.cwd(), f), "utf8");
      expect(src, f).not.toMatch(/\bcount\b\s*[:,}]/);
    }
    expect(readFileSync(join(process.cwd(), "src/pages/reconcile/Incidents.tsx"), "utf8")).toMatch(/counts: "given"/);
  });
});
