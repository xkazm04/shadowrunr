// The voice session on a fake socket, the toggle's memory, and the face mixer.
import { afterEach, describe, expect, it, vi } from "vitest";

import { createClock } from "../avatar/motion";
import { RESTING, type FaceSignal } from "../avatar/types";
import type { SocketLike } from "../source";
import type { AudioLike, BufferLike, SourceLike } from "./player";
import { followFrame, VoiceSession, type VoiceStatus } from "./session";
import { readVoiceOn, voiceBlocked, VOICE_KEY, writeVoiceOn, type StorageLike } from "./toggle";
import { rigBaseUrl, voiceUrl } from "./urls";
import { FaceMixer } from "./useVoice";

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

const audio: AudioLike = {
  currentTime: 0,
  output: {},
  createBuffer: (_c, len, rate): BufferLike => {
    const d = new Float32Array(len);
    return { duration: len / rate, getChannelData: () => d };
  },
  createBufferSource: (): SourceLike => ({ buffer: null, connect: () => undefined, start: () => {}, stop: () => {}, disconnect: () => {} }),
  level: () => 0,
};

function session() {
  const sockets: FakeSocket[] = [];
  const statuses: VoiceStatus[] = [];
  const bad: string[] = [];
  const clock = createClock({ raf: () => 1, caf: () => {}, hidden: () => false, onVisible: () => {} });
  const s = new VoiceSession({
    url: "ws://127.0.0.1:7717/voice",
    audio,
    clock,
    onStatus: (st) => statuses.push(st),
    onBadFrame: (m) => bad.push(m),
    socket: () => {
      const k = new FakeSocket();
      sockets.push(k);
      return k;
    },
  });
  return { s, sockets, statuses, bad };
}

const utterance = (run: string, seq: number) => ({
  type: "utterance",
  id: `${run}:${seq}:0`,
  run_id: run,
  seq,
  part: 0,
  last: true,
  text: "Kneel.",
  sample_rate: 24000,
  duration_ms: 1,
  pcm: btoa("\x00\x10\x00\x10"),
});

afterEach(() => vi.useRealTimers());

describe("urls", () => {
  it("the voice and the rig live on the same daemon as /events", () => {
    expect(voiceUrl("ws://127.0.0.1:7717/events")).toBe("ws://127.0.0.1:7717/voice");
    expect(voiceUrl("wss://h:1/x/events?y=1")).toBe("wss://h:1/x/voice");
    expect(rigBaseUrl("ws://127.0.0.1:7717/events")).toBe("http://127.0.0.1:7717/avatar/");
    expect(rigBaseUrl("wss://h/events")).toBe("https://h/avatar/");
  });

  it("the follow frame names the run, or nothing for the newest live run", () => {
    expect(followFrame("run-7")).toBe('{"follow":{"run_id":"run-7"}}');
    expect(followFrame(null)).toBe('{"follow":{}}');
  });
});

describe("the voice session", () => {
  it("follows the shown run on open and again when it changes", () => {
    const { s, sockets } = session();
    s.follow("a");
    s.start();
    sockets[0]!.open();
    s.follow("a");
    s.follow("b");
    expect(sockets[0]!.sent).toEqual([followFrame("a"), followFrame("b")]);
    s.stop();
  });

  it("reports starting, then the daemon's status; a ready voice without shapes says so", () => {
    const { s, sockets, statuses } = session();
    s.start();
    sockets[0]!.open();
    sockets[0]!.push({ type: "status", state: "degraded", reason: "engine missing", visemes: false });
    sockets[0]!.push({ type: "status", state: "ready", voice: "af_heart", visemes: false });
    expect(statuses).toEqual([
      { state: "starting", reason: "connecting to the voice" },
      { state: "degraded", reason: "engine missing" },
      { state: "ready", reason: "no mouth shapes: the mouth follows loudness" },
    ]);
    s.stop();
  });

  it("plays only the followed run's utterances, and reports bad frames", () => {
    const { s, sockets, bad } = session();
    s.follow("a");
    s.start();
    sockets[0]!.open();
    sockets[0]!.push(utterance("b", 1));
    sockets[0]!.push(utterance("a", 2));
    sockets[0]!.onmessage?.({ data: "{oops" });
    expect(s.player.scheduled().map((e) => e.seq)).toEqual([2]);
    expect(bad).toEqual(["voice frame is not JSON"]);
    s.stop();
  });

  it("a closed socket is degraded and retried with backoff; stop ends it", () => {
    vi.useFakeTimers();
    const { s, sockets, statuses } = session();
    s.start();
    sockets[0]!.open();
    sockets[0]!.onclose?.();
    expect(statuses.at(-1)).toEqual({ state: "degraded", reason: "the voice socket closed; reconnecting" });
    vi.advanceTimersByTime(1000);
    expect(sockets).toHaveLength(2);
    s.stop();
    sockets[1]!.onclose?.();
    vi.advanceTimersByTime(20_000);
    expect(sockets).toHaveLength(2);
  });
});

describe("the Voice toggle", () => {
  const memory = (): StorageLike & { data: Map<string, string> } => {
    const data = new Map<string, string>();
    return { data, getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v) };
  };

  it("is off by default and remembers its setting", () => {
    const m = memory();
    expect(readVoiceOn(m)).toBe(false);
    writeVoiceOn(m, true);
    expect(m.data.get(VOICE_KEY)).toBe("1");
    expect(readVoiceOn(m)).toBe(true);
    writeVoiceOn(m, false);
    expect(readVoiceOn(m)).toBe(false);
  });

  it("blocked or missing storage reads as off and never throws", () => {
    const broken: StorageLike = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
    };
    expect(readVoiceOn(broken)).toBe(false);
    expect(() => writeVoiceOn(broken, true)).not.toThrow();
    expect(readVoiceOn(null)).toBe(false);
  });

  it("the voice plays only live, toggled on, and never headless", () => {
    expect(voiceBlocked({ live: true, on: true, headless: false })).toBeNull();
    expect(voiceBlocked({ live: true, on: false, headless: false })).toMatch(/toggle is off/);
    expect(voiceBlocked({ live: false, on: true, headless: false })).toMatch(/replays/);
    expect(voiceBlocked({ live: true, on: true, headless: true })).toMatch(/headless/);
  });
});

describe("the face mixer", () => {
  it("the voice wins, a caption gives level-less speech, else rest", () => {
    const m = new FaceMixer();
    const seen: FaceSignal[] = [];
    m.subscribe((s) => seen.push(s));
    m.setCaption(true);
    expect(m.current()).toMatchObject({ phase: "speaking", level: 0, viseme: null });
    let push: (s: FaceSignal) => void = () => {};
    m.setVoice({ subscribe: (l) => ((push = l), () => {}) });
    push({ phase: "speaking", level: 0.7, viseme: "aa", open: 0, generation: 3 });
    expect(m.current()).toMatchObject({ level: 0.7, viseme: "aa" });
    push(RESTING);
    expect(m.current()).toMatchObject({ phase: "speaking", level: 0 });
    m.setCaption(false);
    expect(m.current()).toBe(RESTING);
    expect(seen[0]).toBe(RESTING);
    expect(seen.at(-1)).toBe(RESTING);
  });
});
