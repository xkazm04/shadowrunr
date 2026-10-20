// What the stage calls the voice's states on screen. Wire ids are never shown as they are.

import type { VoiceState } from "@shadowrunr/contract";

export const VOICE_STATE_LABEL: Record<VoiceState, string> = {
  off: "voice off",
  starting: "voice starting",
  ready: "voice ready",
  degraded: "voice degraded",
};

/** The disclosure beside the face while the voice is on. */
export const SYNTHETIC_VOICE = "synthetic voice";

/** The Voice toggle's caption. */
export const VOICE_TOGGLE = "Voice";
