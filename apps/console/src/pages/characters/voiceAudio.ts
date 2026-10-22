// The Voice tab's playback. A preview or bench line arrives as base64 PCM16 mono at its sample
// rate; it is decoded here and played on one AudioContext, made on the first play (inside the
// owner's press, so the browser lets it sound). The stage's player is the stage app's own, so
// this is the small local decoder. One line plays at a time: a new play stops the last.

/** Base64 PCM16 little-endian mono to samples in -1..1. */
export function decodePcm16(b64: string): Float32Array<ArrayBuffer> {
  const bin = atob(b64);
  const n = bin.length >> 1;
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    let v = (bin.charCodeAt(2 * i + 1) << 8) | bin.charCodeAt(2 * i);
    if (v >= 0x8000) v -= 0x10000;
    out[i] = v / 0x8000;
  }
  return out;
}

let ctx: AudioContext | undefined;
let current: AudioBufferSourceNode | undefined;

export const canPlay = (): boolean => typeof window !== "undefined" && typeof window.AudioContext === "function";

/** Stops the line playing, if any; its `playPcm` promise resolves. */
export function stopPcm(): void {
  const src = current;
  current = undefined;
  src?.stop();
}

/** Plays one line; resolves when it ends or is stopped. Rejects when there is no audio output. */
export function playPcm(pcm: string, sampleRate: number): Promise<void> {
  if (!canPlay()) return Promise.reject(new Error("This console has no audio output."));
  stopPcm();
  const samples = decodePcm16(pcm);
  if (samples.length === 0) return Promise.resolve();
  ctx ??= new AudioContext();
  const c = ctx;
  const buf = c.createBuffer(1, samples.length, sampleRate);
  buf.copyToChannel(samples, 0);
  const src = c.createBufferSource();
  src.buffer = buf;
  src.connect(c.destination);
  current = src;
  return new Promise((resolve) => {
    src.onended = () => {
      if (current === src) current = undefined;
      resolve();
    };
    void c.resume();
    src.start();
  });
}
