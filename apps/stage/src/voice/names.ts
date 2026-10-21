// What the stage calls the voice's states on screen. Wire ids are never shown as they are.

import type { VoiceSkip, VoiceState } from "@shadowrunr/contract";

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

/** Why the daemon did not voice a line (the `skipped` frame's reason), as the voice panel says it. */
export const SKIP_LABEL: Record<VoiceSkip, string> = {
  late: "late",
  queue_full: "queue full",
  error: "error",
};

/** What the voice chip offers to do about its state. */
export type VoiceAction = "unlock" | "retry";

/** The voice chip's action buttons. */
export const VOICE_ACTION_LABEL: Record<VoiceAction, string> = {
  unlock: "Enable audio",
  retry: "Reconnect now",
};
