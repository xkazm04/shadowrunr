// The table view survives a drill-down round trip: a page that navigates away on row activation
// is remounted by the shell on the way back, and FilterTable restores query, facet, sort and the
// row you opened from an in-memory store keyed by `viewKey` (never browser storage).
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { FilterTable, type FilterTableProps } from "./FilterTable";

interface Run {
  readonly id: string;
  readonly tone: string;
  readonly viewers: number;
  readonly judged: boolean;
}
type V = "all" | "judged" | "unjudged";

const RUNS: readonly Run[] = [
  { id: "run-1", tone: "weary", viewers: 40, judged: true },
  { id: "run-2", tone: "weary", viewers: 900, judged: true },
  { id: "run-3", tone: "theatrical", viewers: 12, judged: true },
  { id: "run-4", tone: "weary", viewers: 300, judged: false },
  { id: "run-5", tone: "theatrical", viewers: 77, judged: false },
];

const COLUMNS = [
  { id: "id", header: "Run", cell: (r: Run) => r.id, sortValue: (r: Run) => r.id },
  { id: "viewers", header: "Viewers", cell: (r: Run) => r.viewers, sortValue: (r: Run) => r.viewers, align: "end" as const },
];

const FACETS = {
  label: "Verdict",
  initial: "all" as V,
  options: [
    { id: "all" as const, label: "All" },
    { id: "judged" as const, label: "Judged" },
    { id: "unjudged" as const, label: "Not judged" },
  ],
  predicate: (r: Run, f: V) => f === "all" || (f === "judged") === r.judged,
};

function runs(over: Partial<FilterTableProps<Run, V>> = {}) {
  return (
    <FilterTable<Run, V>
      rows={RUNS}
      label="Runs"
      getKey={(r) => r.id}
      haystack={(r) => `${r.id} ${r.tone}`}
      columns={COLUMNS}
      facets={FACETS}
      onActivate={() => {}}
      {...over}
    />
  );
}

const input = () => screen.getByLabelText("Filter Runs") as HTMLInputElement;
const pill = (name: string) => screen.getByRole("button", { name: new RegExp(`^${name}`) });
const th = (name: string) => screen.getByRole("columnheader", { name });
const countLine = () => screen.getByText(/ of \d/).textContent;

// Memory only: nothing in this file may reach browser storage.
const local = vi.spyOn(Storage.prototype, "setItem");

afterEach(cleanup);
afterAll(() => {
  expect(local).toHaveBeenCalledTimes(0);
  local.mockRestore();
});

describe("FilterTable remembers its view per viewKey", () => {
  it("restores query, facet and sort on remount, before the rows paint", () => {
    const first = render(runs({ viewKey: "test.simulator.runs" }));
    fireEvent.change(input(), { target: { value: "weary" } });
    fireEvent.click(pill("Judged"));
    fireEvent.click(screen.getByRole("button", { name: "Viewers" }));
    fireEvent.click(screen.getByRole("button", { name: "Viewers" }));
    expect(th("Viewers").getAttribute("aria-sort")).toBe("descending");
    first.unmount();

    render(runs({ viewKey: "test.simulator.runs" }));
    expect(input().value).toBe("weary");
    expect(pill("Judged").getAttribute("aria-pressed")).toBe("true");
    expect(th("Viewers").getAttribute("aria-sort")).toBe("descending");
    expect(countLine()).toBe("2 of 5");
    const ids = [...document.querySelectorAll("tbody tr[data-index]")].map((tr) => tr.firstElementChild?.textContent);
    expect(ids).toEqual(["run-2", "run-1"]);
  });

  it("keeps two tables with the same label apart by viewKey", () => {
    const brain = render(runs({ viewKey: "test.brain.runs" }));
    fireEvent.change(input(), { target: { value: "theatrical" } });
    brain.unmount();

    const sim = render(runs({ viewKey: "test.sim.runs.apart" }));
    expect(input().value).toBe("");
    expect(countLine()).toBe("5 of 5");
    fireEvent.click(pill("Not judged"));
    sim.unmount();

    render(runs({ viewKey: "test.brain.runs" }));
    expect(input().value).toBe("theatrical");
    expect(pill("All").getAttribute("aria-pressed")).toBe("true");
  });

  it("falls back when what it remembered is gone: column, facet option, row", () => {
    const scrolls: number[] = [];
    Object.defineProperty(HTMLElement.prototype, "scrollTo", {
      configurable: true,
      value: (o?: ScrollToOptions | number) => {
        if (typeof o === "object" && o.top !== undefined) scrolls.push(o.top);
      },
    });
    const first = render(runs({ viewKey: "test.vanish" }));
    fireEvent.click(screen.getByRole("button", { name: "Viewers" }));
    fireEvent.click(pill("Not judged"));
    fireEvent.keyDown(screen.getByText("run-4").closest("tr") as HTMLElement, { key: "Enter" });
    first.unmount();

    // Viewers column dropped, "unjudged" no longer offered, run-4 no longer in the rows.
    const remount = () =>
      render(
        runs({
          viewKey: "test.vanish",
          rows: RUNS.filter((r) => r.id !== "run-4"),
          columns: [COLUMNS[0] as (typeof COLUMNS)[number]],
          initialSort: { column: "id", dir: "desc" },
          facets: { ...FACETS, options: FACETS.options.filter((o) => o.id !== "unjudged") },
        }),
      );
    expect(remount).not.toThrow();
    expect(th("Run").getAttribute("aria-sort")).toBe("descending");
    expect(pill("All").getAttribute("aria-pressed")).toBe("true");
    expect(countLine()).toBe("4 of 4");
    expect(document.querySelector("[data-returned]")).toBeNull();
    // The virtualiser's own mount scroll is to 0; nothing scrolls toward a vanished row.
    expect(scrolls.filter((t) => t > 0)).toEqual([]);
    Reflect.deleteProperty(HTMLElement.prototype, "scrollTo");
  });

  it("'Clear filter' forgets the remembered query and facet, and keeps the sort", () => {
    const first = render(runs({ viewKey: "test.clear" }));
    fireEvent.click(screen.getByRole("button", { name: "Viewers" }));
    fireEvent.click(pill("Not judged"));
    fireEvent.change(input(), { target: { value: "nobody" } });
    fireEvent.click(screen.getByRole("button", { name: "Clear filter" }));
    first.unmount();

    render(runs({ viewKey: "test.clear" }));
    expect(input().value).toBe("");
    expect(pill("All").getAttribute("aria-pressed")).toBe("true");
    expect(countLine()).toBe("5 of 5");
    expect(th("Viewers").getAttribute("aria-sort")).toBe("ascending");
  });

  it("without a viewKey nothing is remembered (guard: today's behaviour)", () => {
    const first = render(runs());
    fireEvent.change(input(), { target: { value: "weary" } });
    first.unmount();
    render(runs());
    expect(input().value).toBe("");
  });
});

describe("FilterTable lands on the row you came back from", () => {
  beforeAll(() => {
    // jsdom has no layout: give the viewport a height and a scrollTo that moves it, so the
    // virtualiser renders the window it scrolled to.
    Object.defineProperty(HTMLElement.prototype, "scrollHeight", { configurable: true, get: () => 10_000_000 });
  });
  afterAll(() => {
    Reflect.deleteProperty(HTMLElement.prototype, "scrollHeight");
    Reflect.deleteProperty(HTMLElement.prototype, "scrollTo");
  });

  it("scrolls a 48,000-row table back to the opened row and focuses it", () => {
    const scrolls: number[] = [];
    Object.defineProperty(HTMLElement.prototype, "scrollTo", {
      configurable: true,
      value(this: HTMLElement, o?: ScrollToOptions | number) {
        if (typeof o !== "object" || o.top === undefined) return;
        scrolls.push(o.top);
        this.scrollTop = o.top;
        this.dispatchEvent(new Event("scroll"));
      },
    });
    const data = Array.from({ length: 48_000 }, (_, i) => ({ id: `run-${i}`, tone: i % 3 === 0 ? "weary" : "theatrical" }));
    const opened: string[] = [];
    const table = () => (
      <FilterTable
        rows={data}
        label="Runs"
        viewKey="test.big"
        getKey={(r) => r.id}
        haystack={(r) => `${r.id} ${r.tone}`}
        columns={[
          { id: "id", header: "Run", cell: (r) => r.id },
          { id: "tone", header: "Tone", cell: (r) => r.tone },
        ]}
        onActivate={(r) => opened.push(r.id)}
      />
    );
    const first = render(table());
    // Scroll the operator down to row 4,799 and open it.
    const viewport = document.querySelector("table")?.parentElement as HTMLElement;
    act(() => {
      viewport.scrollTop = 4799 * 38.4 - 200;
      viewport.dispatchEvent(new Event("scroll"));
    });
    const row = screen.getByText("run-4799").closest("tr") as HTMLElement;
    fireEvent.keyDown(row, { key: "Enter" });
    expect(opened).toEqual(["run-4799"]);
    first.unmount();
    scrolls.length = 0;

    render(table());
    expect(scrolls.length).toBeGreaterThan(0);
    expect(scrolls[scrolls.length - 1] ?? 0).toBeGreaterThan(0);
    const back = screen.getByText("run-4799").closest("tr") as HTMLElement;
    expect(back.getAttribute("data-returned")).toBe("true");
    expect(document.activeElement).toBe(back);
    expect(document.querySelectorAll("[data-returned]").length).toBe(1);
  });
});
