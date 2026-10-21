import { useEffect, useMemo, useRef, useState, type ChangeEvent, type PointerEvent } from "react";

import type { StreamEvent } from "@shadowrunr/contract";
import type { ParseIssue } from "@shadowrunr/stage-kit/parse";
import { clipMarkers, segmentSpans } from "@shadowrunr/stage-kit";
import { SPEEDS, type CaseSource } from "./source";
import { VOICE_STATE_LABEL, VOICE_TOGGLE } from "./voice/names";
import type { VoiceStatus } from "./voice/session";

function fmt(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

const CASE_LABEL: Record<CaseSource, string> = {
  transcript: "case from the transcript (case_open)",
  param: "case from ?case=",
  sidecar: "case from sidecar .case.json",
  dropped: "case from dropped file",
  builtin: "built-in fictional demo case",
  none: "no case file: hexes drawn from ids",
};

/** The live preview's DVR in the controls: the window it can reach back to, how far the preview
 * trails the live clock, and the way back to the head. */
export interface DvrControls {
  windowStart: number | null;
  /** The preview shows a past moment (paused or playing back), not the head. */
  behind: boolean;
  behindMs: number;
  onGoLive: () => void;
}

/** The playback keys (space, arrows, [ ]): the stage's one raw keydown listener for them.
 * Controls.tsx is the stage's declared keyboard file (eslint.config.js), so App's handler is
 * attached here while `enabled` (not broadcast; live, they drive the preview's DVR). */
export function usePlaybackKeys(enabled: boolean, onKey: (e: KeyboardEvent) => void): void {
  useEffect(() => {
    if (!enabled) return;
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [enabled, onKey]);
}

export function Controls({
  events,
  start,
  end,
  now,
  playing,
  speed,
  live,
  sourceLabel,
  caseSource,
  errors,
  warnings,
  status,
  onPlay,
  onSeek,
  onSpeed,
  onFile,
  broadcastHref,
  voiceOn,
  voiceStatus,
  onVoice,
  dvr = null,
}: {
  events: readonly StreamEvent[];
  start: number;
  end: number;
  now: number;
  playing: boolean;
  speed: number;
  live: boolean;
  sourceLabel: string;
  caseSource: CaseSource;
  errors: ParseIssue[];
  warnings: ParseIssue[];
  status: string | null;
  onPlay: (p: boolean) => void;
  onSeek: (t: number) => void;
  onSpeed: (s: number) => void;
  onFile: (f: File) => void;
  broadcastHref: string;
  /** The Voice toggle (off by default, remembered per browser). */
  voiceOn: boolean;
  voiceStatus: VoiceStatus;
  onVoice: (on: boolean) => void;
  /** Live preview only: the DVR over the held window (null for a replay). */
  dvr?: DvrControls | null;
}) {
  const [idle, setIdle] = useState(false);
  const [showIssues, setShowIssues] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const track = useRef<HTMLDivElement | null>(null);
  const span = Math.max(1, end - start);
  const markers = useMemo(() => clipMarkers(events), [events]);
  const spans = useMemo(() => segmentSpans(events), [events]);
  // live without a DVR the transport is inert (the head is all there is)
  const inert = live && dvr === null;
  const behind = dvr?.behind ?? false;
  const reach = dvr?.windowStart ?? start;

  useEffect(() => {
    const wake = () => {
      setIdle(false);
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => setIdle(true), 2800);
    };
    wake();
    window.addEventListener("pointermove", wake);
    window.addEventListener("keydown", wake);
    return () => {
      window.removeEventListener("pointermove", wake);
      window.removeEventListener("keydown", wake);
      if (timer.current) clearTimeout(timer.current);
    };
  }, []);

  const seekFromPointer = (e: PointerEvent<HTMLDivElement>) => {
    const el = track.current;
    if (!el || inert) return;
    const r = el.getBoundingClientRect();
    const frac = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
    onSeek(start + frac * span);
  };

  const pick = (e: ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0];
    if (f) onFile(f);
    e.target.value = "";
  };

  const issues = errors.length + warnings.length;
  return (
    <div className={`controls${idle && playing && !showIssues ? " idle" : ""}`}>
      <div className="controls-row">
        <button className="ctl-btn" onClick={() => onPlay(!playing)} disabled={inert} title="Space">
          {playing ? "❚❚" : "▶"}
        </button>
        <button className="ctl-btn" onClick={() => onSeek(reach)} disabled={inert} title={live ? "Back to the window's start" : "Restart"}>
          ⟲
        </button>
        <span className="ctl-time">
          {fmt(now - start)} / {live ? "live" : fmt(span)}
        </span>
        {dvr && (
          <span className="ctl-dvr">
            {behind && <span className="ctl-behind">{Math.round(dvr.behindMs / 1000)} s behind live</span>}
            <button className={`ctl-golive${behind ? "" : " on"}`} onClick={dvr.onGoLive} disabled={!behind} title="Back to the live head">
              {behind ? "GO LIVE" : "● LIVE"}
            </button>
          </span>
        )}
        <div
          className="ctl-track"
          ref={track}
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId);
            seekFromPointer(e);
          }}
          onPointerMove={(e) => {
            if (e.buttons === 1) seekFromPointer(e);
          }}
        >
          {dvr && reach > start && (
            <span className="ctl-outside" style={{ width: `${Math.min(100, ((reach - start) / span) * 100)}%` }} title="no longer held: before the DVR window" />
          )}
          {spans.map((s) => (
            <span
              key={`${s.segment}-${s.from}`}
              className={`ctl-span seg-${s.segment}`}
              style={{ left: `${((s.from - start) / span) * 100}%`, width: `${((s.to - s.from) / span) * 100}%` }}
              title={s.segment}
            />
          ))}
          <span className="ctl-progress" style={{ width: `${Math.min(100, ((now - start) / span) * 100)}%` }} />
          {markers.map((m) => (
            <button
              key={m.at}
              className="ctl-marker"
              style={{ left: `${((m.at - start) / span) * 100}%` }}
              title={`clip: ${m.label}`}
              onPointerDown={(e) => {
                e.stopPropagation();
                onSeek(Math.max(reach, m.at - 3000));
              }}
            />
          ))}
        </div>
        <div className="ctl-speeds">
          {SPEEDS.map((s) => (
            <button key={s} className={`ctl-speed${s === speed ? " on" : ""}`} onClick={() => onSpeed(s)} disabled={inert}>
              {s}×
            </button>
          ))}
        </div>
      </div>
      <div className="controls-row small">
        <label className="ctl-file">
          open transcript…
          <input type="file" accept=".jsonl,.json,.txt" onChange={pick} />
        </label>
        <span className="ctl-source" title={sourceLabel}>
          {sourceLabel}
        </span>
        <span className="ctl-case">{CASE_LABEL[caseSource]}</span>
        {status && <span className="ctl-status">{status}</span>}
        {issues > 0 && (
          <button className={`ctl-issues${errors.length ? " bad" : ""}`} onClick={() => setShowIssues(!showIssues)}>
            {errors.length} skipped · {warnings.length} warnings
          </button>
        )}
        <button
          className={`ctl-speed ctl-voice${voiceOn ? " on" : ""}`}
          aria-pressed={voiceOn}
          onClick={() => onVoice(!voiceOn)}
          title="Malakor's synthetic voice (live runs only)"
        >
          {VOICE_TOGGLE}
        </button>
        <span className={`ctl-voice-state state-${voiceStatus.state}`} title={voiceStatus.reason ?? ""}>
          {VOICE_STATE_LABEL[voiceStatus.state]}
          {voiceStatus.reason ? ` · ${voiceStatus.reason}` : ""}
        </span>
        <span className="ctl-hint">drop a .jsonl anywhere · space play/pause · ← → seek · [ ] speed</span>
        <a className="ctl-broadcast" href={broadcastHref} target="_blank" rel="noreferrer">
          broadcast view ↗
        </a>
      </div>
      {showIssues && (
        <div className="ctl-issue-list">
          {[...errors.map((e) => ({ ...e, kind: "error" })), ...warnings.map((w) => ({ ...w, kind: "warning" }))]
            .slice(0, 60)
            .map((i, k) => (
              <div key={k} className={`issue ${i.kind}`}>
                line {i.line}: {i.message}
              </div>
            ))}
        </div>
      )}
    </div>
  );
}
