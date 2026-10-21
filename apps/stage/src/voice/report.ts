// What the stage tells the daemon on the `/voice` socket, beside `{"follow":{...}}`: a part's
// audio started (`played`), the stage dropped a whole line to respect MAX_AHEAD_S
// (`stage_dropped`), and which face is drawn (`face`, on load and on every change). The daemon
// writes them to the run's `voice.jsonl`; one that ignores them loses nothing, since a report
// never changes the follow and the stage never waits for a reply.
//
// The frames are the generated contract's (`VoiceClientFrame` in crates/shadowrunr-types).

import type {
  FaceState,
  VoiceClientFrame,
  VoiceFaceFrame,
  VoicePlayedFrame,
  VoiceStageDroppedFrame,
} from "@shadowrunr/contract";

export type { FaceState, VoiceClientFrame, VoiceFaceFrame, VoicePlayedFrame, VoiceStageDroppedFrame };

export function playedFrame(runId: string, seq: number, part: number, atWallMs: number): VoicePlayedFrame {
  return { played: { run_id: runId, seq, part, at_wall_ms: Math.round(atWallMs) } };
}

export function stageDroppedFrame(runId: string, seq: number): VoiceStageDroppedFrame {
  return { stage_dropped: { run_id: runId, seq, reason: "stage_cap" } };
}

/**
 * The face state for the drawn form (`null`: nothing drawn). Only the daemon's real layer rig is
 * `rig`; the procedural placeholder is a stand-in and reports `silhouette`, so a watched check of
 * `rig` means Malakor's real face was on screen.
 */
export function faceStateOf(form: "rig" | "placeholder" | null): FaceState {
  return form === "rig" ? "rig" : "silhouette";
}

export function faceFrame(runId: string, state: FaceState, atWallMs: number): VoiceFaceFrame {
  return { face: { run_id: runId, state, at_wall_ms: Math.round(atWallMs) } };
}
