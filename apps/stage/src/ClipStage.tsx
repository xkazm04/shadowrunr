// Clip mode: one candidate cut on a vertical 1080x1920 canvas (`?clip=<url>`).
//
// The composition mirrors the clip factory's poster frame (crates/shadowrunr-clips render.rs):
// a header band (ALLEGED + grade badge + case title for claim-bearing clips, else the segment
// and its foil), the hexmap or the avatar in the middle, the exact transcript captions in the
// lower third, the citation strip, and the AI disclosure with the clip's provenance.
//
// `&record=1` freezes the wall clock: `window.__clip.seek(ms)` sets the frame and resolves once
// React has committed it, two frames have painted and every CSS animation is settled at a
// time derived from the clip clock, so `scripts/record-clip.mjs` can step frame by frame.

import { useEffect, useMemo, useRef, useState } from "react";

import { buildMap, foilInfo, GRADE, Timeline } from "@shadowrunr/stage-kit";
import { FoilCard, Hexmap, Silhouette } from "@shadowrunr/stage-kit/ui";
import { captionAt, captionPage, PAGE_CHARS, VIEWER_PAGE_CHARS, CLIP_H, CLIP_W, headerModel, parseClip, readClipParams, shortSource, speakerLabel, type ClipGrade, type ClipInput } from "./clip";
import { fetchText } from "./source";
import "./clip.css";

interface ClipHandle {
  ready: boolean;
  durationMs: number;
  error: string | null;
  seek: (ms: number) => Promise<void>;
}

declare global {
  interface Window {
    __clip?: ClipHandle;
  }
}

function gradeColor(g: ClipGrade): string {
  return g === "UNGRADED" ? "var(--dim)" : GRADE[g].color;
}

const nextFrame = () => new Promise<void>((r) => requestAnimationFrame(() => r()));

/** Settle every CSS animation and transition at a time derived from the clip clock. */
function settleAnimations(ms: number) {
  for (const a of document.getAnimations()) {
    const timing = a.effect?.getComputedTiming();
    if (timing && timing.endTime === Infinity) {
      a.pause();
      a.currentTime = ms;
    } else {
      try {
        a.finish();
      } catch {
        a.pause();
      }
    }
  }
}

function useFit() {
  const [size, setSize] = useState({ w: window.innerWidth, h: window.innerHeight });
  useEffect(() => {
    const on = () => setSize({ w: window.innerWidth, h: window.innerHeight });
    window.addEventListener("resize", on);
    return () => window.removeEventListener("resize", on);
  }, []);
  const scale = Math.min(size.w / CLIP_W, size.h / CLIP_H);
  return { scale, left: (size.w - CLIP_W * scale) / 2, top: (size.h - CLIP_H * scale) / 2 };
}

export function ClipStage({ clip, now }: { clip: ClipInput; now: number }) {
  const timeline = useMemo(() => new Timeline(clip.events), [clip]);
  const scene = timeline.advanceTo(now);
  const revealKey = scene.revealOrder.join(",");
  const map = useMemo(() => buildMap(clip.case, revealKey ? revealKey.split(",") : []), [clip.case, revealKey]);
  const head = headerModel(clip, scene.foil?.id ?? null);
  const cap = captionAt(clip.captions, now);
  const speaking = cap !== null && cap.caption.speaker === "malakor" && cap.words < cap.total;
  const hexmap = clip.case !== null && (clip.segment === "SEG-02A" || clip.header !== null);
  const words = cap ? cap.caption.text.split(/\s+/).filter(Boolean) : [];
  // A long line is shown a page at a time: the page holding the word being spoken, so every
  // word of the exact text appears and nothing overflows the band.
  const page = cap ? captionPage(words, Math.max(0, cap.words - 1), cap.caption.speaker === "viewer" ? VIEWER_PAGE_CHARS : PAGE_CHARS) : { from: 0, to: 0 };

  return (
    <div className={`clip-canvas clip-${clip.style}`} data-clip-id={clip.id}>
      <div className="clip-backdrop" />
      <header className="clip-head">
        {head.kind === "alleged" ? (
          <>
            <div className="clip-alleged-row">
              <span className="clip-alleged">ALLEGED</span>
              <span className="clip-grade" style={{ background: gradeColor(head.grade) }}>
                {head.grade}
              </span>
            </div>
            <div className="clip-case-title">{head.caseTitle}</div>
            {head.fictional && <span className="clip-fiction">FICTIONAL DEMO CASE</span>}
          </>
        ) : (
          <>
            <div className="clip-seg-title">{head.title}</div>
            {head.foil && <div className="clip-seg-foil">with {head.foil}</div>}
          </>
        )}
      </header>

      <section className={`clip-middle${hexmap ? " clip-middle-map" : ""}`}>
        {hexmap ? (
          <Hexmap map={map} scene={scene} now={now} caseFile={clip.case} />
        ) : (
          <div className="clip-avatar">
            <div className="clip-sil">
              <Silhouette speaking={speaking} />
            </div>
            {scene.foil && (
              <div className="clip-foil" style={{ ["--foil" as string]: foilInfo(scene.foil.id).color }}>
                <FoilCard scene={scene} now={now} />
              </div>
            )}
          </div>
        )}
      </section>

      <section className="clip-captions" aria-live="off">
        {cap && (
          <div className={`clip-caption clip-speaker-${cap.caption.speaker === "malakor" ? "host" : cap.caption.speaker === "viewer" ? "viewer" : "foil"}`} key={`${cap.caption.start_ms}`}>
            <div className="clip-caption-who">{speakerLabel(cap.caption.speaker)}</div>
            <p className="clip-caption-text">
              {words.slice(page.from, page.to).map((w, k) => (
                <span key={page.from + k} className={page.from + k < cap.words ? "w on" : "w"}>
                  {w}{" "}
                </span>
              ))}
            </p>
          </div>
        )}
      </section>

      {clip.header && (
        <section className="clip-cite">
          {clip.header.citation.length > 0 ? (
            <>
              <div className="clip-cite-head">SOURCES · grade {clip.header.grade}</div>
              {clip.header.citation.slice(0, 3).map((c) => (
                <div key={c} className="clip-cite-src">
                  {shortSource(c)}
                </div>
              ))}
            </>
          ) : (
            <div className="clip-cite-src">No hex claim in this cut · case shown by title</div>
          )}
        </section>
      )}

      <footer className="clip-foot">
        <span className="clip-disclosure">{clip.disclosure}</span>
        <span className="clip-prov">{clip.provenance}</span>
      </footer>
    </div>
  );
}

/** The clip-mode app: loads `?clip=`, then previews (looping) or waits for `seek` (record). */
export function ClipApp() {
  const params = useMemo(() => readClipParams(window.location.search), []);
  const [clip, setClip] = useState<ClipInput | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(0);
  const fit = useFit();
  const pending = useRef<{ ms: number; resolve: () => void }[]>([]);
  const nowRef = useRef(0);
  nowRef.current = now;

  useEffect(() => {
    if (!params.clip) return;
    fetchText(params.clip)
      .then((t) => {
        const r = parseClip(t);
        if (r.ok) setClip(r.clip);
        else setError(r.message);
      })
      .catch((e: Error) => setError(e.message));
  }, [params.clip]);

  // The recording handle.
  useEffect(() => {
    const handle: ClipHandle = {
      ready: false,
      durationMs: clip?.duration_ms ?? 0,
      error,
      seek: (ms: number) =>
        new Promise<void>((resolve) => {
          if (ms === nowRef.current) {
            // Same frame: nothing to commit, only to settle.
            void nextFrame().then(() => {
              settleAnimations(ms);
              return nextFrame().then(resolve);
            });
            return;
          }
          pending.current.push({ ms, resolve });
          setNow(ms);
        }),
    };
    window.__clip = handle;
    if (clip) {
      void document.fonts.ready.then(async () => {
        await nextFrame();
        await nextFrame();
        handle.ready = true;
      });
    }
  }, [clip, error]);

  // Resolve seeks once their frame is committed, painted twice and settled.
  useEffect(() => {
    const waiting = pending.current.filter((p) => p.ms === now);
    if (waiting.length === 0) return;
    pending.current = pending.current.filter((p) => p.ms !== now);
    void (async () => {
      await nextFrame();
      settleAnimations(now);
      await nextFrame();
      waiting.forEach((p) => p.resolve());
    })();
  }, [now]);

  // Preview: play and loop on the wall clock.
  useEffect(() => {
    if (!clip || params.record) return;
    let raf = 0;
    const t0 = performance.now();
    const loop = (t: number) => {
      setNow(Math.max(0, t - t0) % (clip.duration_ms + 1500));
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [clip, params.record]);

  return (
    <div className="viewport clip-viewport">
      {clip ? (
        <div
          className="clip-frame"
          style={{ width: CLIP_W, height: CLIP_H, transform: `translate(${fit.left}px, ${fit.top}px) scale(${fit.scale})` }}
        >
          <ClipStage clip={clip} now={Math.min(now, clip.duration_ms)} />
        </div>
      ) : (
        <div className="clip-status">{error ? `clip did not load: ${error}` : "loading clip…"}</div>
      )}
    </div>
  );
}
