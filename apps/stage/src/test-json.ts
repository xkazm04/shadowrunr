// The tests' one JSON door: tests read what the stage wrote (socket frames, fixture lines) through
// readJson, so JSON.parse stays out of test bodies as it stays out of the app. The app's own doors
// are the two typed decoders, clip.ts and voice/frame.ts (eslint.config.js STAGE_DOORS).

/** Parse one JSON text a test produced or reads back; the caller narrows the result. */
export function readJson(text: string): unknown {
  return JSON.parse(text);
}
