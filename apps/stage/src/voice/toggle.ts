// The Voice toggle's memory and the rule for when the stage may sound at all.
//
// Off by default and remembered per browser in localStorage (read and written inside try/catch:
// a private window or blocked storage just means "off"). The voice is live only: a replay is not
// voiced. A headless browser (the clip recorder, any automation) never sounds, whatever is stored.

export const VOICE_KEY = "shadowrunr.stage.voice";

export type StorageLike = Pick<Storage, "getItem" | "setItem">;

/** The page's localStorage, or `null` when it cannot be reached. */
export function pageStorage(): StorageLike | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

export function readVoiceOn(storage: StorageLike | null): boolean {
  try {
    return storage?.getItem(VOICE_KEY) === "1";
  } catch {
    return false;
  }
}

export function writeVoiceOn(storage: StorageLike | null, on: boolean): void {
  try {
    storage?.setItem(VOICE_KEY, on ? "1" : "0");
  } catch {
    // storage blocked: the toggle still works for this page
  }
}

/** Why the voice is off right now, or `null` when it may play. */
export function voiceBlocked(opts: { live: boolean; on: boolean; headless: boolean }): string | null {
  if (opts.headless) return "a headless browser is never voiced";
  if (!opts.live) return "replays are not voiced";
  if (!opts.on) return "the Voice toggle is off";
  return null;
}
