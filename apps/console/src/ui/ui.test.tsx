import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { filterRows, nextSort } from "./filterRows";
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
