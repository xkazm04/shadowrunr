// On Air, layer 1: the poster body (brand bar, the huge "On Air" title, the stream screen, the
// replay transport and the credit with its call to action), as the winner's l1.js `onair`.
// The screen replays a real run (runs.page + run.transcript) or follows the live feed; A/1's
// selector picks which.
import type { SegmentTables } from "@shadowrunr/contract";
import type { CSSProperties } from "react";
import { useConnection, useRpc } from "../../data";
import { RouteLink, pageRoute } from "../../router";
import { LIVE, runSource, sourceKey, useOnAirFeed, type OnAirFeed } from "./feed";
import styles from "./Layer1.module.css";
import type { FeedSnapshot } from "./player";
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

function Progress({ snap }: { snap: FeedSnapshot }) {
  const span = Math.max(1, snap.end - snap.start);
  const from = snap.live ? 1 : Math.min(1, Math.max(0, (snap.position - snap.start) / span));
  const restMs = Math.max(0, ((snap.end - snap.position) / snap.speed) | 0);
  const style = { "--from": from.toFixed(4), "--rest": `${restMs}ms` } as CSSProperties;
  return (
    <div className={styles.prog} aria-hidden="true" data-role="onair-progress">
      <i key={snap.epoch} style={style} data-run={snap.playing && !snap.live ? true : undefined} />
    </div>
  );
}

function Transport({ feed }: { feed: OnAirFeed }) {
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

export function Layer1({ active }: { readonly active: boolean }) {
  const feed = useOnAirFeed(active);
  const conn = useConnection();
  const tables = useRpc("segments.tables", {});
  const { pills, descoped } = pillsOf(tables.data);
  const state = screenState(feed, conn.summary === "offline");
  const run = feed.source?.kind === "run" ? feed.source : null;
  const chatRoute = run ? pageRoute("brain", "transcripts", run.runId) : pageRoute("brain", "transcripts");
  if (!active) return null;
  return (
    <div className={styles.l1} data-role="onair-layer1">
      <div className={styles.top}>
        <div className={styles.brand}>
          <svg viewBox="0 0 40 40" aria-hidden="true">
            <path d="M6 16L9 5l7 7 4-9 4 9 7-7 3 11z" fill="currentColor" />
            <ellipse cx="20" cy="26" rx="11" ry="10" fill="none" stroke="currentColor" strokeWidth="3" />
            <circle cx="16" cy="25" r="2.4" fill="currentColor" />
            <circle cx="24" cy="25" r="2.4" fill="currentColor" />
          </svg>
          <b>Shadowrunr OS</b>
          <span>Season 0 · v0.1</span>
        </div>
      </div>
      <div className={styles.hero}>
        <div className={styles.title} aria-hidden="true" data-role="onair-title">
          <span>On</span>
          <span>Air</span>
        </div>
        <div className={styles.screenSlot}>
          <Screen snapshot={feed.snapshot} pills={pills} descoped={descoped} chatRoute={chatRoute} status={state.status} offAir={state.offAir} />
        </div>
        <Progress snap={feed.snapshot} />
        <Transport feed={feed} />
      </div>
      <div className={styles.foot}>
        <p className={styles.credit}>
          A broadcast of the Overlord pilot ·<span>v0.1 · media-less phase: voice, avatar and broadcast come next · stylised illustration</span>
        </p>
        <RouteLink to={pageRoute("onair", "rundown")} className={styles.cta} data-role="push-in">
          <span>Open the rundown</span>
          <i />
        </RouteLink>
      </div>
    </div>
  );
}
