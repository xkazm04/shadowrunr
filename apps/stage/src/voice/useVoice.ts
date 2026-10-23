// The stage's voice, as React sees it: the face mixer (voice, else the caption, else rest) and the
// hook that runs a `VoiceSession` while the voice is allowed, with the captions that follow its
// audio. The session, the player and the frame guard are imported with `import()` when the voice
// is first allowed, so a stage whose voice is off pays nothing for them.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { RESTING, type FaceSignal, type FaceSource } from "../avatar/types";
import { VoiceCaptions, type SaidLine, type VoiceCaptionView } from "./captions";
import type { VoiceHealth } from "./health";
import { VOICE_ACTION_LABEL, type VoiceAction } from "./names";
import { playoutOf, type PlayoutClock } from "./playout";
import type { ScheduledPart } from "./player";
import type { FaceState } from "./report";
import type { VoiceSession, VoiceStatus } from "./session";
import { audioRunning, sharedAudio, unlockAudio } from "./webaudio";

/**
 * The face's one source on the stage. While the voice sounds, its signal (the audio clock's mouth
 * and loudness) drives the face; otherwise a `say` caption on screen gives the level-less speaking
 * phase (the head sways as he talks, the mouth stays shut); otherwise the face rests.
 */
export class FaceMixer implements FaceSource {
  private voice: FaceSignal = RESTING;
  private caption = false;
  private captionGen = 0;
  private offVoice: (() => void) | null = null;
  private listeners = new Set<(s: FaceSignal) => void>();
  private last: FaceSignal = RESTING;

  /** Read the voice from `source` (null: no voice). */
  setVoice(source: FaceSource | null): void {
    this.offVoice?.();
    this.offVoice = null;
    this.voice = RESTING;
    if (source) {
      this.offVoice = source.subscribe((s) => {
        this.voice = s;
        this.emit();
      });
    }
    this.emit();
  }

  /** A `say` caption is (or is no longer) on screen. */
  setCaption(speaking: boolean): void {
    if (speaking === this.caption) return;
    this.caption = speaking;
    if (speaking) this.captionGen++;
    this.emit();
  }

  current(): FaceSignal {
    if (this.voice.phase === "speaking") return this.voice;
    if (this.caption) return { phase: "speaking", level: 0, viseme: null, open: 0, generation: -this.captionGen };
    return RESTING;
  }

  subscribe(listener: (signal: FaceSignal) => void): () => void {
    this.listeners.add(listener);
    listener(this.last);
    return () => this.listeners.delete(listener);
  }

  private emit() {
    const s = this.current();
    if (s === this.last) return;
    this.last = s;
    for (const l of this.listeners) l(s);
  }
}

export interface VoiceView {
  status: VoiceStatus;
  /** Audio is sounding or queued right now. */
  playing: boolean;
  /** The voice is ready and its audio runs: captions follow the audio (`caption`). */
  followsAudio: boolean;
  /** The caption the audio clock gives the run's said lines now (`streamNow` in stream ms). */
  caption(runId: string, said: readonly SaidLine[], streamNow: number): VoiceCaptionView | null;
  /** The face drawn, for the daemon (sent while the voice runs, and again when it starts). */
  face(state: FaceState): void;
  /** What went wrong for the followed run, read now (null while no session runs). */
  health(): VoiceHealth | null;
  /** Do what the status's `action` offers; called inside the operator's click. */
  act(action: VoiceAction): void;
}

/**
 * The captions on heard time: the caption shown and the moment a line is known unvoiced are both
 * read off the playout clock, the same time the face and the `played` report use.
 */
export function heardCaptions(captions: VoiceCaptions, playout: PlayoutClock) {
  return {
    unvoiced: (run: string, seq: number) => captions.unvoiced(run, seq, playout.heardNow()),
    view: (run: string, said: readonly SaidLine[], streamNow: number) => captions.view(run, said, playout.heardNow(), streamNow),
  };
}

/**
 * The status the stage shows for a session's: a ready voice whose audio does not run is waiting
 * for the operator's Enable audio press in a browser, and only waiting in the broadcast view, which
 * never asks for one. In the operator view a socket waiting out its backoff (`reconnecting`) offers
 * Reconnect now. The broadcast view is never offered an action: it has no one to press it.
 */
export function shownStatus(status: VoiceStatus, running: boolean, broadcast: boolean, reconnecting = false): VoiceStatus {
  if (status.state === "ready" && !running) {
    if (broadcast) return { state: "starting", reason: "waiting for the audio to start" };
    return { state: "degraded", reason: `audio is blocked until you press ${VOICE_ACTION_LABEL.unlock}`, action: "unlock" };
  }
  if (reconnecting && !broadcast && status.state !== "ready") return { ...status, action: "retry" };
  return status;
}

/**
 * Do what the voice chip offers. Unlock resumes the page's AudioContext inside the press (the
 * gesture a browser wants) and leaves the Voice toggle as it is; retry opens the socket now.
 */
export function runVoiceAction(action: VoiceAction, session: { retryNow(): void } | null): void {
  if (action === "unlock") unlockAudio();
  else session?.retryNow();
}

/**
 * Run the voice while `blocked` is null: connect to `url`, follow `runId`, feed the mixer. Returns
 * the state for the controls; `blocked` (why it is off) shows as `off` with that reason. In the
 * broadcast view the audio is resumed unasked, at once and on every utterance (see webaudio.ts).
 */
export function useVoice(opts: {
  blocked: string | null;
  url: string;
  runId: string | null;
  mixer: FaceMixer;
  broadcast: boolean;
  /** Hears each part as its audio starts (the face's performance cue is set on a line's part 0).
   *  Keep it stable: a new one restarts the session. */
  onStarted?: (p: ScheduledPart) => void;
}): VoiceView {
  const { blocked, url, runId, mixer, broadcast, onStarted } = opts;
  const [status, setStatus] = useState<VoiceStatus>({ state: "off" });
  const session = useRef<VoiceSession | null>(null);
  // the running session as render sees it (the ref is for callbacks; render reads this)
  const [liveSession, setLiveSession] = useState<VoiceSession | null>(null);
  const runIdRef = useRef(runId);
  runIdRef.current = runId;
  const captions = useMemo(() => new VoiceCaptions(), []);
  const faceRef = useRef<FaceState | null>(null);

  useEffect(() => {
    if (blocked !== null) return;
    const audio = sharedAudio();
    if (!audio) {
      setStatus({ state: "degraded", reason: "this browser has no Web Audio" });
      return;
    }
    if (broadcast) unlockAudio();
    let cancelled = false;
    setStatus({ state: "starting", reason: "loading the voice" });
    import("./session").then(
      ({ VoiceSession }) => {
        if (cancelled) return;
        const playout = playoutOf(audio.audio);
        const heard = heardCaptions(captions, playout);
        const s = new VoiceSession({
          url,
          audio: audio.audio,
          playout,
          onStatus: setStatus,
          ...(broadcast ? { onUtterance: unlockAudio } : {}),
          onSkipped: heard.unvoiced,
          events: {
            scheduled: (p) => captions.scheduled(p),
            started: (p) => onStarted?.(p),
            dropped: heard.unvoiced,
          },
        });
        session.current = s;
        setLiveSession(s);
        s.follow(runIdRef.current);
        if (faceRef.current) s.face(faceRef.current);
        s.start();
        mixer.setVoice(s.player);
      },
      (error: unknown) => setStatus({ state: "degraded", reason: `the voice did not load (${String(error)})` }),
    );
    return () => {
      cancelled = true;
      mixer.setVoice(null);
      session.current?.stop();
      session.current = null;
      setLiveSession(null);
    };
    // the run is followed by the effect below (a run change is a follow frame, not a reconnect)
  }, [blocked, url, mixer, broadcast, captions, onStarted]);

  useEffect(() => {
    session.current?.follow(runId);
  }, [runId]);

  const face = useCallback((state: FaceState) => {
    faceRef.current = state;
    session.current?.face(state);
  }, []);
  const health = useCallback(() => session.current?.health() ?? null, []);
  const act = useCallback((action: VoiceAction) => runVoiceAction(action, session.current), []);
  const caption = useCallback(
    (run: string, said: readonly SaidLine[], streamNow: number) => {
      const audio = sharedAudio();
      return audio ? heardCaptions(captions, playoutOf(audio.audio)).view(run, said, streamNow) : null;
    },
    [captions],
  );

  if (blocked !== null) return { status: { state: "off", reason: blocked }, playing: false, followsAudio: false, caption, face, health, act };
  const sounding = audioRunning();
  const shown = shownStatus(status, sounding, broadcast, liveSession?.health().reconnect != null);
  return {
    status: shown,
    playing: (session.current?.player.playing ?? false) && sounding,
    followsAudio: shown.state === "ready" && sounding,
    caption,
    face,
    health,
    act,
  };
}
