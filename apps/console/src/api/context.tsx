import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { RpcMethod, RpcParams, RpcResult } from "../contract";
import { daemonEvents, type EventSource } from "./events";
import { DEFAULT_DAEMON_URL, RpcClient, httpTransport, type ConnectionState } from "./rpc";

export interface ConsoleConfig {
  mock: boolean;
  daemonUrl: string;
  stageUrl: string;
}

/** URL switches: `?mock=1`, `?daemon=http://127.0.0.1:7717`, `?stage=http://localhost:5173`. */
export function readConfig(search: string = window.location.search): ConsoleConfig {
  const q = new URLSearchParams(search);
  const env = import.meta.env as Record<string, string | undefined>;
  return {
    mock: q.get("mock") === "1",
    daemonUrl: q.get("daemon") ?? env.VITE_DAEMON_URL ?? DEFAULT_DAEMON_URL,
    stageUrl: q.get("stage") ?? env.VITE_STAGE_URL ?? "http://localhost:5173/",
  };
}

interface ApiValue {
  config: ConsoleConfig;
  client: RpcClient;
  events: EventSource;
  connection: ConnectionState;
}

const ApiContext = createContext<ApiValue | null>(null);

export function ApiProvider({ config, children }: { config: ConsoleConfig; children: ReactNode }) {
  const [connection, setConnection] = useState<ConnectionState>(config.mock ? "mock" : "unknown");
  const [mockParts, setMockParts] = useState<{ client: RpcClient; events: EventSource } | null>(null);

  // The mock and its fixtures are code-split: they never load unless ?mock=1.
  useEffect(() => {
    if (!config.mock) return;
    let live = true;
    void import("./mock").then((m) => {
      if (live) setMockParts({ client: new RpcClient(m.mockTransport(), { mock: true }), events: m.mockEvents() });
    });
    return () => {
      live = false;
    };
  }, [config.mock]);

  const httpParts = useMemo(
    () =>
      config.mock
        ? null
        : {
            client: new RpcClient(httpTransport({ baseUrl: config.daemonUrl }), { onState: setConnection }),
            events: daemonEvents(config.daemonUrl),
          },
    [config.mock, config.daemonUrl],
  );

  const parts = httpParts ?? mockParts;
  if (!parts) return null;
  return (
    <ApiContext.Provider value={{ config, client: parts.client, events: parts.events, connection }}>
      {children}
    </ApiContext.Provider>
  );
}

export function useApi(): ApiValue {
  const v = useContext(ApiContext);
  if (!v) throw new Error("useApi outside ApiProvider");
  return v;
}

export interface Query<T> {
  data: T | undefined;
  error: unknown;
  loading: boolean;
  reload: () => void;
}

/** Load an RPC result; `params === null` skips the call. Re-runs when `key` changes. */
export function useRpc<M extends RpcMethod>(method: M, params: RpcParams<M> | null, key: string = ""): Query<RpcResult<M>> {
  const { client } = useApi();
  const skip = params === null;
  const fullKey = `${method}|${skip ? "-" : key}`;
  const [state, setState] = useState<{ key: string; data?: RpcResult<M> | undefined; error?: unknown; loading: boolean }>({
    key: fullKey,
    loading: !skip,
  });
  const [tick, setTick] = useState(0);
  const paramsRef = useRef(params);
  paramsRef.current = params;

  useEffect(() => {
    const p = paramsRef.current;
    if (p === null) {
      setState({ key: fullKey, loading: false });
      return;
    }
    const ctrl = new AbortController();
    // Keep data while reloading the same key; drop it when the key changes.
    setState((s) => ({ key: fullKey, data: s.key === fullKey ? s.data : undefined, loading: true }));
    client.call(method, p, ctrl.signal).then(
      (data) => !ctrl.signal.aborted && setState({ key: fullKey, data, loading: false }),
      (error: unknown) => !ctrl.signal.aborted && setState({ key: fullKey, error, loading: false }),
    );
    return () => ctrl.abort();
  }, [client, method, fullKey, tick]);

  const reload = useCallback(() => setTick((t) => t + 1), []);
  const current = state.key === fullKey;
  return {
    data: current ? state.data : undefined,
    error: current ? state.error : undefined,
    loading: current ? state.loading : !skip,
    reload,
  };
}
