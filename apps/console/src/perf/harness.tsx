// The perf harness (test-only): mounts a page on the real data layer, door and router with the
// data layer's own doubles, like kit/testing's mountPage, and also exposes what the counts need:
// the manual timers (idle ticks), the live socket, and a React <Profiler> commit counter.
import { act, render } from "@testing-library/react";
import { Profiler, type ReactNode } from "react";
import BUDGETS_JSON from "../../budgets/perf.json";
import { contractDoor } from "../data/contractDoor";
import { createDataLayer, DataProvider } from "../data";
import { fakeSockets, manualTimers, scriptedFetch } from "../data/testing";
import { nodes, settle, type Script } from "../kit/testing";
import { createHashRouter, RouterProvider } from "../router";

export interface Line {
  readonly max: number;
  readonly why: string;
}
export interface RpcLine {
  /** Max calls per method on first paint; a method not listed may not be called at all. */
  readonly calls: Readonly<Record<string, number>>;
  readonly why: string;
}
export interface DrawLine {
  /** Max passes (or commits) per input change. */
  readonly perChange: number;
  /** Max passes while idle over `idleTicks` ticks. */
  readonly idle: number;
  /** Max passes when an event refetches replies identical to what is drawn. */
  readonly sameData?: number;
  readonly why: string;
}
export interface Budgets {
  readonly dom: Readonly<Record<string, Line>>;
  readonly rpc: Readonly<Record<string, RpcLine>>;
  readonly draws: Readonly<Record<string, DrawLine>>;
  readonly commits: Readonly<Record<string, DrawLine>>;
  readonly idleTicks: number;
}

export const BUDGETS: Budgets = BUDGETS_JSON;

export function line<T>(table: Readonly<Record<string, T>>, name: string): T {
  const l = table[name];
  if (l === undefined) throw new Error(`budgets/perf.json has no line "${name}"`);
  return l;
}

export function mountPerf(ui: ReactNode, script: Script, hash: string) {
  window.location.hash = hash;
  const router = createHashRouter(window);
  const fetch = scriptedFetch((method, sent) => script(method, typeof sent === "string" ? sent : ""));
  const sockets = fakeSockets();
  const timers = manualTimers();
  const layer = createDataLayer({ baseUrl: "http://127.0.0.1:7717", door: contractDoor, fetch, socket: sockets.factory, timers });
  let commits = 0;
  const view = render(
    <DataProvider layer={layer}>
      <RouterProvider router={router}>
        <Profiler id="perf" onRender={() => (commits += 1)}>
          {ui}
        </Profiler>
      </RouterProvider>
    </DataProvider>,
  );
  return {
    calls: fetch.calls,
    timers,
    layer,
    socket: () => sockets.last(),
    commits: () => commits,
    dispose: () => {
      view.unmount();
      router.dispose();
    },
  };
}

export type Mounted = ReturnType<typeof mountPerf>;

/** Settles until the call log and the DOM hold still for three rounds (or `max` rounds pass). */
export async function quiet(m: Pick<Mounted, "calls">, max = 200): Promise<void> {
  let last = "";
  let still = 0;
  for (let i = 0; i < max && still < 3; i++) {
    await settle(1);
    const now = `${m.calls.length}:${nodes()}`;
    still = now === last ? still + 1 : 0;
    last = now;
  }
}

/** Idle: `ticks` one-second ticks of the data layer's timers plus real time for window timers. */
export async function idle(m: Pick<Mounted, "timers">, ticks: number): Promise<void> {
  for (let i = 0; i < ticks; i++) {
    await act(async () => {
      m.timers.advance(1000);
      await new Promise((r) => setTimeout(r, 5));
    });
  }
  await settle(2);
}

/** Calls per method. */
export function tally(calls: readonly string[]): Record<string, number> {
  const out: Record<string, number> = {};
  for (const c of calls) out[c] = (out[c] ?? 0) + 1;
  return out;
}

/** Methods called over budget, as "method: got > max" (an unlisted method has budget 0). */
export function overBudget(calls: readonly string[], budget: Readonly<Record<string, number>>): string[] {
  return Object.entries(tally(calls))
    .filter(([m, n]) => n > (budget[m] ?? 0))
    .map(([m, n]) => `${m}: ${n} > ${budget[m] ?? 0}`);
}

/** An enveloped live frame, valid against the contract schema. */
export const frame = (seq: number, event: unknown, runId = "live-run") => ({ seq, run_id: runId, event });
