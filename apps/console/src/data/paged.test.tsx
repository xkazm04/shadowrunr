// Cursor chains and fixed call sets over the real query cache (moved from Brain's queries.ts
// tests when the hooks were promoted to data/).
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ReactNode } from "react";
import type { RunRow } from "@shadowrunr/contract";
import { contractDoor } from "./contractDoor";
import { createDataLayer, type DataLayer } from "./layer";
import { usePaged, usePagedMany, useRpcMany } from "./paged";
import { DataProvider } from "./react";
import { fakeSockets, manualTimers, scriptedFetch, type FetchReply } from "./testing";

afterEach(cleanup);

const run = (i: number, tid = "t1"): RunRow => ({
  run_id: `r${i}`,
  tournament_id: tid,
  has_verdict: false,
  spec: { run_id: `r${i}`, character: "malakor", tone_variant: "theatrical", core_tier: "haiku", core_effort: "low", segment: "SEG-01", audience_size: 3, seed: i, duration_s: 60, population: "gen" },
});

const field = (sent: string, name: string) => new RegExp(`"${name}":"([^"]*)"`).exec(sent)?.[1];

function daemon(runs: Record<string, number>): (method: string, sent: string) => FetchReply {
  return (method, sent) => {
    if (method === "run.transcript") return { body: { result: [] } };
    if (method !== "runs.page") return { body: { error: { code: "method_not_found", message: method } } };
    const tid = field(sent, "tournament_id") ?? "t1";
    const all = Array.from({ length: runs[tid] ?? 0 }, (_, i) => run(i, tid));
    const limit = Number(/"limit":(\d+)/.exec(sent)?.[1] ?? "500");
    const from = Number(field(sent, "cursor") ?? "0");
    const items = all.slice(from, from + limit);
    return { body: { result: from + limit < all.length ? { items, next_cursor: String(from + limit) } : { items } } };
  };
}

function mount(ui: ReactNode, runs: Record<string, number>): { layer: DataLayer; calls: string[]; timers: ReturnType<typeof manualTimers> } {
  const fetch = scriptedFetch((m, sent) => daemon(runs)(m, typeof sent === "string" ? sent : ""));
  const timers = manualTimers();
  const layer = createDataLayer({ baseUrl: "http://x", door: contractDoor, fetch, socket: fakeSockets().factory, timers });
  render(<DataProvider layer={layer}>{ui}</DataProvider>);
  return { layer, calls: fetch.calls, timers };
}

async function settle(rounds = 12) {
  for (let i = 0; i < rounds; i++) await act(async () => void (await new Promise((r) => setTimeout(r, 0))));
}

function Chain({ max }: { max?: number }) {
  const q = usePaged("runs.page", { limit: 100 }, max);
  return (
    <p>
      {q.query.status} · items {q.query.data?.length ?? "none"} · pages {q.pages} · more {String(q.more)} · done {String(q.done)}
    </p>
  );
}

function Many() {
  const q = usePagedMany("runs.page", [
    { tournament_id: "t1", limit: 100 },
    { tournament_id: "t2", limit: 100 },
  ]);
  return <p>{q.map((c) => `${c.query.data?.length ?? 0}/${String(c.done)}`).join(" ")}</p>;
}

function Each({ ids }: { ids: readonly string[] }) {
  const q = useRpcMany(
    "run.transcript",
    ids.map((run_id) => ({ run_id })),
  );
  return (
    <p>
      loaded {q.data.filter((d) => d !== undefined).length} of {ids.length} · ready {String(q.ready)}
    </p>
  );
}

describe("usePaged", () => {
  it("follows next_cursor up to maxPages and reports that more remain", async () => {
    const { calls } = mount(<Chain max={3} />, { t1: 1000 });
    await settle();
    expect(screen.getByText("ready · items 300 · pages 3 · more true · done true")).toBeTruthy();
    expect(calls).toEqual(["runs.page", "runs.page", "runs.page"]);
  });

  it("reads every page by default and stops at the last one", async () => {
    mount(<Chain />, { t1: 250 });
    await settle();
    expect(screen.getByText("ready · items 250 · pages 3 · more false · done true")).toBeTruthy();
  });

  it("refetches every page when an event invalidates the method", async () => {
    const { calls, layer, timers } = mount(<Chain max={2} />, { t1: 300 });
    await settle();
    await act(async () => {
      layer.cache.onEvent({ type: "stream_end" });
      timers.advance(250);
    });
    await settle();
    expect(calls.filter((c) => c === "runs.page").length).toBe(4);
  });

  it("follows several chains at once", async () => {
    mount(<Many />, { t1: 250, t2: 40 });
    await settle();
    expect(screen.getByText("250/true 40/true")).toBeTruthy();
  });
});

describe("useRpcMany", () => {
  it("reads one call per entry and is ready once all answered", async () => {
    const { calls } = mount(<Each ids={["a", "b", "c"]} />, {});
    await settle(8);
    expect(screen.getByText("loaded 3 of 3 · ready true")).toBeTruthy();
    expect(calls).toEqual(["run.transcript", "run.transcript", "run.transcript"]);
  });
});
