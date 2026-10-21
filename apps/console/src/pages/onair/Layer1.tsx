// On Air, layer 1: the poster body (brand bar, the huge "On Air" title, the stream screen, the
// replay scrubber and transport, and the credit with its call to action), as the winner's l1.js `onair`.
// The screen replays a real run (runs.page + run.transcript) or follows the live feed; A/1's
// selector picks which. When the latest pre-flight has a red check, the transport says so and
// links to Brain's board (the same `preflight.run` query entry the board reads).
import type { PreflightRunResult, SegmentTables, StreamEvent } from "@shadowrunr/contract";
import { clipMarkers, segmentSpans } from "@shadowrunr/stage-kit";
import { useMemo, type CSSProperties } from "react";
import { useConnection, useRpc } from "../../data";
import { PosterFrame } from "../../kit/PosterFrame";
import { RouteLink, pageRoute } from "../../router";
import { floorRoute, transcriptRoute } from "../../router/brain";
import { LIVE, runSource, sourceKey, useOnAirFeed, type OnAirFeed } from "./feed";
import styles from "./Layer1.module.css";
import type { Feed, FeedSnapshot } from "./player";
import { Screen, type RundownPill } from "./Screen";

const LIVE_SEGMENTS = ["SEG-01", "SEG-02A"] as const;

export function pillsOf(tables: SegmentTables | undefined): { pills: RundownPill[]; descoped: number } {
  const pills = LIVE_SEGMENTS.map((id) => ({ id, title: tables?.segments.find((s) => s.id === id)?.title ?? "" }));
  const descoped = tables?.segments.filter((s) => s.status === "descoped" || s.status === "retired").length ?? 0;
  return { pills, descoped };
}

/** The designed states, in the screen's own words: what the HUD says and whether it is off air. */
export function screenState(feed: OnAirFeed, offline: boolean): { status?: string; offAir?: string } {
  if (feed.source?.kind === "live") {
    if (offline) return { status: "Live · the daemon is not reachable", offAir: "The daemon is not reachable. The live feed resumes when it answers." };
    if (feed.snapshot.gap) return { status: "Live · missed events, catching up" };
    return feed.snapshot.scene.applied === 0 ? { status: "Live · waiting for the stream", offAir: "Nothing is on air. A stream or a simulation shows here the moment it starts." } : {};
  }
  if (feed.runs.data === undefined) {
    if (feed.runs.errorView) return { status: `Off air · ${feed.runs.errorView.title}`, offAir: feed.runs.errorView.hint };
    return { status: "Loading the runs" };
  }
  if (feed.source === null) return { status: "No runs yet", offAir: "No run has been recorded yet. Start a simulation, or switch to the live feed." };
  const t = feed.transcript;
  if (t?.data === undefined) {
    if (t?.errorView) return { status: `Off air · ${t.errorView.title}`, offAir: t.errorView.hint };
    return { status: "Loading the run" };
  }
  if (t.data.length === 0) return { status: "This run has no events", offAir: "This run recorded no events. Pick another run." };
  return {};
}

/** `m:ss` for the slider's spoken value (kit/events' span, inlined: importing that module pulls
 * its shared chunk, about 6 KB, into the first layer). */
const clock = (ms: number): string => {
  const s = Math.max(0, Math.round(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

/** Moment ticks: one per marked moment at this resolution, never more than MAX_TICKS nodes. */
const TICK_RESOLUTION = 200;
/** The perf gate's onair.l1 line (260 nodes at 10x) leaves room for about this many. */
const MAX_TICKS = 12;
/** One arrow key steps the replay this far in transcript time (the stage's own step). */
const STEP_MS = 5000;
const PAGE_MS = 30_000;

function binned(marks: readonly { frac: number; label: string }[], bins: number): { frac: number; labels: string[] }[] {
  const out = new Map<number, { frac: number; labels: string[] }>();
  for (const m of marks) {
    const bin = Math.min(bins - 1, Math.floor(m.frac * bins));
    const b = out.get(bin);
    if (b) b.labels.push(m.label);
    else out.set(bin, { frac: m.frac, labels: [m.label] });
  }
  return [...out.values()];
}

/**
 * The run's marked moments as scrubber ticks: one per moment (moments closer than 0.5% share a
 * tick), and when a run marks more than MAX_TICKS places, coarser bins (a 100k-line transcript
 * with 10k moments is still a dozen nodes). A tick's title names its moments.
 */
export function momentTicks(events: readonly StreamEvent[]): { frac: number; label: string }[] {
  const marks = clipMarkers(events);
  let bins = binned(marks, TICK_RESOLUTION);
  if (bins.length > MAX_TICKS) bins = binned(marks, MAX_TICKS);
  return bins.map((b) => ({ frac: b.frac, label: b.labels.length > 3 ? `${b.labels.slice(0, 3).join(" · ")} · +${b.labels.length - 3}` : b.labels.join(" · ") }));
}

/**
 * The replay's position as a slider over the run in transcript time: arrows step 5 s, Page keys
 * 30 s, Home and End jump to the ends, and a pointer press or drag seeks. Behind the needle, the
 * run's segment bands and its marked moments. The fill animates between events from the one
 * snapshot (keyed on its epoch), so nothing ticks per frame. Disabled on the live feed.
 */
function Scrubber({ snap, feed, events }: { snap: FeedSnapshot; feed: Feed | null; events: readonly StreamEvent[] | undefined }) {
  const span = Math.max(1, snap.end - snap.start);
  const from = snap.live ? 1 : Math.min(1, Math.max(0, (snap.position - snap.start) / span));
  const restMs = Math.max(0, ((snap.end - snap.position) / snap.speed) | 0);
  const style = { "--from": from.toFixed(4), "--rest": `${restMs}ms` } as CSSProperties;
  const disabled = snap.live || feed === null;
  const bands = useMemo(() => (events && !snap.live ? segmentSpans(events) : []), [events, snap.live]);
  const ticks = useMemo(() => (events && !snap.live ? momentTicks(events) : []), [events, snap.live]);
  const seek = (t: number) => {
    if (!disabled) feed.seek(t);
  };
  const at = (el: HTMLElement, clientX: number) => {
    const r = el.getBoundingClientRect();
    return snap.start + (r.width > 0 ? Math.min(1, Math.max(0, (clientX - r.left) / r.width)) : 0) * span;
  };
  const pct = (t: number) => `${(Math.min(1, Math.max(0, (t - snap.start) / span)) * 100).toFixed(3)}%`;
  return (
    <div
      className={styles.prog}
      role="slider"
      tabIndex={disabled ? -1 : 0}
      aria-label="Replay position"
      aria-valuemin={snap.start}
      aria-valuemax={snap.end}
      aria-valuenow={snap.position}
      aria-valuetext={snap.live ? "Live" : `${clock(snap.position - snap.start)} of ${clock(snap.end - snap.start)}`}
      aria-disabled={disabled || undefined}
      data-role="onair-progress"
      onKeyDown={(e) => {
        const to =
          e.key === "ArrowRight" || e.key === "ArrowUp"
            ? snap.position + STEP_MS
            : e.key === "ArrowLeft" || e.key === "ArrowDown"
              ? snap.position - STEP_MS
              : e.key === "PageUp"
                ? snap.position + PAGE_MS
                : e.key === "PageDown"
                  ? snap.position - PAGE_MS
                  : e.key === "Home"
                    ? snap.start
                    : e.key === "End"
                      ? snap.end
                      : null;
        if (to === null || disabled) return;
        e.preventDefault();
        seek(to);
      }}
      onPointerDown={(e) => {
        if (disabled || e.button !== 0) return;
        e.currentTarget.setPointerCapture?.(e.pointerId);
        seek(at(e.currentTarget, e.clientX));
      }}
      onPointerMove={(e) => {
        if (!disabled && e.currentTarget.hasPointerCapture?.(e.pointerId)) seek(at(e.currentTarget, e.clientX));
      }}
    >
      {bands.map((b) => (
        <span key={`${b.segment}@${b.from}`} className={styles.band} data-seg={b.segment} style={{ left: pct(b.from), width: pct(snap.start + b.to - b.from) }} />
      ))}
      <i key={snap.epoch} className={styles.fill} style={style} data-run={snap.playing && !snap.live ? true : undefined} />
      {ticks.map((t) => (
        <span key={t.frac} className={styles.tick} data-mark title={t.label} style={{ left: `${(t.frac * 100).toFixed(3)}%` }} />
      ))}
    </div>
  );
}

/** The one preflight query's params (Brain's board reads the same cache entry). */
const PREFLIGHT_PARAMS = {} as const;
const PREFLIGHT_BOARD = floorRoute("preflight");

/** The red checks of the latest pre-flight, by id; empty when none is red or none has run. */
export function redChecks(reports: PreflightRunResult | undefined): string[] {
  return (reports ?? []).filter((r) => r.state === "red").map((r) => r.check);
}

function Transport({ feed, red }: { feed: OnAirFeed; red: readonly string[] }) {
  const { snapshot, source, runs } = feed;
  const items = runs.data?.items ?? [];
  const value = source ? sourceKey(source) : "";
  const known = source === null || source.kind === "live" || items.some((r) => sourceKey(runSource(r)) === value);
  return (
    <div className={styles.ctl} role="group" aria-label="Replay controls" data-role="onair-transport">
      <button type="button" disabled={snapshot.live || !feed.feed} onClick={() => (snapshot.playing ? feed.feed?.pause() : feed.feed?.play())}>
        {snapshot.playing && !snapshot.live ? "Pause" : "Play"}
      </button>
      <button type="button" disabled={snapshot.live || !feed.feed} onClick={() => feed.feed?.restart()}>
        Restart
      </button>
      {red.length > 0 && (
        <RouteLink to={PREFLIGHT_BOARD} className={styles.pfRed} data-role="onair-preflight-red" title={`Red: ${red.join(", ")}. Open the pre-flight board`}>
          Pre-flight red
        </RouteLink>
      )}
      <span className={styles.sp} />
      <span className={styles.lab}>{source?.kind === "live" ? "Live" : "Replay"}</span>
      <select
        className={styles.pick}
        aria-label="Choose a real run to replay, or the live feed"
        value={value}
        onChange={(e) => {
          if (e.target.value === "live") return feed.select(LIVE);
          const row = items.find((r) => sourceKey(runSource(r)) === e.target.value);
          if (row) feed.select(runSource(row));
        }}
      >
        {source === null && <option value="">No runs yet</option>}
        <option value="live">Live feed</option>
        {!known && source?.kind === "run" && <option value={value}>{source.runId}</option>}
        {items.map((r) => (
          <option key={sourceKey(runSource(r))} value={sourceKey(runSource(r))}>
            {r.spec.tone_variant.replace(/-/g, " ")} · {r.spec.core_tier} core · {r.spec.segment} · {r.spec.audience_size} viewers
          </option>
        ))}
      </select>
    </div>
  );
}

/** The configured character's active tone (the operator's choice, else its default), in words. */
function useActiveTone(active: boolean): string | undefined {
  const list = useRpc("character.list", active ? {} : null);
  const c = list.data?.find((x) => x.id === "malakor") ?? list.data?.[0];
  return c === undefined ? undefined : c.active_tone.replace(/-/g, " ");
}

export function Layer1({ active }: { readonly active: boolean }) {
  const feed = useOnAirFeed(active);
  const conn = useConnection();
  const tables = useRpc("segments.tables", {});
  // The Lord's dispatch on the slip: the plan the operator chose (absent until one is chosen).
  const plan = useRpc("plan.current", active ? {} : null);
  const tone = useActiveTone(active);
  const preflight = useRpc("preflight.run", active ? PREFLIGHT_PARAMS : null);
  const red = redChecks(preflight.data);
  const { pills, descoped } = pillsOf(tables.data);
  const state = screenState(feed, conn.summary === "offline");
  const run = feed.source?.kind === "run" ? feed.source : null;
  const chatRoute = run ? transcriptRoute(run) : floorRoute("transcripts");
  if (!active) return null;
  return (
    <PosterFrame
      className={styles.l1}
      role="onair-layer1"
      measure
      credit={tone === undefined ? "A broadcast of the Overlord pilot · " : `A broadcast of the Overlord pilot · next stream's tone: ${tone} · `}
      note="v0.2 · voiced and recorded · stylised illustration"
      cta={{ to: pageRoute("onair", "rundown"), label: "Open the rundown", role: "push-in" }}
    >
      <div className={styles.hero}>
        <div className={styles.title} aria-hidden="true" data-role="onair-title">
          <span>On</span>
          <span>Air</span>
        </div>
        <div className={styles.screenSlot}>
          <Screen snapshot={feed.snapshot} pills={pills} descoped={descoped} chatRoute={chatRoute} status={state.status} offAir={state.offAir} dispatch={plan.data?.lord_dispatch} />
        </div>
        <Scrubber snap={feed.snapshot} feed={feed.feed} events={feed.source?.kind === "run" ? feed.transcript?.data : undefined} />
        <Transport feed={feed} red={red} />
      </div>
    </PosterFrame>
  );
}
