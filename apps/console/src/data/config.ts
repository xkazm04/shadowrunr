import { DEFAULT_DAEMON_URL } from "./rpc";

export interface DataConfig {
  readonly daemonUrl: string;
}

/** `?daemon=http://127.0.0.1:7717` or `VITE_DAEMON_URL` override the default daemon address. */
export function readDataConfig(search: string, env: Readonly<Record<string, string | undefined>>): DataConfig {
  const q = new URLSearchParams(search);
  return { daemonUrl: q.get("daemon") ?? env["VITE_DAEMON_URL"] ?? DEFAULT_DAEMON_URL };
}
