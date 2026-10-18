// The cursor chain and the fixed call set over the real query cache.
import { act, cleanup, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { usePagedRpc, useRpcEach } from "./queries";
import { daemon, mount, season, settle } from "./testgen";

afterEach(cleanup);

function Chain({ max }: { max: number }) {
  const q = usePagedRpc("runs.page", { limit: 100 }, max);
  return (
    <p>
      items {q.data.items.length} · pages {q.data.pages} · more {String(q.data.more)} · ready {String(q.ready)}
    </p>
  );
}

function Each({ ids }: { ids: readonly string[] }) {
  const q = useRpcEach(
    "run.transcript",
    ids.map((run_id) => ({ run_id })),
  );
  return (
    <p>
      loaded {q.data.filter((d) => d !== undefined).length} of {ids.length} · ready {String(q.ready)}
    </p>
  );
}

describe("usePagedRpc", () => {
  it("follows next_cursor up to maxPages and reports that more remain", async () => {
    const { calls } = mount(<Chain max={3} />, daemon(season({ tournaments: 1, runs: 1000, rowsPerRun: 0, transcriptLines: 1 })), "#/brain");
    await settle(12);
    expect(screen.getByText("items 300 · pages 3 · more true · ready true")).toBeTruthy();
    expect(calls).toEqual(["runs.page", "runs.page", "runs.page"]);
  });

  it("stops at the last page", async () => {
    mount(<Chain max={10} />, daemon(season({ tournaments: 1, runs: 250, rowsPerRun: 0, transcriptLines: 1 })), "#/brain");
    await settle(12);
    expect(screen.getByText("items 250 · pages 3 · more false · ready true")).toBeTruthy();
  });

  it("refetches every page when an event invalidates the method", async () => {
    const { calls, layer } = mount(<Chain max={2} />, daemon(season({ tournaments: 1, runs: 300, rowsPerRun: 0, transcriptLines: 1 })), "#/brain");
    await settle(12);
    await act(async () => layer.cache.onEvent("stream_end"));
    await settle(12);
    expect(calls.filter((c) => c === "runs.page").length).toBe(4);
  });
});

describe("useRpcEach", () => {
  it("reads one call per entry and is ready once all answered", async () => {
    const { calls } = mount(<Each ids={["a", "b", "c"]} />, daemon(season({ tournaments: 1, runs: 1, rowsPerRun: 0, transcriptLines: 5 })), "#/brain");
    await settle(8);
    expect(screen.getByText("loaded 3 of 3 · ready true")).toBeTruthy();
    expect(calls).toEqual(["run.transcript", "run.transcript", "run.transcript"]);
  });
});
