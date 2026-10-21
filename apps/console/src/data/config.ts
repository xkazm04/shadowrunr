// No imports: errors.ts and rpc.ts read the daemon's address from here, so this module must not
// import either of them (import-x/no-cycle).

/** The daemon's default address (crates/shadowrunr-daemon `[daemon] port`, default 7717). */
export const DEFAULT_DAEMON_URL = "http://127.0.0.1:7717";

/** The stage's dev server (`pnpm -C apps/stage dev`, apps/stage/vite.config.ts). */
export const DEFAULT_STAGE_URL = "http://localhost:5178/";

export interface DataConfig {
  readonly daemonUrl: string;
  /** Where the broadcast stage is served; "Open in stage" links point here. */
  readonly stageUrl: string;
}

type Env = Readonly<Record<string, string | undefined>>;

declare global {
  interface Window {
    /** Set by the desktop shell before any console code runs (src-tauri/src/endpoint.rs). */
    readonly __SHADOWRUNR_DAEMON__?: string;
  }
}

/** The daemon the desktop shell resolved, or undefined in a plain browser. */
export function shellDaemonUrl(): string | undefined {
  const v = typeof window === "undefined" ? undefined : window.__SHADOWRUNR_DAEMON__;
  return typeof v === "string" && v !== "" ? v : undefined;
}

/**
 * The daemon: the desktop shell's value (the one origin its window CSP admits), else
 * `?daemon=http://127.0.0.1:7717`, else `VITE_DAEMON_URL`, else the default.
 * `?stage=<url>` or `VITE_STAGE_URL` override the stage's address.
 */
export function readDataConfig(search: string, env: Env, shell: string | undefined = shellDaemonUrl()): DataConfig {
  const q = new URLSearchParams(search);
  return {
    daemonUrl: shell ?? q.get("daemon") ?? env["VITE_DAEMON_URL"] ?? DEFAULT_DAEMON_URL,
    stageUrl: q.get("stage") ?? env["VITE_STAGE_URL"] ?? DEFAULT_STAGE_URL,
  };
}

/** The daemon URL the console runs against: the shell, its own query string and build env decide. */
export function currentDaemonUrl(): string {
  const env: Env = import.meta.env;
  return readDataConfig(typeof window === "undefined" ? "" : window.location.search, env).daemonUrl;
}

/** The port of a daemon URL as the operator types it to `shadowrunr daemon --port`. */
export function daemonPort(daemonUrl: string): string {
  return /:(\d+)\/*$/.exec(daemonUrl)?.[1] ?? "80";
}

/** The stage URL the console runs against: its own query string and build env decide. */
function currentStageUrl(): string {
  const env: Env = import.meta.env;
  return readDataConfig(typeof window === "undefined" ? "" : window.location.search, env).stageUrl;
}

/**
 * The stage replaying one run: `?run=<id>` subscribes it to the daemon's stored replay
 * (apps/stage `readParams`; contract § daemon protocol). A run id may be tournament-qualified
 * (`<tournament_id>/<run_id>`). An unparseable base falls back to the default stage.
 */
export function stageReplayUrl(runId: string, stageUrl: string = currentStageUrl()): string {
  let u: URL;
  try {
    u = new URL(stageUrl);
  } catch (e) {
    if (!(e instanceof TypeError)) throw e;
    u = new URL(DEFAULT_STAGE_URL);
  }
  u.searchParams.set("run", runId);
  return u.toString();
}
