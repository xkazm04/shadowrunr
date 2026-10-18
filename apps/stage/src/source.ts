// Where a transcript and its case come from: URL parameters, fetches, drag and drop, and the
// daemon WebSocket (contract § daemon protocol).

import type { Case, StreamEvent } from "@shadowrunr/contract";
import { checkCase, parseStreamEvent } from "@shadowrunr/stage-kit/parse";

export const DEFAULT_WS = "ws://127.0.0.1:7717/events";
export const SPEEDS = [0.25, 0.5, 1, 1.5, 2, 4, 8] as const;

export interface StageParams {
  /** `?file=<url>`: a transcript.jsonl to fetch. */
  file: string | null;
  /** `?case=<url>`: a SEG-02A Case JSON to draw the hexmap with. */
  caseUrl: string | null;
  /** `?broadcast=1`: hide every control (the OBS browser source view). */
  broadcast: boolean;
  speed: number;
  /** `?run=<run_id>|live`: subscribe on the daemon WebSocket instead of loading a file. */
  run: string | null;
  /** `?ws=<url>`: the daemon WebSocket URL (default ws://127.0.0.1:7717/events). */
  ws: string;
  /** `?loop=1`: restart a finished file replay after a short pause. */
  loop: boolean;
  /** `?debug=1`: outline anything that overflows its box. */
  debug: boolean;
  /** `?t=<seconds>`: start a file replay this far in (a deep link to a moment). */
  startAt: number;
  /** `?paused=1`: load paused (with `?t=`, a still frame). */
  paused: boolean;
}

function flag(p: URLSearchParams, k: string): boolean {
  const v = p.get(k);
  return v !== null && v !== "0" && v !== "false";
}

export function readParams(search: string): StageParams {
  const p = new URLSearchParams(search);
  const speedRaw = Number(p.get("speed") ?? "1");
  const speed = Number.isFinite(speedRaw) && speedRaw > 0 ? Math.min(16, speedRaw) : 1;
  const wsParam = p.get("ws");
  const run = p.get("run") ?? (wsParam !== null ? "live" : null);
  return {
    file: p.get("file"),
    caseUrl: p.get("case"),
    broadcast: flag(p, "broadcast"),
    speed,
    run,
    ws: wsParam && /^wss?:\/\//.test(wsParam) ? wsParam : DEFAULT_WS,
    loop: flag(p, "loop"),
    debug: flag(p, "debug"),
    startAt: Math.max(0, Number(p.get("t") ?? "0") || 0),
    paused: flag(p, "paused"),
  };
}

/** `runs/x/transcript.jsonl` → `runs/x/transcript.case.json` (an optional sidecar). */
export function sidecarCaseUrl(fileUrl: string): string | null {
  const [path, query] = fileUrl.split("?", 2);
  if (!path || !/\.jsonl$/i.test(path)) return null;
  const side = path.replace(/\.jsonl$/i, ".case.json");
  return query ? `${side}?${query}` : side;
}

export async function fetchText(url: string): Promise<string> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return res.text();
}

/** Fetch a case; `null` when it does not exist or does not parse (a sidecar is optional). */
export async function fetchCase(url: string): Promise<Case | null> {
  let text: string;
  try {
    const res = await fetch(url);
    if (!res.ok) return null;
    text = await res.text();
  } catch {
    // Unreachable sidecar: the same as an absent one.
    return null;
  }
  const c = checkCase(text);
  return c.ok ? c.value : null;
}

export type CaseSource = "transcript" | "param" | "sidecar" | "dropped" | "builtin" | "none";

/** The case of the first `case_open` in the transcript, if it has one. */
export function transcriptCase(events: readonly StreamEvent[]): Case | null {
  for (const e of events) if (e.type === "case_open") return e.case;
  return null;
}

/** Hex ids revealed anywhere in the transcript. */
export function revealedHexIds(events: readonly StreamEvent[]): string[] {
  const ids: string[] = [];
  for (const e of events) {
    if (e.type === "op" && e.op.op === "reveal_hex" && !ids.includes(e.op.hex)) ids.push(e.op.hex);
  }
  return ids;
}

/**
 * Which case to draw. The transcript's own `case_open` wins: `current` (the scene's case at the
 * playback position, which also covers a live stream) and otherwise the first `case_open` in
 * `events`. Older transcripts without one fall back to an explicit case (`?case=`, sidecar,
 * dropped), then to the built-in fictional demo case, used only when every hex the transcript
 * reveals exists in it (simulations run on that case), so an unrelated transcript never borrows
 * its title. Otherwise the map is drawn procedurally from hex ids.
 */
export function resolveCase(
  explicit: { case: Case; source: Exclude<CaseSource, "transcript" | "builtin" | "none"> } | null,
  events: readonly StreamEvent[],
  builtin: Case,
  current: Case | null = null,
): { case: Case | null; source: CaseSource } {
  const fromTranscript = current ?? transcriptCase(events);
  if (fromTranscript) return { case: fromTranscript, source: "transcript" };
  if (explicit) return explicit;
  const revealed = revealedHexIds(events);
  const known = new Set(builtin.hexes.map((h) => h.id));
  if (revealed.length > 0 && revealed.every((id) => known.has(id))) {
    return { case: builtin, source: "builtin" };
  }
  return { case: null, source: "none" };
}

export type WsStatus = "connecting" | "open" | "closed" | "error";

/**
 * Subscribe on the daemon WebSocket. Sends `{"subscribe": run, "speed": speed}` on open; every
 * message is one StreamEvent. Returns a close function. Reconnects with backoff until closed.
 */
export function connectDaemon(
  url: string,
  run: string,
  speed: number,
  onEvent: (ev: StreamEvent) => void,
  onStatus: (s: WsStatus, detail?: string) => void,
  onBadMessage: (detail: string) => void,
): () => void {
  let ws: WebSocket | null = null;
  let stopped = false;
  let retry = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const open = () => {
    onStatus("connecting");
    ws = new WebSocket(url);
    ws.onopen = () => {
      retry = 0;
      onStatus("open");
      ws?.send(JSON.stringify({ subscribe: run, speed }));
    };
    ws.onmessage = (m) => {
      if (typeof m.data !== "string") return;
      // The contract door decodes and validates the frame (bare event or enveloped).
      const r = parseStreamEvent(m.data);
      if (r.ok) onEvent(r.value);
      else onBadMessage(r.message);
    };
    ws.onerror = () => onStatus("error", url);
    ws.onclose = () => {
      onStatus("closed");
      if (stopped) return;
      retry++;
      timer = setTimeout(open, Math.min(10_000, 500 * 2 ** Math.min(retry, 5)));
    };
  };
  open();
  return () => {
    stopped = true;
    if (timer) clearTimeout(timer);
    ws?.close();
  };
}
