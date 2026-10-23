// STUB (P0, spark avatar-3d): the lich3d face draws the glow face until its work package lands.

import { startFace as startGlow, type FaceHandle } from "../runtime";
import type { StartFaceOptions } from "../variants";

export function startFace(opts: StartFaceOptions): FaceHandle {
  return startGlow(opts);
}
