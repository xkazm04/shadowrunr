// Where a line sits in the session's recording: its real start when the voice was heard
// saying it, else its transcript time, moved onto the file's clock (`t - stream_offset_ms`).
import type { ClipRunRecording } from "@shadowrunr/contract";

/** The stream-clock ms a line starts at: its first `played`, else its transcript time. */
export function lineStartMs(rec: ClipRunRecording, line: number, transcriptMs: number): { ms: number; heard: boolean } {
  const h = rec.heard.find((x) => x.line === line);
  return h ? { ms: h.start_ms, heard: true } : { ms: transcriptMs, heard: false };
}

/** Seconds into the recording for a stream-clock time (never before its first frame). */
export const fileSeconds = (rec: ClipRunRecording, streamMs: number): number => Math.max(0, streamMs - rec.stream_offset_ms) / 1000;

/** The recording's URL on the daemon. */
export const mediaUrl = (daemonUrl: string, rec: ClipRunRecording): string => `${daemonUrl.replace(/\/+$/, "")}${rec.media_path}`;
