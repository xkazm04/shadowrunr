import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { contractDoor } from "./contractDoor";
import { createDataLayer, type DataLayer } from "./layer";
import { DataProvider, useConnection, useMutation, useRpc } from "./react";
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
