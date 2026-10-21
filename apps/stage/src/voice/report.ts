// What the stage tells the daemon on the `/voice` socket, beside `{"follow":{...}}`: a part's
// audio started (`played`), the stage dropped a whole line to respect MAX_AHEAD_S
// (`stage_dropped`), and which face is drawn (`face`, on load and on every change). The daemon
// writes them to the run's `voice.jsonl`; one that ignores them loses nothing, since a report
// never changes the follow and the stage never waits for a reply.
//
// The types are local until `@shadowrunr/contract` generates `VoiceClientFrame` (owner: REC); the
// JSON shapes are the shared show contract's, key for key.

/** Which face the stage draws: the real layer rig, or a stand-in (the silhouette or the placeholder). */
export type FaceState = "rig" | "silhouette";

export interface VoicePlayedFrame {
  played: { run_id: string; seq: number; part: number; at_wall_ms: number };
}

export interface VoiceStageDroppedFrame {
  stage_dropped: { run_id: string; seq: number; reason: "stage_cap" };
}

export interface VoiceFaceFrame {
  face: { run_id: string; state: FaceState; at_wall_ms: number };
}

export type VoiceClientFrame = VoicePlayedFrame | VoiceStageDroppedFrame | VoiceFaceFrame;

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
