// The voice session: the `/voice` WebSocket (contract appendix (voice)), the player, and the voice
// state the controls show. Live only: it follows the run the stage is showing (`{"follow":
// {"run_id"}}`, re-sent when that run changes) and reconnects with backoff until stopped. The
// stage never waits on it: a dead socket or a silent sidecar is a `degraded` state with a reason,
// and the stream and the face carry on.

import type { VoiceState } from "@shadowrunr/contract";

import { frames, type Clock } from "../avatar/motion";
import type { SocketLike } from "../source";
import { parseVoiceFrame } from "./frame";
import { VoicePlayer, type AudioLike } from "./player";

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
  socket?: (url: string) => SocketLike;
}

export class VoiceSession {
  readonly player: VoicePlayer;
  private ws: SocketLike | null = null;
  private stopped = false;
  private retry = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private runId: string | null = null;

  constructor(private readonly o: VoiceSessionOptions) {
    this.player = new VoicePlayer(o.audio, o.clock ?? frames());
  }

  start(): void {
    this.open();
  }

  /** Follow `runId` (null: the newest live run); sent now when the socket is open, else on open. */
  follow(runId: string | null): void {
    if (runId === this.runId) return;
    this.runId = runId;
    this.send();
  }

  stop(): void {
    this.stopped = true;
    if (this.timer) clearTimeout(this.timer);
    this.ws?.close();
    this.ws = null;
    this.player.dispose();
  }

  private send() {
    if (this.ws && this.ws.readyState === OPEN) this.ws.send(followFrame(this.runId));
  }

  private open() {
    this.o.onStatus({ state: "starting", reason: "connecting to the voice" });
    const s = (this.o.socket ?? ((u) => new WebSocket(u) as unknown as SocketLike))(this.o.url);
    this.ws = s;
    s.onopen = () => {
      this.retry = 0;
      this.send();
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
        if (this.runId === null || v.run_id === this.runId) this.player.accept(v);
      }
      // `skipped` lines simply never arrive as audio; nothing is queued for them here.
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
