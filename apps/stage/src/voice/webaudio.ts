// The page's one AudioContext, with an AnalyserNode in front of the speakers for the mouth's
// loudness. Browsers start a context suspended until a user gesture, so the Voice toggle's click
// calls `unlockAudio()`, which makes the context (if needed) and resumes it inside the gesture.

import type { AudioLike, BufferLike, SourceLike } from "./player";

interface WebAudio {
  ctx: AudioContext;
  audio: AudioLike;
}

let shared: WebAudio | null = null;

function make(): WebAudio | null {
  if (typeof window === "undefined" || typeof window.AudioContext !== "function") return null;
  const ctx = new AudioContext({ latencyHint: "playback" });
  const analyser = ctx.createAnalyser();
  analyser.fftSize = 1024;
  analyser.smoothingTimeConstant = 0;
  analyser.connect(ctx.destination);
  const buf = new Float32Array(analyser.fftSize);
  const audio: AudioLike = {
    get currentTime() {
      return ctx.currentTime;
    },
    createBuffer: (ch, len, rate) => ctx.createBuffer(ch, len, rate) as BufferLike,
    createBufferSource: () => ctx.createBufferSource() as unknown as SourceLike,
    output: analyser,
    level() {
      analyser.getFloatTimeDomainData(buf);
      let sum = 0;
      for (let i = 0; i < buf.length; i++) sum += buf[i]! * buf[i]!;
      // RMS of speech sits around 0.05-0.25: scale so a loud syllable reads near 1
      return Math.min(1, Math.sqrt(sum / buf.length) * 4);
    },
  };
  return { ctx, audio };
}

/** The shared audio, made on first use; `null` where the browser has no Web Audio. */
export function sharedAudio(): WebAudio | null {
  shared ??= make();
  return shared;
}

/** Make and resume the context. Call from a user gesture (the Voice toggle). */
export function unlockAudio(): void {
  const a = sharedAudio();
  if (a && a.ctx.state === "suspended") void a.ctx.resume().catch(() => undefined);
}

/** Whether the context is allowed to sound (it starts suspended until a gesture). */
export function audioRunning(): boolean {
  return shared?.ctx.state === "running";
}
