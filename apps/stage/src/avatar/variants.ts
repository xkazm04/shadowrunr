// The face renderers the stage can draw. `glow` is the shipped rig renderer (`./runtime`); `cut`
// (a 2.5D cut-out of the same rig) and `depth` (a depth-mesh head) are the streamer-screen
// prototypes, each in its own folder behind the same `startFace` signature, loaded with `import()`
// so a variant costs nothing until it is chosen.

import type { FaceHandle, startFace as glowStartFace } from "./runtime";

export const FACE_VARIANTS = ["glow", "cut", "depth"] as const;
export type FaceVariant = (typeof FACE_VARIANTS)[number];

/**
 * Where Malakor looks, relative to his own face box: `x` -1 (his box's left edge of the canvas
 * side) .. 1 (right), `y` -1 (up) .. 1 (down); `{x: 0, y: 0}` is straight at the viewer. Read every
 * frame; a renderer eases toward it and may ignore it.
 */
export interface Look {
  x: number;
  y: number;
}

export interface LookSource {
  get(): Look;
}

export type StartFaceOptions = Parameters<typeof glowStartFace>[0] & { look?: LookSource };
export type StartFace = (opts: StartFaceOptions) => FaceHandle;

/** The variant's `startFace`, loaded on first use. */
export function loadVariant(variant: FaceVariant): Promise<StartFace> {
  switch (variant) {
    case "glow":
      return import("./runtime").then((m) => m.startFace);
    case "cut":
      return import("./cut").then((m) => m.startFace);
    case "depth":
      return import("./depth").then((m) => m.startFace);
  }
}
