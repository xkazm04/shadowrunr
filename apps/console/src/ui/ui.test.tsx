import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { filterRows, nextSort } from "./filterRows";
import { FilterTable } from "./FilterTable";
import { RateBar } from "./RateBar";
import { State } from "./Chip";
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

describe("primitives", () => {
  it("maps states to tones through one table", () => {
    render(<State value="not_provoked" />);
    expect(screen.getByText("not provoked")).toBeTruthy();
  });

  it("shows a rate with n and its bound", () => {
    render(<RateBar label="Held" rate={0.62} n={40} bound={{ lo: 0.48, hi: 0.74 }} />);
    expect(screen.getByRole("img").getAttribute("aria-label")).toBe("Held: 62% (48%–74%), n=40");
  });

  it("toggles a switch", () => {
    let on = false;
    render(<Switch label="Rule of three" checked={on} onChange={(v) => (on = v)} />);
    fireEvent.click(screen.getByRole("switch"));
    expect(on).toBe(true);
  });
});
