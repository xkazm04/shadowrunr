// The agent layer's hook into the composition root (docs/guide/athena.md). It rides the first
// load, so it holds only the gate: the catalog is a dynamic chunk fetched the first time both
// conditions hold (a WebMCP registry and the operator's opt-in). Switching off aborts the one
// controller, and every tool registered under it is unregistered.
import { drive as pageDrive, type DriveStore } from "../../kit/athenaDrive";
import type { AgentDeps, Catalog } from "./types";

export function startAgent(deps: AgentDeps, drive: DriveStore = pageDrive, load: () => Promise<Catalog> = () => import("./catalog")): () => void {
  let ctrl: AbortController | null = null;
  const sync = () => {
    const registry = drive.registry();
    const want = registry !== undefined && drive.get();
    if (want && ctrl === null) {
      const c = new AbortController();
      ctrl = c;
      load().then(
        (m) => {
          if (!c.signal.aborted) m.registerTools(registry, deps, c.signal);
        },
        (e: unknown) => console.error("athena: the tool catalog did not load", e),
      );
    } else if (!want && ctrl !== null) {
      ctrl.abort();
      ctrl = null;
    }
  };
  sync();
  const off = drive.subscribe(sync);
  return () => {
    off();
    ctrl?.abort();
    ctrl = null;
  };
}
