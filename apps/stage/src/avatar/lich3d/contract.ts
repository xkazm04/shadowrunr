// The lich asset as the lich3d face consumes it: `lich.glb` + `lich.json` beside the rig's
// `layers.json` (made offline by `tools/avatar3d`, spark avatar-3d WP-B1). Pure: no three.js here,
// so the checks run in the unit tests and cost the main chunk nothing.
//
// The GLB names six nodes: `skull`, `jaw` (its origin at the hinge, so a turn about its own X opens
// the mouth), `eye_l` and `eye_r` (socket centres, a little inside; `_l` is the viewer's left, as in
// the rig's `eye_l.png`), `monocle` (the ring's centre, over `eye_r`) and `neck` (the head's pivot
// under the skull). `lich.json` says how far the jaw opens, how big a socket is and where the
// camera stands so the head sits in the frame as the portrait has it.

/** The nodes a lich GLB must name, exactly. */
export const LICH_NODES = ["skull", "jaw", "eye_l", "eye_r", "monocle", "neck"] as const;
export type LichNode = (typeof LICH_NODES)[number];

/**
 * Nodes a lich GLB may name: `collar`, the high collar and shoulders (a mesh whose origin is its
 * pivot, near `neck`), turned a fraction of the head's turn. Absent means no node at all.
 */
export const LICH_OPTIONAL_NODES = ["collar"] as const;
export type LichOptionalNode = (typeof LICH_OPTIONAL_NODES)[number];

export type Vec3 = [number, number, number];

export interface LichCamera {
  /** Vertical field of view, degrees. */
  fov: number;
  pos: Vec3;
  target: Vec3;
}

export interface LichMeta {
  version: 1;
  source_model: string;
  portrait_sha256: string;
  tris: number;
  /** The widest the jaw opens, radians about its hinge. */
  jaw_open_rad: number;
  /** A socket's radius, scene units: how far the eye fire may wander in it. */
  eye_radius: number;
  camera: LichCamera;
}

const isObj = (x: unknown): x is Record<string, unknown> => typeof x === "object" && x !== null && !Array.isArray(x);
const finite = (x: unknown): x is number => typeof x === "number" && Number.isFinite(x);

function vec3(x: unknown, what: string): Vec3 {
  if (!Array.isArray(x) || x.length !== 3 || !x.every(finite)) throw new Error(`${what} is not three numbers`);
  return [x[0] as number, x[1] as number, x[2] as number];
}

/** Check a parsed `lich.json`. Throws with the first reason it is unusable. */
export function checkLichMeta(raw: unknown): LichMeta {
  if (!isObj(raw)) throw new Error("lich.json is not an object");
  if (raw.version !== 1) throw new Error(`lich.json version ${String(raw.version)} (want 1)`);
  if (typeof raw.source_model !== "string" || !raw.source_model) throw new Error("source_model is missing");
  if (typeof raw.portrait_sha256 !== "string" || !/^[0-9a-f]{64}$/i.test(raw.portrait_sha256)) throw new Error("portrait_sha256 is not a sha256");
  if (!finite(raw.tris) || !Number.isInteger(raw.tris) || raw.tris <= 0) throw new Error("tris is not a positive integer");
  if (!finite(raw.jaw_open_rad) || raw.jaw_open_rad <= 0 || raw.jaw_open_rad > 1.2) throw new Error("jaw_open_rad is outside (0, 1.2]");
  if (!finite(raw.eye_radius) || raw.eye_radius <= 0) throw new Error("eye_radius is not positive");
  const cam = raw.camera;
  if (!isObj(cam)) throw new Error("camera is missing");
  if (!finite(cam.fov) || cam.fov < 5 || cam.fov > 120) throw new Error("camera.fov is outside 5..120 degrees");
  const pos = vec3(cam.pos, "camera.pos");
  const target = vec3(cam.target, "camera.target");
  if (Math.hypot(pos[0] - target[0], pos[1] - target[1], pos[2] - target[2]) < 1e-6) throw new Error("camera.pos is its target");
  return {
    version: 1,
    source_model: raw.source_model,
    portrait_sha256: raw.portrait_sha256,
    tris: raw.tris,
    jaw_open_rad: raw.jaw_open_rad,
    eye_radius: raw.eye_radius,
    camera: { fov: cam.fov, pos, target },
  };
}

/** The contract's nodes a model lacks (none: it can be drawn). */
export function missingNodes(has: (name: LichNode) => boolean): LichNode[] {
  return LICH_NODES.filter((n) => !has(n));
}

/**
 * The procedural placeholder's numbers (`skull.ts`): a skull one unit tall centred on the origin,
 * framed as the M8 portrait frames it (the skull is 470 of the portrait's 1024 px, its centre at
 * (511, 392)), so the rig's torso card lines up behind it.
 */
export const PLACEHOLDER_META: LichMeta = Object.freeze({
  version: 1,
  source_model: "procedural-placeholder",
  portrait_sha256: "0".repeat(64),
  tris: 0,
  jaw_open_rad: 0.32,
  eye_radius: 0.12,
  camera: Object.freeze({ fov: 30, pos: [0, -0.26, 4.07] as Vec3, target: [0, -0.26, 0] as Vec3 }),
}) as LichMeta;

/** Which skull the face draws, and why not the GLB when it is the placeholder. */
export type SkullChoice = { use: "glb" } | { use: "placeholder"; why: string };

/**
 * The asset decision: the GLB when its json checks and it names every node, else the placeholder
 * with the reason (said once in the console).
 */
export function chooseSkull(input: { rigBase: string | null; meta?: unknown; metaError?: string; glbError?: string; missing?: readonly string[] }): SkullChoice {
  if (!input.rigBase) return { use: "placeholder", why: "no rig base" };
  if (input.metaError) return { use: "placeholder", why: `lich.json: ${input.metaError}` };
  if (input.glbError) return { use: "placeholder", why: `lich.glb: ${input.glbError}` };
  try {
    checkLichMeta(input.meta);
  } catch (error) {
    return { use: "placeholder", why: `lich.json: ${error instanceof Error ? error.message : String(error)}` };
  }
  if (input.missing && input.missing.length > 0) return { use: "placeholder", why: `lich.glb lacks ${input.missing.join(", ")}` };
  return { use: "glb" };
}

/** Whether the face is drawn in three.js or handed to the glow face (which shows the silhouette). */
export function chooseRenderer(env: { webgl2: boolean; rendererError?: string }): { use: "lich3d" } | { use: "glow"; why: string } {
  if (!env.webgl2) return { use: "glow", why: "no WebGL2 for the 3D head" };
  if (env.rendererError) return { use: "glow", why: `the 3D head did not start: ${env.rendererError}` };
  return { use: "lich3d" };
}
