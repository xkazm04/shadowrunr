// The stage's voice, as React sees it: the face mixer (voice, else the caption, else rest) and the
// hook that runs a `VoiceSession` while the voice is allowed. The session, the player and the frame
// guard are imported with `import()` when the voice is first allowed, so a stage whose voice is
// off pays nothing for them.

import { useEffect, useRef, useState } from "react";

import { RESTING, type FaceSignal, type FaceSource } from "../avatar/types";
import type { VoiceSession, VoiceStatus } from "./session";
import { audioRunning, sharedAudio } from "./webaudio";

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
}

/**
 * Run the voice while `blocked` is null: connect to `url`, follow `runId`, feed the mixer. Returns
 * the state for the controls; `blocked` (why it is off) shows as `off` with that reason.
 */
export function useVoice(opts: { blocked: string | null; url: string; runId: string | null; mixer: FaceMixer }): VoiceView {
  const { blocked, url, runId, mixer } = opts;
  const [status, setStatus] = useState<VoiceStatus>({ state: "off" });
  const session = useRef<VoiceSession | null>(null);
  const runIdRef = useRef(runId);
  runIdRef.current = runId;

  useEffect(() => {
    if (blocked !== null) return;
    const audio = sharedAudio();
    if (!audio) {
      setStatus({ state: "degraded", reason: "this browser has no Web Audio" });
      return;
    }
    let cancelled = false;
    setStatus({ state: "starting", reason: "loading the voice" });
    import("./session").then(
      ({ VoiceSession }) => {
        if (cancelled) return;
        const s = new VoiceSession({ url, audio: audio.audio, onStatus: setStatus });
        session.current = s;
        s.follow(runIdRef.current);
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
  }, [blocked, url, mixer]);

  useEffect(() => {
    session.current?.follow(runId);
  }, [runId]);

  if (blocked !== null) return { status: { state: "off", reason: blocked }, playing: false };
  if (status.state === "ready" && !audioRunning()) {
    return { status: { state: "degraded", reason: "audio is blocked until a click on the page" }, playing: false };
  }
  return { status, playing: (session.current?.player.playing ?? false) && audioRunning() };
}
