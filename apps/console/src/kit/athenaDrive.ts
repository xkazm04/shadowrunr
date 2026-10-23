// "Let Athena drive": the detector and the operator's opt-in, the only part of the agent layer
// that rides the first load. Athena (the operator's assistant) hosts the console as a tab and
// polyfills `document.modelContext`; the tool catalog (app/agent) is registered on it only
// while that registry exists AND the operator switched the toggle on. Outside Athena there is
// no registry, so the toggle is not rendered and nothing else of the agent layer loads.

/** The localStorage key of the opt-in; "1" is on, anything else (or nothing) is off. */
export const DRIVE_KEY = "shadowrunr.athena.drive";

/** The WebMCP registry, as much of it as the console uses. */
export interface ModelContextLike {
  registerTool(tool: { readonly name: string }, options?: { signal?: AbortSignal }): void;
  unregisterTool?(name: string): void;
}

export interface DriveStore {
  /** The page's WebMCP registry, or undefined outside Athena. */
  registry(): ModelContextLike | undefined;
  /** The opt-in as stored. */
  get(): boolean;
  set(on: boolean): void;
  subscribe(listener: () => void): () => void;
}

type StorageLike = Pick<Storage, "getItem" | "setItem">;

export function createDrive(doc: () => { modelContext?: ModelContextLike } | undefined, storage: () => StorageLike | undefined): DriveStore {
  const listeners = new Set<() => void>();
  const read = (): boolean => {
    try {
      return storage()?.getItem(DRIVE_KEY) === "1";
    } catch {
      return false;
    }
  };
  let on = read();
  return {
    registry: () => doc()?.modelContext,
    get: () => on,
    set(next) {
      on = next;
      try {
        storage()?.setItem(DRIVE_KEY, next ? "1" : "0");
      } catch {
        // Blocked storage: the switch still works for this page load.
        on = next;
      }
      for (const l of listeners) l();
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}

/** The page's own store: `document` and `localStorage`, each read when asked. */
export const drive: DriveStore = createDrive(
  () => (typeof document === "undefined" ? undefined : (document as Document & { modelContext?: ModelContextLike })),
  () => {
    try {
      return typeof localStorage === "undefined" ? undefined : localStorage;
    } catch {
      return undefined;
    }
  },
);
