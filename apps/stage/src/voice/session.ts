// The voice session: the `/voice` WebSocket (contract appendix (voice)), the player, and the voice
// state the controls show. Live only: it follows the run the stage is showing (`{"follow":
// {"run_id"}}`, re-sent when that run changes) and reconnects with backoff until stopped. The
// stage never waits on it: a dead socket or a silent sidecar is a `degraded` state with a reason,
// and the stream and the face carry on.
//
// The same socket carries the stage's reports (`report.ts`): each part's start, each line the
// stage's cap dropped, and the face drawn. A report made while the socket is down is not kept,
// except the face, which is sent again on every open: the daemon only ever reads them.

import type { VoiceState } from "@shadowrunr/contract";

import { frames, type Clock } from "../avatar/motion";
import type { SocketLike } from "../source";
import { parseVoiceFrame } from "./frame";
import { VoicePlayer, type AudioLike, type PlayerEvents } from "./player";
import { faceFrame, playedFrame, stageDroppedFrame, type FaceState, type VoiceClientFrame } from "./report";

export interface VoiceStatus {
  state: VoiceState;
  reason?: string;
}

/** The follow frame for a run: `null` follows the newest live run. */
export function followFrame(runId: string | null): string {
  return JSON.stringify({ follow: runId ? { run_id: runId } : {} });
}

const OPEN = 1;

export interface VoiceSessionOptions {
  url: string;
  audio: AudioLike;
  /** The frame clock the face signal is produced on (default: the page's). */
  clock?: Clock;
  onStatus: (s: VoiceStatus) => void;
  onBadFrame?: (message: string) => void;
  /** An utterance arrived for the followed run (the broadcast view resumes its audio on each). */
  onUtterance?: () => void;
  /** The daemon skipped a line: it will not sound. */
  onSkipped?: (runId: string, seq: number) => void;
  /** Hears the player's parts as they are scheduled, start and are dropped. */
  events?: PlayerEvents;
  /** The wall clock in epoch ms (default `Date.now`), for the reports' `at_wall_ms`. */
  wall?: () => number;
  socket?: (url: string) => SocketLike;
}

export class VoiceSession {
  readonly player: VoicePlayer;
  private ws: SocketLike | null = null;
  private stopped = false;
  private retry = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private runId: string | null = null;
  /** The run the last utterance belonged to (what `follow(null)` turned out to follow). */
  private heardRun: string | null = null;
  private faceNow: { state: FaceState; at: number } | null = null;
  private faceSent = "";

  constructor(private readonly o: VoiceSessionOptions) {
    const ev = o.events ?? {};
    this.player = new VoicePlayer(o.audio, o.clock ?? frames(), {
      scheduled: (p) => ev.scheduled?.(p),
      started: (p, lateS) => {
        this.report(playedFrame(p.runId, p.seq, p.part, this.wall() - lateS * 1000));
        ev.started?.(p, lateS);
      },
      dropped: (runId, seq) => {
        this.report(stageDroppedFrame(runId, seq));
        ev.dropped?.(runId, seq);
      },
    });
  }

  /** The face drawn now: reported on the first call and on every change. */
  face(state: FaceState): void {
    if (this.faceNow?.state === state) return;
    this.faceNow = { state, at: this.wall() };
    this.sendFace();
  }

  start(): void {
    this.open();
  }

  /** Follow `runId` (null: the newest live run); sent now when the socket is open, else on open. */
  follow(runId: string | null): void {
    if (runId === this.runId) return;
    this.runId = runId;
    this.send();
    this.sendFace();
  }

  stop(): void {
    this.stopped = true;
    if (this.timer) clearTimeout(this.timer);
    this.ws?.close();
    this.ws = null;
    this.player.dispose();
  }

  private wall(): number {
    return (this.o.wall ?? Date.now)();
  }

  private isOpen(): boolean {
    return this.ws !== null && this.ws.readyState === OPEN;
  }

  private send() {
    if (this.isOpen()) this.ws!.send(followFrame(this.runId));
  }

  private report(f: VoiceClientFrame) {
    if (this.isOpen()) this.ws!.send(JSON.stringify(f));
  }

  /** The face, once per (run, change) while the socket is open and the run is known. */
  private sendFace() {
    const run = this.runId ?? this.heardRun;
    const f = this.faceNow;
    if (!f || !run || !this.isOpen()) return;
    const key = `${run}|${f.state}|${f.at}`;
    if (key === this.faceSent) return;
    this.faceSent = key;
    this.report(faceFrame(run, f.state, f.at));
  }

  private open() {
    this.o.onStatus({ state: "starting", reason: "connecting to the voice" });
    const s = (this.o.socket ?? ((u) => new WebSocket(u) as unknown as SocketLike))(this.o.url);
    this.ws = s;
    s.onopen = () => {
      this.retry = 0;
      this.faceSent = "";
      this.send();
      this.sendFace();
    };
    s.onmessage = (m) => {
      if (typeof m.data !== "string") return;
      const f = parseVoiceFrame(m.data);
      if (!f.ok) {
        this.o.onBadFrame?.(f.message);
        return;
      }
      const v = f.value;
      if (v.type === "status") {
        const st: VoiceStatus = { state: v.state };
        if (v.reason) st.reason = v.reason;
        else if (v.state === "ready" && !v.visemes) st.reason = "no mouth shapes: the mouth follows loudness";
        this.o.onStatus(st);
        if (v.state !== "ready") this.player.reset();
      } else if (v.type === "utterance") {
        if (this.runId === null || v.run_id === this.runId) {
          this.heardRun = v.run_id;
          this.o.onUtterance?.();
          this.player.accept(v);
          this.sendFace();
        }
      } else if (this.runId === null || v.run_id === this.runId) {
        // a skipped line never arrives as audio; only its caption needs to know
        this.o.onSkipped?.(v.run_id, v.seq);
      }
    };
    s.onerror = () => {};
    s.onclose = () => {
      if (this.ws !== s) return;
      if (this.stopped) return;
      this.o.onStatus({ state: "degraded", reason: "the voice socket closed; reconnecting" });
      this.retry++;
      this.timer = setTimeout(() => this.open(), Math.min(10_000, 500 * 2 ** Math.min(this.retry, 5)));
    };
  }
}
