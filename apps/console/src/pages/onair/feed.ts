// What On Air plays: the operator's choice (a stored run or the live feed), and the feeds
// themselves. Both live in small external stores, so leaving layer 1 for a nested page and
// coming back resumes the same replay at the same place. Nothing here persists across reloads.
import type { RunRow, RunTranscriptResult } from "@shadowrunr/contract";
import { useCallback, useEffect, useSyncExternalStore } from "react";
import { useLiveEvents, useRpc, type Query } from "../../data";
import { LiveFeed, ReplayPlayer, type Feed, type FeedSnapshot } from "./player";
import { initialScene } from "@shadowrunr/stage-kit";

export type Source = { readonly kind: "live" } | { readonly kind: "run"; readonly runId: string; readonly tournamentId: string };

export const LIVE: Source = { kind: "live" };
/** Runs offered in the layer-1 selector (the rundown pages through all of them). */
export const RUN_CHOICES = 40;

export function sourceKey(s: Source): string {
  return s.kind === "live" ? "live" : `run:${s.tournamentId}/${s.runId}`;
}

export function runSource(r: Pick<RunRow, "run_id" | "tournament_id">): Source {
  return { kind: "run", runId: r.run_id, tournamentId: r.tournament_id };
}

function store<T>(initial: T) {
  let value = initial;
  const listeners = new Set<() => void>();
  return {
    get: () => value,
    set(next: T) {
      value = next;
      for (const l of [...listeners]) l();
    },
    subscribe(l: () => void) {
      listeners.add(l);
      return () => listeners.delete(l);
    },
  };
}

/** The operator's pick; null until they choose (then the newest stored run plays). */
const chosen = store<Source | null>(null);

export function selectSource(s: Source): void {
  chosen.set(s);
}

export function useChosenSource(): Source | null {
  return useSyncExternalStore(chosen.subscribe, chosen.get, chosen.get);
}

/** One replay at a time: switching runs disposes the old player. */
const replay = store<{ key: string; player: ReplayPlayer } | null>(null);

function ensureReplay(key: string, events: RunTranscriptResult): void {
  const cur = replay.get();
  if (cur?.key === key) return;
  cur?.player.dispose();
  replay.set({ key, player: new ReplayPlayer(events) });
}

/** The live feed folds every `/events` event while On Air is mounted. */
const live = new LiveFeed();

const IDLE_SNAPSHOT: FeedSnapshot = { scene: initialScene(), playing: false, position: 0, start: 0, end: 0, speed: 1, epoch: 0, live: false };
const noSubscribe = () => () => undefined;
const idle = () => IDLE_SNAPSHOT;

export interface OnAirFeed {
  readonly source: Source | null;
  readonly runs: Query<{ items: RunRow[]; next_cursor?: string }>;
  /** The transcript query of the chosen run (undefined for the live feed). */
  readonly transcript: Query<RunTranscriptResult> | undefined;
  readonly feed: Feed | null;
  readonly snapshot: FeedSnapshot;
  readonly select: (s: Source) => void;
}

/** Wires the chosen source to a feed; plays while `active`, pauses when hidden or unmounted. */
export function useOnAirFeed(active: boolean): OnAirFeed {
  const runs = useRpc("runs.page", { limit: RUN_CHOICES });
  const picked = useChosenSource();
  const first = runs.data?.items[0];
  const source = picked ?? (first ? runSource(first) : null);
  const run = source?.kind === "run" ? source : null;
  const transcript = useRpc("run.transcript", run ? { run_id: run.runId, tournament_id: run.tournamentId } : null);

  useLiveEvents((env) => live.push(env.event, env.runId));

  const key = source ? sourceKey(source) : null;
  const events = run ? transcript.data : undefined;
  useEffect(() => {
    if (key !== null && key !== "live" && events !== undefined && events.length > 0) ensureReplay(key, events);
  }, [key, events]);

  const current = useSyncExternalStore(replay.subscribe, replay.get, replay.get);
  const feed: Feed | null = source?.kind === "live" ? live : current !== null && current.key === key ? current.player : null;

  const subscribe = useCallback((l: () => void) => (feed ? feed.subscribe(l) : noSubscribe()), [feed]);
  const getSnapshot = useCallback(() => (feed ? feed.getSnapshot() : idle()), [feed]);
  const snapshot = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);

  useEffect(() => {
    if (!feed || !active) return;
    feed.play();
    return () => feed.pause();
  }, [feed, active]);

  return { source, runs, transcript: run ? transcript : undefined, feed, snapshot, select: selectSource };
}
