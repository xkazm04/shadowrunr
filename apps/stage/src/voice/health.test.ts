// The voice panel's facts: what the session saw go wrong for the followed run (lines the daemon
// skipped and why, lines the stage's cap dropped, frames the guard refused, the reconnect pending)
// and the two actions they call for, Enable audio and Reconnect now.
import { afterEach, describe, expect, it, vi } from "vitest";

import { createClock } from "../avatar/motion";
import type { SocketLike } from "../source";
import { healthRows } from "./health";
import type { AudioLike, BufferLike, SourceLike } from "./player";
import { VoiceSession } from "./session";
import { runVoiceAction, shownStatus } from "./useVoice";

class FakeSocket implements SocketLike {
  onopen: (() => void) | null = null;
  onmessage: ((m: { data: unknown }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  readyState = 0;
  sent: string[] = [];
  send(d: string) {
    this.sent.push(d);
  }
  close() {
    this.readyState = 3;
  }
  open() {
    this.readyState = 1;
    this.onopen?.();
  }
  push(frame: unknown) {
    this.onmessage?.({ data: JSON.stringify(frame) });
  }
}

class FakeAudio implements AudioLike {
  currentTime = 10;
  readonly output = {};
  createBuffer(_c: number, len: number, rate: number): BufferLike {
    const d = new Float32Array(len);
    return { duration: len / rate, getChannelData: () => d };
  }
  createBufferSource(): SourceLike {
    return { buffer: null, connect: () => undefined, start: () => {}, stop: () => {}, disconnect: () => {} };
  }
  level() {
    return 0;
  }
}

function rig() {
  const audio = new FakeAudio();
  const sockets: FakeSocket[] = [];
  const clock = createClock({ raf: () => 1, caf: () => {}, hidden: () => false, onVisible: () => {} });
  const s = new VoiceSession({
    url: "ws://127.0.0.1:7717/voice",
    audio,
    clock,
    onStatus: () => {},
    socket: () => {
      const k = new FakeSocket();
      sockets.push(k);
      return k;
    },
  });
  return { audio, s, sockets };
}

/** `ms` of audio at 8 kHz for run `run`. */
const utterance = (run: string, seq: number, ms: number, extra: Record<string, unknown> = {}) => ({
  type: "utterance",
  id: `${run}:${seq}:0`,
  run_id: run,
  seq,
  part: 0,
  last: true,
  text: "Kneel.",
  sample_rate: 8000,
  duration_ms: ms,
  pcm: btoa("\x00\x10".repeat(ms * 8)),
  ...extra,
});

const skipped = (seq: number, reason: string) => ({ type: "skipped", run_id: "r", seq, part: 0, reason });

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.resetModules();
});

describe("the voice's health for the followed run", () => {
  it("counts the daemon's skipped lines by reason, and the panel says so", () => {
    const { s, sockets } = rig();
    s.follow("r");
    s.start();
    sockets[0]!.open();
    sockets[0]!.push(skipped(1, "queue_full"));
    sockets[0]!.push(skipped(2, "late"));
    sockets[0]!.push(skipped(3, "queue_full"));
    sockets[0]!.push({ type: "skipped", run_id: "other", seq: 4, part: 0, reason: "error" });
    const h = s.health();
    expect(h.skipped).toEqual({ late: 1, queue_full: 2, error: 0 });
    expect(healthRows(h)).toContain("3 lines not voiced by the daemon (2 queue full, 1 late)");
    s.stop();
  });

  it("counts the frames the guard refused, with the last reason", () => {
    const { s, sockets } = rig();
    s.follow("r");
    s.start();
    sockets[0]!.open();
    sockets[0]!.onmessage?.({ data: "{oops" });
    sockets[0]!.push(utterance("r", 1, 10, { sample_rate: 3 }));
    const h = s.health();
    expect(h.badFrames).toBe(2);
    expect(h.lastBad).toBe("voice utterance frame: bad sample_rate");
    s.stop();
  });

  it("counts the lines the stage's cap dropped and the audio waiting; a run change starts afresh", () => {
    const { audio, s, sockets } = rig();
    s.follow("r");
    s.start();
    sockets[0]!.open();
    sockets[0]!.push(utterance("r", 1, 9000));
    audio.currentTime = 10.1;
    sockets[0]!.push(utterance("r", 2, 9000));
    sockets[0]!.push(utterance("r", 3, 9000));
    sockets[0]!.push(skipped(4, "late"));
    sockets[0]!.onmessage?.({ data: "{oops" });
    const h = s.health();
    expect(h.stageDropped).toBe(1);
    expect(h.behindS).toBe(Math.round(s.player.ahead * 10) / 10);
    expect(h.behindS).toBeGreaterThan(0);
    s.follow("s");
    expect(s.health()).toMatchObject({ stageDropped: 0, badFrames: 0, lastBad: null, skipped: { late: 0, queue_full: 0, error: 0 } });
    s.stop();
  });

  it("shows the reconnect pending, and Reconnect now opens at once without losing the backoff", () => {
    vi.useFakeTimers();
    const { s, sockets } = rig();
    s.start();
    sockets[0]!.open();
    expect(s.health().reconnect).toBeNull();
    sockets[0]!.onclose?.();
    vi.advanceTimersByTime(1000);
    expect(sockets).toHaveLength(2);
    sockets[1]!.onclose?.();
    expect(s.health().reconnect).toEqual({ attempt: 2, nextInMs: 2000 });
    s.retryNow();
    expect(sockets).toHaveLength(3);
    expect(s.health().reconnect).toBeNull();
    vi.advanceTimersByTime(5000);
    expect(sockets).toHaveLength(3);
    sockets[2]!.onclose?.();
    expect(s.health().reconnect).toEqual({ attempt: 3, nextInMs: 4000 });
    expect(healthRows(s.health())).toContain("reconnecting: attempt 3, next in 4 s");
    s.stop();
  });
});

/** An AudioContext that starts suspended, as a browser makes one before any gesture. */
function stubAudioContext() {
  const made: { state: string; resumes: number }[] = [];
  class FakeContext {
    state = "suspended";
    resumes = 0;
    currentTime = 0;
    destination = {};
    constructor() {
      made.push(this);
    }
    createAnalyser() {
      return { fftSize: 0, smoothingTimeConstant: 0, connect: () => {}, getFloatTimeDomainData: () => {} };
    }
    resume() {
      this.resumes++;
      this.state = "running";
      return Promise.resolve();
    }
  }
  vi.stubGlobal("AudioContext", FakeContext);
  vi.stubGlobal("window", { AudioContext: FakeContext });
  return made;
}

describe("the voice's actions", () => {
  it("a blocked context in the operator view offers Enable audio, and pressing it resumes the audio", async () => {
    const made = stubAudioContext();
    const webaudio = await import("./webaudio");
    const voice = await import("./useVoice");
    webaudio.sharedAudio();
    const ready = { state: "ready" as const };
    const blocked = voice.shownStatus(ready, webaudio.audioRunning(), false);
    expect(blocked.state).toBe("degraded");
    expect(blocked.action).toBe("unlock");
    expect(blocked.reason).toMatch(/Enable audio/);
    expect(blocked.reason).not.toMatch(/click on the page/);
    const toggled: boolean[] = [];
    voice.runVoiceAction("unlock", { retryNow: () => toggled.push(false) });
    // one resume, inside the press (the browser's gesture), and the toggle is left alone
    expect(made).toHaveLength(1);
    expect(made[0]!.resumes).toBe(1);
    expect(toggled).toEqual([]);
    expect(webaudio.audioRunning()).toBe(true);
    expect(voice.shownStatus(ready, webaudio.audioRunning(), false)).toEqual(ready);
  });

  it("a socket waiting to reconnect offers Reconnect now in the operator view", () => {
    const closed = { state: "degraded" as const, reason: "the voice socket closed; reconnecting" };
    expect(shownStatus(closed, true, false, true)).toEqual({ ...closed, action: "retry" });
    let retried = 0;
    runVoiceAction("retry", { retryNow: () => retried++ });
    expect(retried).toBe(1);
  });

  it("the broadcast view offers no action and its status rule is unchanged", () => {
    const ready = { state: "ready" as const };
    expect(shownStatus(ready, false, true)).toEqual({ state: "starting", reason: "waiting for the audio to start" });
    expect(shownStatus(ready, true, true)).toEqual(ready);
    const closed = { state: "degraded" as const, reason: "the voice socket closed; reconnecting" };
    expect(shownStatus(closed, true, true, true)).toEqual(closed);
  });
});
