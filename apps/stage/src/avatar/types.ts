// Malakor's face: the shapes every part of it agrees on (contract appendix (voice)).
//
// The voice player turns the audio it is playing (the one clock), the utterance's mouth shapes and
// the playback loudness into one `FaceSignal` a frame. The `<Avatar>` reads signals from a
// `FaceSource` and draws them; a signal never causes a React render.

import type { Viseme } from "@shadowrunr/contract";

export type { Viseme };

/** The Oculus 15 mouth shapes the voice's timeline uses (contract `Viseme`). */
export const VISEMES = ["sil", "PP", "FF", "TH", "DD", "kk", "CH", "SS", "nn", "RR", "aa", "E", "ih", "oh", "ou"] as const satisfies readonly Viseme[];

/** The forms the face can take: the daemon's rig (`/avatar/layers.json`) or the procedural placeholder. */
export type AvatarForm = "rig" | "placeholder";

/** What the face is doing. Malakor speaks or waits; the stage has no listening or thinking face. */
export type FacePhase = "idle" | "speaking";

/**
 * What the face shows this frame. `viseme` is the mouth shape under the playback position when the
 * utterance has a timeline, else `null` (the mouth follows `level`); `open` is the producer's own
 * opening 0..1, used when there is no viseme. `generation` names the utterance it belongs to.
 */
export interface FaceSignal {
  phase: FacePhase;
  level: number;
  viseme: Viseme | null;
  open: number;
  generation: number;
}

/** Where an `<Avatar>` reads its signal from: a subscription that hands it every new signal. */
export interface FaceSource {
  subscribe(listener: (signal: FaceSignal) => void): () => void;
}

/** A face at rest: what the stage shows before any signal arrives. */
export const RESTING: FaceSignal = Object.freeze({ phase: "idle", level: 0, viseme: null, open: 0, generation: 0 });
