// Whose mouth moves. The caption on screen says who is speaking: Malakor's own lines move his face
// (the level-less speaking phase without a voice, the voice's mouth with one), and a line the Table
// Read's guest says (SEG-03, `say` with `speaker: foil`) lights the guest's speaking glow instead.
// With the voice on, the caption follows the audio clock (voice/captions.ts), so the glow is lit
// exactly while the guest's audio is heard; the player itself never hands a guest's audio to his
// mouth (voice/player.ts `signalAt`).

import type { ScheduledCaption } from "@shadowrunr/stage-kit";

import type { VoiceCaptionView } from "./voice/captions";

/** A caption counts as spoken for this long after its last word lands (ms). */
export const SPEAKING_TAIL_MS = 300;

export interface Speakers {
  /** Malakor is speaking: his face's speaking phase, his silhouette's glow. */
  host: boolean;
  /** The guest's foil id while the guest is speaking, else null. */
  guest: string | null;
}

export const SILENT: Speakers = Object.freeze({ host: false, guest: null });

/**
 * Who speaks at `now`. `voiceCaption` undefined: no voice is followed, the scene's caption on its
 * estimated timing decides; otherwise the voice's caption (null: nothing heard) decides.
 */
export function speakersNow(caption: ScheduledCaption | null, voiceCaption: VoiceCaptionView | null | undefined, now: number): Speakers {
  let speaking: boolean;
  let foil: string | undefined;
  if (voiceCaption === undefined) {
    if (!caption) return SILENT;
    const e = now - caption.at;
    speaking = e >= 0 && e <= caption.spokenMs + SPEAKING_TAIL_MS;
    foil = caption.foil;
  } else {
    if (!voiceCaption) return SILENT;
    speaking = voiceCaption.speaking;
    foil = voiceCaption.caption.foil;
  }
  if (!speaking) return SILENT;
  return foil === undefined ? { host: true, guest: null } : { host: false, guest: foil };
}
