// The live preview's DVR: a pure transport over the window the live store holds. Following, the
// preview is the head (`store.scene`, never re-folded); behind, it is `store.sceneAt(t)` with its
// own clock, played back at the controls' speed until it reaches the head again. Only the operator
// preview has one: the broadcast view (OBS, the sandbox) builds no DVR state and always draws the
// head, and nothing here touches the seqs or the acks the sandbox's stage_ack gate reads.

import type { SceneState } from "@shadowrunr/stage-kit";
import type { SceneAt } from "./live";

export type Dvr = { mode: "follow" } | { mode: "behind"; t: number; playing: boolean };

export const FOLLOW: Dvr = { mode: "follow" };

/** The held window, in stream ms: its earliest reachable moment and the head's latest timestamp. */
export interface DvrWindow {
  start: number;
  head: number;
}

export type DvrAction =
  | { type: "seek"; t: number; win: DvrWindow }
  | { type: "step"; by: number; win: DvrWindow }
  | { type: "play"; win: DvrWindow }
  | { type: "pause"; win: DvrWindow }
  | { type: "toggle"; win: DvrWindow }
  | { type: "tick"; dt: number; speed: number; win: DvrWindow }
  | { type: "goLive" };

/** Where the preview is, in stream ms: the head while following. */
function position(s: Dvr, win: DvrWindow): number {
  return s.mode === "follow" ? win.head : s.t;
}

/** Behind at `t` (clamped into the window), or following once `t` reaches the head. */
function behind(t: number, playing: boolean, win: DvrWindow): Dvr {
  if (t >= win.head) return FOLLOW;
  return { mode: "behind", t: Math.max(win.start, t), playing };
}

export function dvrReduce(s: Dvr, a: DvrAction): Dvr {
  switch (a.type) {
    case "goLive":
      return FOLLOW;
    case "seek":
      return behind(a.t, s.mode === "follow" || s.playing, a.win);
    case "step":
      return behind(position(s, a.win) + a.by, s.mode === "follow" || s.playing, a.win);
    case "play":
      return s.mode === "follow" || s.playing ? s : behind(s.t, true, a.win);
    case "pause":
      // pausing the head freezes that moment; the head itself keeps moving on
      return s.mode === "behind" && !s.playing ? s : { mode: "behind", t: position(s, a.win), playing: false };
    case "toggle":
      return dvrReduce(s, { type: s.mode === "follow" || s.playing ? "pause" : "play", win: a.win });
    case "tick": {
      if (s.mode === "follow") return s;
      if (!s.playing) return s.t < a.win.start ? { ...s, t: a.win.start } : s;
      return behind(s.t + a.dt * a.speed, true, a.win);
    }
  }
}

/** The DVR state a stage view starts with: none for the broadcast view or a file replay. */
export function dvrFor(o: { live: boolean; broadcast: boolean }): Dvr | null {
  return o.live && !o.broadcast ? FOLLOW : null;
}

export interface LiveView {
  scene: SceneState;
  /** The preview's clock: the live clock while following, the DVR's moment while behind. */
  now: number;
  /** How far the preview trails the live clock (0 while following). */
  behindMs: number;
  /** Captions may follow the voice's audio clock only at the head: the voice is live audio, so a
   * past moment shows the estimated caption timing. */
  followsVoice: boolean;
}

/** What the live canvas draws for a DVR state (null: the broadcast view, always the head). */
export function liveView(store: { scene: SceneState; sceneAt(t: number): SceneAt }, dvr: Dvr | null, now: number): LiveView {
  if (dvr === null || dvr.mode === "follow") return { scene: store.scene, now, behindMs: 0, followsVoice: true };
  const at = store.sceneAt(dvr.t);
  return { scene: at.scene, now: at.t, behindMs: Math.max(0, now - at.t), followsVoice: false };
}
