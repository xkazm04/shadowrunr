// The stage's voice, as React sees it: the face mixer (voice, else the caption, else rest) and the
// hook that runs a `VoiceSession` while the voice is allowed, with the captions that follow its
// audio. The session, the player and the frame guard are imported with `import()` when the voice
// is first allowed, so a stage whose voice is off pays nothing for them.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { RESTING, type FaceSignal, type FaceSource } from "../avatar/types";
import { VoiceCaptions, type SaidLine, type VoiceCaptionView } from "./captions";
import { playoutOf, type PlayoutClock } from "./playout";
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
 * for a click in a browser, and only waiting in the broadcast view, which never asks for one.
 */
export function shownStatus(status: VoiceStatus, running: boolean, broadcast: boolean): VoiceStatus {
  if (status.state !== "ready" || running) return status;
  if (broadcast) return { state: "starting", reason: "waiting for the audio to start" };
  return { state: "degraded", reason: "audio is blocked until a click on the page" };
}

/**
 * Run the voice while `blocked` is null: connect to `url`, follow `runId`, feed the mixer. Returns
 * the state for the controls; `blocked` (why it is off) shows as `off` with that reason. In the
 * broadcast view the audio is resumed unasked, at once and on every utterance (see webaudio.ts).
 */
export function useVoice(opts: { blocked: string | null; url: string; runId: string | null; mixer: FaceMixer; broadcast: boolean }): VoiceView {
  const { blocked, url, runId, mixer, broadcast } = opts;
  const [status, setStatus] = useState<VoiceStatus>({ state: "off" });
  const session = useRef<VoiceSession | null>(null);
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
            dropped: heard.unvoiced,
          },
        });
        session.current = s;
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
    };
    // the run is followed by the effect below (a run change is a follow frame, not a reconnect)
  }, [blocked, url, mixer, broadcast, captions]);

  useEffect(() => {
    session.current?.follow(runId);
  }, [runId]);

  const face = useCallback((state: FaceState) => {
    faceRef.current = state;
    session.current?.face(state);
  }, []);
  const caption = useCallback(
    (run: string, said: readonly SaidLine[], streamNow: number) => {
      const audio = sharedAudio();
      return audio ? heardCaptions(captions, playoutOf(audio.audio)).view(run, said, streamNow) : null;
    },
    [captions],
  );

  if (blocked !== null) return { status: { state: "off", reason: blocked }, playing: false, followsAudio: false, caption, face };
  const running = audioRunning();
  const shown = shownStatus(status, running, broadcast);
  return {
    status: shown,
    playing: (session.current?.player.playing ?? false) && running,
    followsAudio: shown.state === "ready" && running,
    caption,
    face,
  };
}
