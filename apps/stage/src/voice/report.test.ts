// What the stage reports on the `/voice` socket (played, stage_dropped, face) and the broadcast
// view's audio, which resumes unasked and never asks for a click.
import { describe, expect, it } from "vitest";

import { createClock } from "../avatar/motion";
import type { SocketLike } from "../source";
import { readJson } from "../test-json";
import type { AudioLike, BufferLike, SourceLike } from "./player";
import { PlayoutClock } from "./playout";
import { faceFrame, faceStateOf, playedFrame, stageDroppedFrame } from "./report";
import { VoiceSession } from "./session";
import { shownStatus } from "./useVoice";
import { resumeIfSuspended } from "./webaudio";

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
  /** What was sent, minus the follow frames. */
  reports() {
    return this.sent.map((s) => readJson(s) as Record<string, unknown>).filter((f) => !("follow" in f));
  }
}

class FakeAudio implements AudioLike {
  currentTime = 10;
  outputLatency: number | undefined = undefined;
  stamp: { contextTime: number; performanceTime: number } | undefined = undefined;
  readonly output = {};
  outputTimestamp() {
    return this.stamp;
  }
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

function handClock() {
  let pending: ((t: number) => void) | null = null;
  const clock = createClock({ raf: (cb) => ((pending = cb), 1), caf: () => (pending = null), hidden: () => false, onVisible: () => {} });
  return {
    clock,
    tick() {
      const cb = pending;
      pending = null;
      cb?.(16);
    },
  };
}

/** `ms` of audio at 8 kHz (the lowest rate the frame guard takes). */
const utterance = (seq: number, part: number, ms: number) => ({
  type: "utterance",
  id: `r:${seq}:${part}`,
  run_id: "r",
  seq,
  part,
  last: true,
  text: "Kneel.",
  sample_rate: 8000,
  duration_ms: ms,
  pcm: btoa("\x00\x10".repeat(ms * 8)),
});

function rig(opts: { onUtterance?: () => void; onSkipped?: (run: string, seq: number) => void } = {}, start = 1_700_000_000_000) {
  const audio = new FakeAudio();
  const h = handClock();
  let wall = start;
  const sock = new FakeSocket();
  const s = new VoiceSession({
    url: "ws://127.0.0.1:7795/voice",
    audio,
    clock: h.clock,
    onStatus: () => {},
    wall: () => wall,
    playout: new PlayoutClock(audio, { wall: () => wall, perfWall: (p) => 999_500 + p }),
    socket: () => sock,
    ...opts,
  });
  return { audio, h, sock, s, advance: (ms: number) => (wall += ms) };
}

/** A part placed at context 12.0 (accepted at 11.94, START_LEAD_S ahead), its played report seen at `ctx`. */
function playedAt(o: { ctx: number; wall: number; latency?: number; stamp?: { contextTime: number; performanceTime: number } }) {
  const r = rig({}, o.wall);
  r.audio.currentTime = 11.94;
  if (o.latency !== undefined) r.audio.outputLatency = o.latency;
  r.audio.stamp = o.stamp;
  r.s.follow("r");
  r.s.start();
  r.sock.open();
  r.sock.push(utterance(4, 0, 500));
  expect(r.s.player.scheduled()[0]!.start).toBeCloseTo(12.0, 9);
  r.audio.currentTime = o.ctx;
  r.h.tick();
  return r.sock.reports();
}

describe("report frames", () => {
  it("match the show contract key for key", () => {
    expect(JSON.stringify(playedFrame("r", 4, 1, 1_700_000_000_123.4))).toBe('{"played":{"run_id":"r","seq":4,"part":1,"at_wall_ms":1700000000123}}');
    expect(JSON.stringify(stageDroppedFrame("r", 9))).toBe('{"stage_dropped":{"run_id":"r","seq":9,"reason":"stage_cap"}}');
    expect(JSON.stringify(faceFrame("r", "rig", 5))).toBe('{"face":{"run_id":"r","state":"rig","at_wall_ms":5}}');
  });
});

describe("the face state", () => {
  it("only the daemon's real layer rig is `rig`; the placeholder and nothing drawn are `silhouette`", () => {
    expect(faceStateOf("rig")).toBe("rig");
    expect(faceStateOf("placeholder")).toBe("silhouette");
    expect(faceStateOf(null)).toBe("silhouette");
  });
});

describe("the session reports on the /voice socket", () => {
  it("a part's start, with the wall time its audio actually started", () => {
    const { audio, h, sock, s, advance } = rig();
    s.follow("r");
    s.start();
    sock.open();
    sock.push(utterance(4, 0, 500));
    expect(sock.reports()).toEqual([]);
    // the audio started at 10.06; the frame is seen 40 ms later
    audio.currentTime = 10.1;
    advance(40);
    h.tick();
    expect(sock.reports()).toEqual([{ played: { run_id: "r", seq: 4, part: 0, at_wall_ms: 1_700_000_000_000 } }]);
    h.tick();
    expect(sock.reports()).toHaveLength(1);
  });

  it("the wall time the part's start is heard, from the output timestamp (the latency is not added twice)", () => {
    // performanceTime 500 is wall 1_000_000; the sample at context 11.9 is at the speakers then.
    // Today's tick-based estimate (wall 1_000_300, 140 ms past the start) would say 1_000_160.
    const reports = playedAt({ ctx: 12.14, wall: 1_000_300, latency: 0.1, stamp: { contextTime: 11.9, performanceTime: 500 } });
    expect(reports).toEqual([{ played: { run_id: "r", seq: 4, part: 0, at_wall_ms: 1_000_100 } }]);
  });

  it("the same start seen on two ticks 20 ms of wall apart (a quantized context clock) reports one time", () => {
    const stamp = { contextTime: 11.9, performanceTime: 500 };
    const a = playedAt({ ctx: 12.14, wall: 1_000_300, latency: 0.1, stamp });
    const b = playedAt({ ctx: 12.14, wall: 1_000_320, latency: 0.1, stamp });
    expect(a).toEqual(b);
  });

  it("without an output timestamp the report uses the latency the device reports now (a device swap)", () => {
    const reports = playedAt({ ctx: 12.3, wall: 2_000_000, latency: 0.2 });
    // heard now is 12.1, 100 ms past the start
    expect(reports).toEqual([{ played: { run_id: "r", seq: 4, part: 0, at_wall_ms: 1_999_900 } }]);
  });

  it("each whole line the stage's cap drops", () => {
    const { audio, sock, s } = rig();
    s.follow("r");
    s.start();
    sock.open();
    sock.push(utterance(1, 0, 9000));
    audio.currentTime = 10.1;
    sock.push(utterance(2, 0, 9000));
    sock.push(utterance(3, 0, 9000));
    expect(sock.reports()).toEqual([{ stage_dropped: { run_id: "r", seq: 2, reason: "stage_cap" } }]);
  });

  it("the face on load and on every change, again after a reconnect, never twice for one change", () => {
    const { sock, s, advance } = rig();
    s.face("silhouette");
    s.follow("r");
    s.start();
    expect(sock.reports()).toEqual([]);
    sock.open();
    expect(sock.reports()).toEqual([{ face: { run_id: "r", state: "silhouette", at_wall_ms: 1_700_000_000_000 } }]);
    advance(900);
    s.face("rig");
    s.face("rig");
    expect(sock.reports().slice(1)).toEqual([{ face: { run_id: "r", state: "rig", at_wall_ms: 1_700_000_000_900 } }]);
    sock.open();
    expect(sock.reports()).toHaveLength(3);
  });

  it("following the newest run, the face waits for the run's first utterance", () => {
    const { sock, s } = rig();
    s.face("rig");
    s.start();
    sock.open();
    expect(sock.reports()).toEqual([]);
    sock.push(utterance(1, 0, 10));
    expect(sock.reports()[0]).toMatchObject({ face: { run_id: "r", state: "rig" } });
  });

  it("a daemon that ignores the reports changes nothing, and replies are not needed", () => {
    const { sock, s } = rig();
    s.follow("r");
    s.start();
    sock.open();
    s.face("rig");
    sock.push(utterance(1, 0, 10));
    sock.push({ type: "status", state: "ready", visemes: true });
    expect(s.player.scheduled()).toHaveLength(1);
  });

  it("skipped lines and utterances are heard (the captions and the broadcast audio)", () => {
    const heard: string[] = [];
    const { sock, s } = rig({ onUtterance: () => heard.push("utterance"), onSkipped: (r, q) => heard.push(`skipped ${r}:${q}`) });
    s.follow("r");
    s.start();
    sock.open();
    sock.push(utterance(1, 0, 10));
    sock.push({ type: "skipped", run_id: "r", seq: 2, part: 0, reason: "late" });
    sock.push({ type: "skipped", run_id: "other", seq: 3, part: 0, reason: "late" });
    expect(heard).toEqual(["utterance", "skipped r:2"]);
  });
});

describe("broadcast audio", () => {
  it("a suspended context is asked to resume, unasked; a running one is left alone", () => {
    const calls: string[] = [];
    const ctx = {
      state: "suspended",
      resume() {
        calls.push("resume");
        return Promise.resolve();
      },
    };
    expect(resumeIfSuspended(ctx)).toBe(true);
    ctx.state = "running";
    expect(resumeIfSuspended(ctx)).toBe(false);
    expect(calls).toEqual(["resume"]);
  });

  it("a context whose resume rejects or throws never breaks the stage", async () => {
    expect(resumeIfSuspended({ state: "suspended", resume: () => Promise.reject(new Error("no gesture")) })).toBe(true);
    expect(
      resumeIfSuspended({
        state: "suspended",
        resume: () => {
          throw new Error("closed");
        },
      }),
    ).toBe(false);
    await Promise.resolve();
  });

  it("the broadcast view never asks for a click; the operator view says why the audio waits", () => {
    const ready = { state: "ready" as const };
    const onAir = shownStatus(ready, false, true);
    expect(onAir.state).toBe("starting");
    expect(onAir.reason).not.toMatch(/click/i);
    expect(shownStatus(ready, false, false)).toEqual({ state: "degraded", reason: "audio is blocked until a click on the page" });
    expect(shownStatus(ready, true, true)).toEqual(ready);
  });
});
