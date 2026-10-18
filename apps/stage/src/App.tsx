import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import demoCaseJson from "../fixtures/demo-case.json";
import demoTranscript from "../fixtures/demo.jsonl?raw";
import type { Case, StreamEvent } from "@shadowrunr/contract";
import { Controls } from "./Controls";
import { CANVAS_H, CANVAS_W, Timeline, buildMap, fitCanvas, initialScene, reduce, type SceneState } from "@shadowrunr/stage-kit";
import { checkCase, parseCase, parseTranscript, type ParseIssue } from "@shadowrunr/stage-kit/parse";
import {
  SPEEDS,
  connectDaemon,
  fetchCase,
  fetchText,
  readParams,
  resolveCase,
  sidecarCaseUrl,
  type CaseSource,
} from "./source";
import { Stage } from "./Stage";

const BUILTIN_CASE: Case = parseCase(demoCaseJson);
/** Keep showing the last frame (the outro) this long before a loop restarts. */
const END_HOLD_MS = 8000;

interface Loaded {
  events: StreamEvent[];
  errors: ParseIssue[];
  warnings: ParseIssue[];
  label: string;
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

/** Outline anything inside the canvas that overflows its box (`?debug=1`). Re-checks every
 * second, so entry animations that have finished are measured at rest. */
function useOverflowAudit(enabled: boolean, root: React.RefObject<HTMLDivElement | null>) {
  useEffect(() => {
    if (!enabled) return;
    const audit = () => {
      const canvas = root.current?.querySelector(".canvas");
      if (!canvas) return;
      const found: string[] = [];
      for (const el of canvas.querySelectorAll<HTMLElement>(".region:not(.hidden) *")) {
        el.classList.remove("overflowing");
        if (el.closest("[data-clip]") || el instanceof SVGElement || el.classList.contains("w")) continue;
        const style = getComputedStyle(el);
        if (style.display === "inline") continue;
        // Intentional truncation (line clamp, ellipsis) is not overflow.
        if (style.webkitLineClamp !== "none" && style.webkitLineClamp !== "") continue;
        if (style.textOverflow === "ellipsis") continue;
        if (el.scrollWidth > el.clientWidth + 1 || el.scrollHeight > el.clientHeight + 1) {
          el.classList.add("overflowing");
          found.push(`${el.className || el.tagName} (${el.scrollWidth}x${el.scrollHeight} > ${el.clientWidth}x${el.clientHeight})`);
        }
      }
      (window as unknown as { __stageOverflow: string[] }).__stageOverflow = found;
      document.body.dataset.overflow = found.length ? found.join(" | ") : "none";
    };
    audit();
    const id = setInterval(audit, 1000);
    return () => clearInterval(id);
  }, [enabled, root]);
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
      if (!timeline) return;
      clock.current = Math.max(timeline.start, Math.min(t, timeline.end + END_HOLD_MS));
      setNow(clock.current);
    },
    [timeline],
  );

  // ── Live (daemon WebSocket) ──────────────────────────────────────────────────
  const liveScene = useRef<SceneState>(initialScene());
  const liveLast = useRef<{ ts: number; wall: number } | null>(null);
  const [, bump] = useState(0);
  useEffect(() => {
    if (!live || !params.run) return;
    const close = connectDaemon(
      params.ws,
      params.run,
      params.speed,
      (ev) => {
        liveScene.current = reduce(liveScene.current, ev);
        liveLast.current = { ts: ev.ts_ms, wall: performance.now() };
        setLoaded((l) => (l ? { ...l, events: [...l.events, ev] } : l));
      },
      (s, d) => setStatus(`daemon ${s}${d ? ` (${d})` : ""}`),
      (d) => setLoaded((l) => (l ? { ...l, errors: [...l.errors, { line: 0, message: d }].slice(-200) } : l)),
    );
    let raf = 0;
    let lastPaint = 0;
    const loop = (t: number) => {
      if (t - lastPaint >= 32) {
        lastPaint = t;
        const ll = liveLast.current;
        if (ll) setNow(ll.ts + Math.min(120_000, (t - ll.wall) * params.speed));
        bump((x) => x + 1);
      }
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => {
      close();
      cancelAnimationFrame(raf);
    };
  }, [live, params]);

  // ── Keyboard ─────────────────────────────────────────────────────────────────
  useEffect(() => {
    if (params.broadcast || live) return;
    const on = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement) return;
      if (e.code === "Space") {
        e.preventDefault();
        setPlaying((p) => !p);
      } else if (e.key === "ArrowRight") seek(clock.current + 5000);
      else if (e.key === "ArrowLeft") seek(clock.current - 5000);
      else if (e.key === "]" || e.key === "[") {
        const i = SPEEDS.indexOf(speedRef.current as (typeof SPEEDS)[number]);
        const next = SPEEDS[Math.max(0, Math.min(SPEEDS.length - 1, (i === -1 ? 2 : i) + (e.key === "]" ? 1 : -1)))]!;
        setSpeed(next);
      }
    };
    window.addEventListener("keydown", on);
    return () => window.removeEventListener("keydown", on);
  }, [params.broadcast, live, seek]);

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
  const events = useMemo(() => loaded?.events ?? [], [loaded]);
  const scene = live ? liveScene.current : timeline ? timeline.advanceTo(now) : initialScene();
  const resolved = useMemo(
    () => resolveCase(explicitCase, events, BUILTIN_CASE, scene.caseFile),
    [explicitCase, events, scene.caseFile],
  );
  const revealKey = scene.revealOrder.join(",");
  const map = useMemo(() => buildMap(resolved.case, revealKey ? revealKey.split(",") : []), [resolved.case, revealKey]);
  useOverflowAudit(params.debug, rootRef);
  if (params.debug) {
    (window as unknown as { __stage: unknown }).__stage = { now, applied: scene.applied, card: scene.card };
  }

  const broadcastHref = useMemo(() => {
    const p = new URLSearchParams(window.location.search);
    p.set("broadcast", "1");
    return `?${p.toString()}`;
  }, []);

  const start = timeline?.start ?? (events[0]?.ts_ms ?? 0);
  const end = timeline?.end ?? (events[events.length - 1]?.ts_ms ?? 0);
  const caseSource: CaseSource = resolved.source;

  return (
    <div className="viewport" ref={rootRef}>
      <div
        className="canvas-frame"
        style={{ width: CANVAS_W, height: CANVAS_H, transform: `translate(${fit.left}px, ${fit.top}px) scale(${fit.scale})` }}
      >
        <Stage
          scene={scene}
          now={now}
          map={map}
          caseFile={resolved.case}
          sourceTag={live ? "LIVE" : "REPLAY"}
          debug={params.debug}
        />
      </div>
      {!params.broadcast && loaded && (
        <Controls
          events={events}
          start={start}
          end={end}
          now={now}
          playing={playing}
          speed={speed}
          live={live}
          sourceLabel={loaded.label}
          caseSource={caseSource}
          errors={loaded.errors}
          warnings={loaded.warnings}
          status={status}
          onPlay={setPlaying}
          onSeek={seek}
          onSpeed={setSpeed}
          onFile={(f) => void onFile(f)}
          broadcastHref={broadcastHref}
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
