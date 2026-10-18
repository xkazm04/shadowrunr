import { DEFAULT_DAEMON_URL } from "./rpc";

/** The stage's dev server (`pnpm -C apps/stage dev`, apps/stage/vite.config.ts). */
export const DEFAULT_STAGE_URL = "http://localhost:5178/";

export interface DataConfig {
  readonly daemonUrl: string;
  /** Where the broadcast stage is served; "Open in stage" links point here. */
  readonly stageUrl: string;
}

type Env = Readonly<Record<string, string | undefined>>;

/**
 * `?daemon=http://127.0.0.1:7717` or `VITE_DAEMON_URL` override the default daemon address;
 * `?stage=<url>` or `VITE_STAGE_URL` override the stage's.
 */
export function readDataConfig(search: string, env: Env): DataConfig {
  const q = new URLSearchParams(search);
  return {
    daemonUrl: q.get("daemon") ?? env["VITE_DAEMON_URL"] ?? DEFAULT_DAEMON_URL,
    stageUrl: q.get("stage") ?? env["VITE_STAGE_URL"] ?? DEFAULT_STAGE_URL,
  };
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
