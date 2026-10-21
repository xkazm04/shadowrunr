import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";

import demoCaseJson from "../fixtures/demo-case.json";
import demoTranscript from "../fixtures/demo.jsonl?raw";
import type { Case, StreamEvent } from "@shadowrunr/contract";
import { Controls, usePlaybackKeys } from "./Controls";
import { CANVAS_H, CANVAS_W, Timeline, buildMap, fitCanvas, initialScene, type SceneState } from "@shadowrunr/stage-kit";
import { checkCase, parseCase, parseTranscript, type ParseIssue } from "@shadowrunr/stage-kit/parse";
import {
  SPEEDS,
  fetchCase,
  fetchText,
  readParams,
  resolveCase,
  sidecarCaseUrl,
  type CaseSource,
} from "./source";
import { LiveSession, type LiveStore } from "./live";
import { FOLLOW, dvrFor, dvrReduce, liveView, type DvrAction, type DvrWindow } from "./dvr";
import type { FaceSource } from "./avatar/types";
import { rigBaseUrl, voiceUrl } from "./voice/urls";
import { pageStorage, readVoiceOn, voiceBlocked, writeVoiceOn } from "./voice/toggle";
import { FaceMixer, useVoice } from "./voice/useVoice";
import { unlockAudio } from "./voice/webaudio";
import { SignalLostSlate, StageErrorBoundary, StalledMark, fmtClock, streamElapsed } from "./Slates";
import { Stage } from "./Stage";
import { useOverflowAudit } from "./overflowAudit";

const BUILTIN_CASE: Case = parseCase(demoCaseJson);
const EMPTY_SCENE: SceneState = initialScene();
const NO_EVENTS: StreamEvent[] = [];
/** Keep showing the last frame (the outro) this long before a loop restarts. */
const END_HOLD_MS = 8000;

interface Loaded {
  events: StreamEvent[];
  errors: ParseIssue[];
  warnings: ParseIssue[];
  label: string;
}

/** The live store's DVR window: its earliest reachable moment and the head (null before any event). */
function windowOf(store: LiveStore | undefined): DvrWindow | null {
  const start = store?.windowStart() ?? null;
  const head = store?.scene.lastTs ?? null;
  return start === null || head === null ? null : { start, head };
}

function loadText(text: string, label: string): Loaded {
  return { ...parseTranscript(text), label };
}

function useViewport() {
  const [size, setSize] = useState({ w: window.innerWidth, h: window.innerHeight });
  useEffect(() => {
    const on = () => setSize({ w: window.innerWidth, h: window.innerHeight });
    window.addEventListener("resize", on);
    return () => window.removeEventListener("resize", on);
  }, []);
  return fitCanvas(size.w, size.h);
}

export function App() {
  const params = useMemo(() => readParams(window.location.search), []);
  const live = params.run !== null;
  const [loaded, setLoaded] = useState<Loaded | null>(live ? { events: [], errors: [], warnings: [], label: `daemon ${params.ws} · ${params.run}` } : null);
  const [explicitCase, setExplicitCase] = useState<{ case: Case; source: "param" | "sidecar" | "dropped" } | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [speed, setSpeed] = useState(params.speed);
  const [playing, setPlaying] = useState(true);
  const [now, setNow] = useState(0);
  const [dragging, setDragging] = useState(false);
  const [dvrState, dispatchDvr] = useReducer(dvrReduce, FOLLOW);
  // the operator preview of a live run looks back; the broadcast view has no DVR at all
  const dvr = dvrFor({ live, broadcast: params.broadcast }) === null ? null : dvrState;
  const fit = useViewport();
  const rootRef = useRef<HTMLDivElement | null>(null);

  // ── Initial source ───────────────────────────────────────────────────────────
  useEffect(() => {
    if (live) return;
    if (params.file) {
      setStatus(`loading ${params.file}…`);
      fetchText(params.file)
        .then((t) => {
          setLoaded(loadText(t, params.file!));
          setStatus(null);
        })
        .catch((e: Error) => {
          setStatus(`could not load ${params.file}: ${e.message}; showing the built-in demo`);
          setLoaded(loadText(demoTranscript, "fixtures/demo.jsonl (built-in)"));
        });
    } else {
      setLoaded(loadText(demoTranscript, "fixtures/demo.jsonl (built-in)"));
    }
    const caseUrl = params.caseUrl ?? (params.file ? sidecarCaseUrl(params.file) : null);
    if (caseUrl) {
      void fetchCase(caseUrl).then((c) => {
        if (c) setExplicitCase({ case: c, source: params.caseUrl ? "param" : "sidecar" });
        else if (params.caseUrl) setStatus(`case ${params.caseUrl} did not load`);
      });
    }
  }, [live, params]);

  // ── File playback ────────────────────────────────────────────────────────────
  const timeline = useMemo(() => (loaded && !live ? new Timeline(loaded.events) : null), [loaded, live]);
  const clock = useRef(0);
  const firstTimeline = useRef<Timeline | null>(null);
  const playingRef = useRef(playing);
  const speedRef = useRef(speed);
  const liveRef = useRef<LiveSession | null>(null);
  playingRef.current = playing;
  speedRef.current = speed;

  useEffect(() => {
    if (!timeline) return;
    // `?t=` applies to the first transcript only, not to one dropped in later.
    firstTimeline.current ??= timeline;
    const t0 = timeline.start + (firstTimeline.current === timeline ? params.startAt * 1000 : 0);
    clock.current = Math.min(t0, timeline.end + END_HOLD_MS);
    setNow(clock.current);
    setPlaying(!params.paused);
  }, [timeline, params]);

  useEffect(() => {
    if (!timeline) return;
    let raf = 0;
    let last = performance.now();
    let lastPaint = 0;
    const loop = (t: number) => {
      // rAF timestamps can precede the performance.now() taken at setup: never run backwards.
      const dt = Math.max(0, Math.min(250, t - last));
      last = t;
      if (playingRef.current) {
        clock.current += dt * speedRef.current;
        if (clock.current > timeline.end + END_HOLD_MS) {
          if (params.loop) clock.current = timeline.start;
          else {
            clock.current = timeline.end + END_HOLD_MS;
            setPlaying(false);
          }
        }
      }
      if (t - lastPaint >= 32) {
        lastPaint = t;
        setNow(clock.current);
      }
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [timeline, params.loop]);

  const seek = useCallback(
    (t: number) => {
      if (live) {
        const win = windowOf(liveRef.current?.store);
        if (win) dispatchDvr({ type: "seek", t, win });
        return;
      }
      if (!timeline) return;
      clock.current = Math.max(timeline.start, Math.min(t, timeline.end + END_HOLD_MS));
      setNow(clock.current);
    },
    [timeline, live],
  );

  // ── Live (daemon WebSocket) ──────────────────────────────────────────────────
  /** The DVR's play/pause and relative steps: the window is read when the action happens. */
  const dvrAct = useCallback((act: (win: DvrWindow) => DvrAction) => {
    const win = windowOf(liveRef.current?.store);
    if (win) dispatchDvr(act(win));
  }, []);
  const [, bump] = useState(0);
  useEffect(() => {
    if (!live || !params.run) return;
    const session = new LiveSession({
      url: params.ws,
      run: params.run,
      speed: params.speed,
      now: () => performance.now(),
      onStatus: (s, d) => setStatus(`daemon ${s}${d ? ` (${d})` : ""}`),
      onBadMessage: (d) => setLoaded((l) => (l ? { ...l, errors: [...l.errors, { line: 0, message: d }].slice(-200) } : l)),
      onErrorFrame: (code, m) => setStatus(`daemon ${code}: ${m}`),
    });
    liveRef.current = session;
    session.start();
    // the operator preview's DVR clock runs here too; the broadcast view has none to run
    const dvrOn = dvrFor({ live, broadcast: params.broadcast }) !== null;
    let raf = 0;
    let lastPaint = 0;
    const loop = (t: number) => {
      if (t - lastPaint >= 32) {
        const dt = lastPaint === 0 ? 0 : Math.min(250, t - lastPaint);
        lastPaint = t;
        const win = dvrOn ? windowOf(session.store) : null;
        if (win) dispatchDvr({ type: "tick", dt, speed: speedRef.current, win });
        const ll = session.store.lastEvent;
        if (ll) setNow(ll.ts + Math.min(120_000, (t - ll.wall) * params.speed));
        bump((x) => x + 1);
      }
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => {
      session.stop();
      liveRef.current = null;
      cancelAnimationFrame(raf);
    };
  }, [live, params]);

  // ── Voice and face ───────────────────────────────────────────────────────────
  const storage = useMemo(pageStorage, []);
  const [voiceOn, setVoiceOn] = useState(() => params.voice || readVoiceOn(storage));
  const headless = typeof navigator !== "undefined" && navigator.webdriver === true;
  const blocked = voiceBlocked({ live, on: voiceOn, headless });
  const mixer = useMemo(() => new FaceMixer(), []);
  const [demoFace, setDemoFace] = useState<FaceSource | null>(null);
  useEffect(() => {
    const mode = params.face;
    if (!import.meta.env.DEV || !mode) return;
    void import("./avatar/demo").then((m) => {
      if (m.isDemoMode(mode)) setDemoFace(m.demoSource(mode));
    });
  }, [params.face]);
  const followRun = live ? liveRef.current?.store.runId || (params.run === "live" ? null : params.run) : null;
  const voice = useVoice({ blocked, url: useMemo(() => voiceUrl(params.ws), [params.ws]), runId: followRun, mixer, broadcast: params.broadcast });
  const voiceShown = blocked === null && voice.status.state === "ready";
  // the daemon serves the rig to replays too; without a daemon the face falls back to the placeholder
  const rigBase = useMemo(() => params.rig ?? rigBaseUrl(params.ws), [params.rig, params.ws]);
  const toggleVoice = useCallback(
    (on: boolean) => {
      // the click is the gesture the browser wants before any audio may sound
      if (on) unlockAudio();
      setVoiceOn(on);
      writeVoiceOn(storage, on);
    },
    [storage],
  );
  const captionSpeaking = useCallback((s: boolean) => mixer.setCaption(s), [mixer]);

  // ── Keyboard ─────────────────────────────────────────────────────────────────
  // The listener itself lives in Controls.tsx, the stage's one keyboard file.
  const onPlaybackKey = useCallback(
    (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement) return;
      if (e.code === "Space") {
        e.preventDefault();
        if (live) dvrAct((win) => ({ type: "toggle", win }));
        else setPlaying((p) => !p);
      } else if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
        const by = e.key === "ArrowRight" ? 5000 : -5000;
        if (live) dvrAct((win) => ({ type: "step", by, win }));
        else seek(clock.current + by);
      } else if (e.key === "]" || e.key === "[") {
        const i = SPEEDS.indexOf(speedRef.current as (typeof SPEEDS)[number]);
        const next = SPEEDS[Math.max(0, Math.min(SPEEDS.length - 1, (i === -1 ? 2 : i) + (e.key === "]" ? 1 : -1)))]!;
        setSpeed(next);
      }
    },
    [seek, live, dvrAct],
  );
  // live, the keys drive the preview's DVR; the broadcast view has no keys
  usePlaybackKeys(!params.broadcast, onPlaybackKey);

  // ── Drag and drop ────────────────────────────────────────────────────────────
  const onFile = useCallback(async (f: File) => {
    const text = await f.text();
    if (/\.json$/i.test(f.name)) {
      const c = checkCase(text);
      if (c.ok) {
        setExplicitCase({ case: c.value, source: "dropped" });
        setStatus(`case loaded: ${c.value.title}`);
        return;
      }
      // Not a case: fall through and try it as a transcript.
    }
    setLoaded(loadText(text, f.name));
    setStatus(null);
  }, []);

  useEffect(() => {
    if (live) return;
    const over = (e: DragEvent) => {
      e.preventDefault();
      setDragging(true);
    };
    const leave = (e: DragEvent) => {
      if (e.relatedTarget === null) setDragging(false);
    };
    const drop = (e: DragEvent) => {
      e.preventDefault();
      setDragging(false);
      const files = [...(e.dataTransfer?.files ?? [])];
      // A case dropped together with a transcript applies to it.
      files.sort((a) => (/\.json$/i.test(a.name) ? -1 : 1)).forEach((f) => void onFile(f));
    };
    window.addEventListener("dragover", over);
    window.addEventListener("dragleave", leave);
    window.addEventListener("drop", drop);
    return () => {
      window.removeEventListener("dragover", over);
      window.removeEventListener("dragleave", leave);
      window.removeEventListener("drop", drop);
    };
  }, [live, onFile]);

  // ── Scene ────────────────────────────────────────────────────────────────────
  const store = liveRef.current?.store ?? null;
  // Live: the debug views read the bounded ring (copied once per change, and not at all in the
  // broadcast view, which has none).
  const events = useMemo(() => loaded?.events ?? [], [loaded]);
  const liveEvents = store && !params.broadcast ? store.events() : NO_EVENTS;
  const shown = live ? liveEvents : events;
  // Live: the head, or the DVR's past moment in the operator preview (`dvr` is null in broadcast).
  const view = live && store ? liveView(store, dvr, now) : null;
  const head = store?.scene ?? EMPTY_SCENE;
  const scene = live ? (view?.scene ?? EMPTY_SCENE) : timeline ? timeline.advanceTo(now) : EMPTY_SCENE;
  const at = view?.now ?? now;
  const revealKey = scene.revealOrder.join(",");
  const resolved = useMemo(
    () => resolveCase(explicitCase, shown, BUILTIN_CASE, scene.caseFile, live ? (revealKey ? revealKey.split(",") : []) : undefined),
    [explicitCase, shown, scene.caseFile, live, revealKey],
  );
  const map = useMemo(() => buildMap(resolved.case, revealKey ? revealKey.split(",") : []), [resolved.case, revealKey]);
  useOverflowAudit(params.debug, rootRef);
  if (params.debug) {
    window.__stage = { now: at, applied: scene.applied, card: scene.card };
  }
  const watch = store ? store.watch(performance.now()) : null;
  // with the voice sounding, captions follow its audio clock (voice/captions.ts); a past moment in
  // the DVR shows the estimated timing, since the voice is live audio
  const followsVoice = view?.followsVoice ?? true;
  const voiceCaption = followsVoice && voice.followsAudio && store?.runId ? voice.caption(store.runId, store.said, now) : undefined;
  const elapsed = live ? streamElapsed(scene, at) : null;
  const overlay = watch && (
    <>
      {watch.disconnected && <SignalLostSlate />}
      {!watch.disconnected && watch.stalled && <StalledMark ms={watch.stalledMs} />}
    </>
  );

  const broadcastHref = useMemo(() => {
    const p = new URLSearchParams(window.location.search);
    p.set("broadcast", "1");
    return `?${p.toString()}`;
  }, []);

  // live, the track spans the whole stream so far (the head's), and the DVR reaches back to windowStart
  const start = timeline?.start ?? (live ? (head.meta?.at ?? head.t0 ?? 0) : (events[0]?.ts_ms ?? 0));
  const end = timeline?.end ?? (live ? (head.lastTs ?? 0) : (events[events.length - 1]?.ts_ms ?? 0));
  const windowStart = dvr && store ? store.windowStart() : null;
  const dvrPlaying = dvr === null || dvr.mode === "follow" || dvr.playing;
  const caseSource: CaseSource = resolved.source;

  return (
    <div className="viewport" ref={rootRef}>
      <div
        className="canvas-frame"
        style={{ width: CANVAS_W, height: CANVAS_H, transform: `translate(${fit.left}px, ${fit.top}px) scale(${fit.scale})` }}
      >
        <StageErrorBoundary voice={voiceShown}>
          <Stage
            scene={scene}
            now={at}
            map={map}
            caseFile={resolved.case}
            sourceTag={live ? "LIVE" : "REPLAY"}
            debug={params.debug}
            broadcast={params.broadcast}
            voice={voiceShown}
            {...(elapsed === null ? {} : { clock: fmtClock(elapsed) })}
            overlay={overlay}
            face={demoFace ?? mixer}
            rigBase={rigBase}
            variant={params.avatar}
            onCaptionSpeaking={captionSpeaking}
            {...(voiceCaption === undefined ? {} : { voiceCaption })}
            onFace={voice.face}
          />
        </StageErrorBoundary>
      </div>
      {!params.broadcast && loaded && (
        <Controls
          events={shown}
          start={start}
          end={end}
          now={at}
          playing={live ? dvrPlaying : playing}
          speed={speed}
          live={live}
          sourceLabel={loaded.label}
          caseSource={caseSource}
          errors={loaded.errors}
          warnings={loaded.warnings}
          status={status}
          onPlay={live ? (p) => dvrAct((win) => ({ type: p ? "play" : "pause", win })) : setPlaying}
          onSeek={seek}
          onSpeed={setSpeed}
          onFile={(f) => void onFile(f)}
          broadcastHref={broadcastHref}
          voiceOn={voiceOn}
          voiceStatus={voice.status}
          onVoice={toggleVoice}
          dvr={dvr === null ? null : { windowStart, behind: dvr.mode === "behind", behindMs: view?.behindMs ?? 0, onGoLive: () => dispatchDvr({ type: "goLive" }) }}
        />
      )}
      {dragging && !params.broadcast && (
        <div className="drop-overlay">
          <div>Drop a transcript.jsonl (and optionally its case .json)</div>
        </div>
      )}
    </div>
  );
}
