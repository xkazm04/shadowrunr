import { act, cleanup, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { contractDoor } from "./contractDoor";
import { createDataLayer, type DataLayer } from "./layer";
import { DataProvider, useConnection, useInflight, useMutation, useRpc } from "./react";
import { fakeSockets, manualTimers, scriptedFetch, type FetchReply } from "./testing";

const flush = () => act(async () => void (await new Promise((r) => setTimeout(r, 0))));

function layerWith(reply: (method: string) => FetchReply) {
  const sockets = fakeSockets();
  const fetch = scriptedFetch(reply);
  const layer = createDataLayer({ baseUrl: "http://x", door: contractDoor, fetch, socket: sockets.factory, timers: manualTimers() });
  return { layer, sockets, fetch };
}

function Directives() {
  const q = useRpc("directive.list", {});
  const conn = useConnection();
  if (q.errorView) return <p>error: {q.errorView.state}</p>;
  return (
    <p>
      {q.status}: {(q.data ?? []).map((d) => d.text).join(", ")} · {conn.summary}
    </p>
  );
}

function AddButton({ onDone }: { onDone: (ok: boolean) => void }) {
  const m = useMutation("directive.add");
  return (
    <button type="button" onClick={() => void m.run({ kind: "try", text: "new" }).then((r) => onDone(r.ok))}>
      add
    </button>
  );
}

const D = (id: string, text: string) => ({ id, kind: "try", text, created_ms: 1, active: true });

afterEach(cleanup);

describe("typed hooks", () => {
  let layer: DataLayer;

  it("reads a typed query and re-renders when a mutation invalidates it", async () => {
    let list = [D("d1", "Hold the hexmap")];
    const h = layerWith((m) => (m === "directive.add" ? { body: { result: D("d2", "new") } } : { body: { result: list } }));
    layer = h.layer;
    let ok: boolean | null = null;
    render(
      <DataProvider layer={layer}>
        <Directives />
        <AddButton onDone={(r) => (ok = r)} />
      </DataProvider>,
    );
    await flush();
    expect(screen.getByText(/ready: Hold the hexmap/)).toBeTruthy();
    list = [...list, D("d2", "new")];
    await act(async () => screen.getByText("add").click());
    await flush();
    expect(ok).toBe(true);
    expect(screen.getByText(/ready: Hold the hexmap, new/)).toBeTruthy();
    expect(h.fetch.calls).toEqual(["directive.list", "directive.add", "directive.list"]);
  });

  it("gives a mutation its own timeout (the LLM-backed calls run for minutes)", async () => {
    const h = layerWith(() => ({ hang: true }));
    let reason: unknown;
    function Run() {
      const m = useMutation("reconcile.run", { timeoutMs: 5 });
      return (
        <button type="button" onClick={() => void m.run({ source: { tournament_id: "t" } }).then((r) => (reason = r.ok ? "ok" : r.error.detail?.reason))}>
          run
        </button>
      );
    }
    render(
      <DataProvider layer={h.layer}>
        <Run />
      </DataProvider>,
    );
    await act(async () => screen.getByText("run").click());
    await act(async () => void (await new Promise((r) => setTimeout(r, 30))));
    expect(reason).toBe("timeout");
  });

  it("shows the offline state from the error taxonomy", async () => {
    const h = layerWith(() => ({ network: true }));
    render(
      <DataProvider layer={h.layer}>
        <Directives />
      </DataProvider>,
    );
    await flush();
    expect(screen.getByText("error: offline")).toBeTruthy();
  });

  it("opens the live stream on mount and closes it on unmount", async () => {
    const h = layerWith(() => ({ body: { result: [] } }));
    const view = render(
      <DataProvider layer={h.layer}>
        <Directives />
      </DataProvider>,
    );
    expect(h.sockets.all).toHaveLength(1);
    view.unmount();
    expect(h.sockets.last().closed).toEqual({ code: 1000, reason: "client closed" });
  });
});

describe("in-flight mutations outlive the view that started them", () => {
  function RunButton() {
    const m = useMutation("reconcile.run");
    const inflight = useInflight("reconcile.run");
    return (
      <>
        <button type="button" disabled={m.pending || m.running.length > 0} onClick={() => void m.run({ source: { tournament_id: "t1" } })}>
          run
        </button>
        <ul>
          {inflight.map((f) => (
            <li key={f.key} data-started={f.startedAt}>
              {(f.params as { source: { tournament_id: string } }).source.tournament_id}
            </li>
          ))}
        </ul>
      </>
    );
  }

  it("a fresh instance after an unmount sees the run in flight and disables its button", async () => {
    const h = layerWith(() => ({ hang: true }));
    function Host() {
      const [shown, setShown] = useState(true);
      return (
        <>
          <button type="button" onClick={() => setShown((x) => !x)}>
            toggle
          </button>
          {shown && <RunButton />}
        </>
      );
    }
    render(
      <DataProvider layer={h.layer}>
        <Host />
      </DataProvider>,
    );
    await act(async () => screen.getByText("run").click());
    await flush();
    await act(async () => screen.getByText("toggle").click());
    expect(screen.queryByText("run")).toBeNull();
    await act(async () => screen.getByText("toggle").click());
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(1);
    expect(items[0]?.textContent).toBe("t1");
    expect(Number(items[0]?.getAttribute("data-started"))).toBeGreaterThan(0);
    expect((screen.getByText("run") as HTMLButtonElement).disabled).toBe(true);
    expect(h.fetch.calls.filter((m) => m === "reconcile.run")).toHaveLength(1);
  });

});
