import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { TournamentResults } from "../contract";
import simFixture from "../fixtures/sim.json";
import { cellId, ruleFails, worstFirst } from "../lib/heatmap";
import { Heatmap } from "./Heatmap";

const results = (simFixture as unknown as { results: Record<string, TournamentResults> }).results["tones-2026-10-17"]!;

afterEach(cleanup);

describe("<Heatmap>", () => {
  it("renders rows worst first, matching worstFirst()", () => {
    render(<Heatmap results={results} selection={null} onSelect={() => {}} />);
    const rendered = screen.getAllByTestId("hm-row").map((r) => r.getAttribute("data-cell"));
    expect(rendered).toEqual(worstFirst(results.cells).map((c) => cellId(c.cell)));
    // The fixture has failing cells; the first row must be one of them.
    expect(ruleFails(worstFirst(results.cells)[0]!)).toBeGreaterThan(0);
  });

  it("selects a cell and column on click and with the keyboard", () => {
    const onSelect = vi.fn();
    render(<Heatmap results={results} selection={null} onSelect={onSelect} />);
    const grid = screen.getByRole("grid");
    const firstRules = grid.querySelector('[data-pos="0:0"]') as HTMLElement;
    fireEvent.click(firstRules);
    expect(onSelect).toHaveBeenLastCalledWith(worstFirst(results.cells)[0], "rules");

    fireEvent.keyDown(grid, { key: "ArrowDown" });
    fireEvent.keyDown(grid, { key: "ArrowRight" });
    fireEvent.keyDown(grid, { key: "Enter" });
    const [cell, column] = onSelect.mock.calls.at(-1)!;
    expect(cell).toBe(worstFirst(results.cells)[1]);
    expect(column).not.toBe("rules");
  });

  it("marks the selected square", () => {
    const first = worstFirst(results.cells)[0]!;
    render(<Heatmap results={results} selection={{ cellId: cellId(first.cell), column: "rules" }} onSelect={() => {}} />);
    expect(screen.getByRole("grid").querySelector('[data-pos="0:0"]')?.getAttribute("aria-selected")).toBe("true");
  });
});
