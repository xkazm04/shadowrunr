// QueryView's freshness: a region says how old its data is exactly when the live feed cannot
// vouch for it (the stream is down, or it reopened and the refetch has not landed), a failed
// refresh names the time of the reply it keeps showing, and a refresh over held data marks the
// region busy without a placeholder. The live path renders nothing extra.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createDataLayer, DataProvider, useRpc, type ConnectionSnapshot } from "../data";
import { contractDoor } from "../data/contractDoor";
import { fakeSockets, manualTimers, scriptedFetch, type FetchReply } from "../data/testing";
import { fail, ok, role, settle } from "./testing";
import { QueryView } from "./QueryView";
import { freshnessOf, timeOf } from "./freshness";

/** 14:02:31 local time, the moment the first reply lands. */
const AT = new Date(2026, 9, 21, 14, 2, 31).getTime();

function Probe() {
  const q = useRpc("tournament.list", {});
  return <QueryView query={q}>{(rows) => <p data-role="probe">{rows.length} rows</p>}</QueryView>;
}

/** A daemon answering `tournament.list` from `reply`, re-read on every call. */
const daemon = (reply: () => FetchReply) => (method: string) => (method === "tournament.list" ? reply() : fail("method_not_found", "nope"));

/** Probe on the real data layer with its doubles exposed; `hold` delays a reply until it settles. */
function mount(script: (method: string) => FetchReply, hold: () => Promise<void> | undefined = () => undefined) {
  const inner = scriptedFetch((method) => script(method));
  const fetch = Object.assign(
    (url: string, init: RequestInit & { body?: unknown }) => {
      const pending = hold();
      return pending ? pending.then(() => inner(url, init as never)) : inner(url, init as never);
    },
    { calls: inner.calls },
  );
  const sockets = fakeSockets();
  const timers = manualTimers();
  const layer = createDataLayer({ baseUrl: "http://127.0.0.1:7717", door: contractDoor, fetch: fetch as never, socket: sockets.factory, timers });
  render(
    <DataProvider layer={layer}>
      <Probe />
    </DataProvider>,
  );
  return { layer, sockets, timers, fetch: inner, calls: inner.calls };
}

const strip = () => role("query-freshness");

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(AT);
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("QueryView freshness", () => {
  it("guard: data with the stream open and no issue renders the content and nothing else", async () => {
    const page = mount(daemon(() => ok([])));
    page.sockets.last().open();
    await settle();
    expect(role("probe")?.textContent).toBe("0 rows");
    expect(strip()).toBeNull();
    expect(document.querySelector("[data-state]")).toBeNull();
  });

  it("says 'As of' with a Refresh while the stream is reconnecting", async () => {
    const page = mount(daemon(() => ok([])));
    page.sockets.last().open();
    await settle();
    page.sockets.last().drop();
    await settle();
    expect(role("probe")).not.toBeNull();
    expect(strip()?.getAttribute("role")).toBe("status");
    expect(strip()?.textContent).toContain("As of 14:02:31 · live updates paused");
    const before = page.calls.length;
    fireEvent.click(strip()!.querySelector("button")!);
    await settle();
    expect(page.calls.slice(before)).toEqual(["tournament.list"]);
  });

  it("titles a failed refresh with the time of the reply it keeps", async () => {
    let broken = false;
    const page = mount(daemon(() => (broken ? fail("internal", "boom") : ok([]))));
    page.sockets.last().open();
    await settle();
    broken = true;
    vi.setSystemTime(AT + 60_000);
    page.layer.cache.refetch("tournament.list", {});
    await settle();
    const panel = document.querySelector("[data-state='degraded']");
    expect(panel?.textContent).toMatch(/Showing the reply from 14:02:31 · /);
    expect(panel?.textContent).not.toContain("last good reply");
    expect(role("probe")).not.toBeNull();
  });

  it("marks the region busy while a refresh runs over held data, with no placeholder", async () => {
    let hang = false;
    const page = mount(daemon(() => (hang ? { hang: true } : ok([]))));
    page.sockets.last().open();
    await settle();
    hang = true;
    page.layer.cache.refetch("tournament.list", {});
    await settle();
    expect(document.querySelector("[aria-busy='true']")).not.toBeNull();
    expect(role("probe")?.textContent).toBe("0 rows");
    expect(document.querySelector("[data-state='loading']")).toBeNull();
  });

  it("stays unconfirmed after a reopen until the refetch lands, then goes quiet", async () => {
    const gate: { held?: Promise<void> } = {};
    let release = () => undefined as void;
    const page = mount(daemon(() => ok([])), () => gate.held);
    page.sockets.last().open();
    await settle();
    page.sockets.last().drop();
    vi.setSystemTime(AT + 90_000);
    gate.held = new Promise<void>((r) => (release = r));
    await act(async () => page.timers.advance(30_000));
    await act(async () => page.sockets.last().open());
    await settle();
    // Open again, but the reply on screen predates the reopen: the feed cannot vouch for it yet.
    expect(strip()?.textContent).toContain("As of 14:02:31");
    expect(strip()?.getAttribute("aria-busy")).toBe("true");
    release();
    await settle();
    expect(strip()).toBeNull();
    expect(page.fetch.calls).toEqual(["tournament.list", "tournament.list"]);
  });
});

describe("freshnessOf", () => {
  const open: ConnectionSnapshot = { rpc: "online", stream: "open", retryInMs: undefined, issue: undefined };
  const reconnecting: ConnectionSnapshot = { ...open, stream: "reconnecting" };

  it("is 'none' with no reply yet", () => {
    expect(freshnessOf({ updatedAt: undefined, error: undefined }, open, undefined)).toBe("none");
  });
  it("is 'live' when the open feed vouches for a reply fetched since the last reopen", () => {
    expect(freshnessOf({ updatedAt: 2_000, error: undefined }, open, 1_000)).toBe("live");
    expect(freshnessOf({ updatedAt: 2_000, error: undefined }, open, undefined)).toBe("live");
  });
  it("is 'unconfirmed' with the feed open but the reply older than the last reopen", () => {
    expect(freshnessOf({ updatedAt: 500, error: undefined }, open, 1_000)).toBe("unconfirmed");
  });
  it("is 'unconfirmed' while the feed is down", () => {
    expect(freshnessOf({ updatedAt: 2_000, error: undefined }, reconnecting, undefined)).toBe("unconfirmed");
  });
  it("is 'failed' when the last refresh failed over held data", () => {
    expect(freshnessOf({ updatedAt: 2_000, error: { code: "internal", message: "boom" } as never }, open, undefined)).toBe("failed");
  });
  it("formats wall-clock time as HH:MM:SS", () => {
    expect(timeOf(AT)).toBe("14:02:31");
  });
});

describe("Preflight reads its time from the query", () => {
  it("no page reaches around useRpc into the cache for a snapshot", () => {
    const walk = (dir: string): string[] =>
      readdirSync(dir).flatMap((n) => {
        const p = join(dir, n);
        return statSync(p).isDirectory() ? walk(p) : /\.tsx?$/.test(n) && !/\.test\./.test(n) ? [p] : [];
      });
    const hits = walk(join(__dirname, "..", "pages")).filter((f) => readFileSync(f, "utf8").includes("layer.cache.getSnapshot"));
    expect(hits).toEqual([]);
  });
});
