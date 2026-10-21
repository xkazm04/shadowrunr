// The depth face's pure parts: the depth map's metadata and sampling, the displaced meshes, the
// camera (the same projection the vertex shader does, in JS, so it can be tested), the tearing
// fade and the canvas fit. No WebGL here.

import type { LoadedForm } from "../layers";
import type { Gray16 } from "./png16";

/** `depth.json`, as `tools/avatar/depth_map.py` writes it. */
export interface DepthMeta {
  version: number;
  method: string;
  width: number;
  height: number;
  /** Source px toward the viewer of the brightest and the darkest level. */
  near: number;
  far: number;
  /** Each floating part's plane depth, source px. */
  planes: Record<string, number>;
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const isNum = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);

/** Check `depth.json` against the rig it sits beside. Throws with the reason. */
export function checkDepthMeta(raw: unknown, width: number, height: number): DepthMeta {
  if (!isObj(raw)) throw new Error("depth.json is not an object");
  if (raw.version !== 1) throw new Error(`depth.json is version ${String(raw.version)}, not 1`);
  if (raw.width !== width || raw.height !== height) throw new Error(`depth.json is ${String(raw.width)}x${String(raw.height)}, the rig ${width}x${height}`);
  if (!isNum(raw.near) || !isNum(raw.far) || raw.near <= raw.far) throw new Error("depth.json's near/far are malformed");
  const planes: Record<string, number> = {};
  if (raw.planes !== undefined) {
    if (!isObj(raw.planes)) throw new Error("depth.json's planes are malformed");
    for (const [k, v] of Object.entries(raw.planes)) if (isNum(v)) planes[k] = v;
  }
  return { version: 1, method: typeof raw.method === "string" ? raw.method : "unknown", width, height, near: raw.near, far: raw.far, planes };
}

/** A depth map in source px toward the viewer, row-major. */
export interface DepthField {
  width: number;
  height: number;
  z: Float32Array;
}

/** Levels to source px: level 65535 is `near`, 0 is `far`. */
export function depthField(png: Gray16, meta: Pick<DepthMeta, "near" | "far">): DepthField {
  const z = new Float32Array(png.data.length);
  const k = (meta.near - meta.far) / 65535;
  for (let i = 0; i < z.length; i++) z[i] = meta.far + png.data[i]! * k;
  return { width: png.width, height: png.height, z };
}

/** The depth at source point (x, y), bilinear, clamped to the map. */
export function sampleDepth(f: DepthField, x: number, y: number): number {
  const fx = Math.min(f.width - 1, Math.max(0, x - 0.5));
  const fy = Math.min(f.height - 1, Math.max(0, y - 0.5));
  const x0 = Math.floor(fx);
  const y0 = Math.floor(fy);
  const x1 = Math.min(f.width - 1, x0 + 1);
  const y1 = Math.min(f.height - 1, y0 + 1);
  const tx = fx - x0;
  const ty = fy - y0;
  const r0 = f.z[y0 * f.width + x0]! * (1 - tx) + f.z[y0 * f.width + x1]! * tx;
  const r1 = f.z[y1 * f.width + x0]! * (1 - tx) + f.z[y1 * f.width + x1]! * tx;
  return r0 * (1 - ty) + r1 * ty;
}

/** Floats per vertex: source x, y; depth z; the depth's slope gx, gy; the largest step to a neighbour along x and y. */
export const STRIDE = 7;

export interface Mesh {
  /** Interleaved vertices (`STRIDE` floats each). */
  data: Float32Array;
  idx: Uint32Array;
  /** The grid's cell, source px (the larger side). */
  cell: number;
}

/**
 * A `nx` x `ny` grid over a box, each vertex displaced by `depthAt`. Each vertex carries its
 * depth's slope (for shading) and the largest depth step to its neighbours (for the tearing fade).
 */
export function displacedGrid(box: readonly [number, number, number, number], nx: number, ny: number, depthAt: (x: number, y: number) => number): Mesh {
  const [x, y, w, h] = box;
  const cx = w / nx;
  const cy = h / ny;
  const cols = nx + 1;
  const z = new Float32Array(cols * (ny + 1));
  for (let j = 0; j <= ny; j++) for (let i = 0; i <= nx; i++) z[j * cols + i] = depthAt(x + i * cx, y + j * cy);
  const at = (i: number, j: number) => z[Math.min(ny, Math.max(0, j)) * cols + Math.min(nx, Math.max(0, i))]!;
  const data = new Float32Array(cols * (ny + 1) * STRIDE);
  let k = 0;
  for (let j = 0; j <= ny; j++)
    for (let i = 0; i <= nx; i++) {
      const zc = at(i, j);
      const l = at(i - 1, j);
      const r = at(i + 1, j);
      const u = at(i, j - 1);
      const d = at(i, j + 1);
      data[k++] = x + i * cx;
      data[k++] = y + j * cy;
      data[k++] = zc;
      data[k++] = (r - l) / ((i > 0 && i < nx ? 2 : 1) * cx);
      data[k++] = (d - u) / ((j > 0 && j < ny ? 2 : 1) * cy);
      data[k++] = Math.max(Math.abs(r - zc), Math.abs(zc - l));
      data[k++] = Math.max(Math.abs(d - zc), Math.abs(zc - u));
    }
  const idx = new Uint32Array(nx * ny * 6);
  k = 0;
  for (let j = 0; j < ny; j++)
    for (let i = 0; i < nx; i++) {
      const a = j * cols + i;
      const b = a + 1;
      const c = a + cols;
      const e = c + 1;
      idx[k++] = a;
      idx[k++] = b;
      idx[k++] = c;
      idx[k++] = b;
      idx[k++] = e;
      idx[k++] = c;
    }
  return { data, idx, cell: Math.max(cx, cy) };
}

/** The union of boxes (x, y, w, h). */
export function unionBox(boxes: readonly (readonly [number, number, number, number])[]): [number, number, number, number] {
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  for (const [x, y, w, h] of boxes) {
    x0 = Math.min(x0, x);
    y0 = Math.min(y0, y);
    x1 = Math.max(x1, x + w);
    y1 = Math.max(y1, y + h);
  }
  return [x0, y0, x1 - x0, y1 - y0];
}

/**
 * The camera: a pinhole at `distance` source px in front of the art's plane (z = 0), looking at
 * the art's centre. Each vertex is lifted off the picture by its depth so that, unturned, it
 * projects back onto the very pixel it came from: the rest frame is the art.
 */
export interface Camera {
  cx: number;
  cy: number;
  distance: number;
  /** The head turns about (pivotX, pivotY) on the picture, `pivotZ` deep. */
  pivotX: number;
  pivotY: number;
  pivotZ: number;
}

/** The camera for a rig: about 25 degrees across the art. */
export function cameraFor(width: number, height: number, head: { center: [number, number]; pivot_y?: number; chin_y: number }, pivotZ: number): Camera {
  return { cx: width / 2, cy: height / 2, distance: 2.4 * Math.max(width, height), pivotX: head.center[0], pivotY: head.pivot_y ?? head.chin_y, pivotZ };
}

/** Rotate (x, y, z) by yaw (front toward +x), then pitch (front toward +y, down), then roll in the picture. */
export function rotate(x: number, y: number, z: number, yaw: number, pitch: number, roll: number): [number, number, number] {
  const x1 = x * Math.cos(yaw) + z * Math.sin(yaw);
  const z1 = -x * Math.sin(yaw) + z * Math.cos(yaw);
  const y1 = y * Math.cos(pitch) + z1 * Math.sin(pitch);
  const z2 = -y * Math.sin(pitch) + z1 * Math.cos(pitch);
  const c = Math.cos(roll);
  const s = Math.sin(roll);
  return [x1 * c - y1 * s, x1 * s + y1 * c, z2];
}

/**
 * Where source point (x, y) at depth `z` lands on the picture, turned by `w` of the angles (rad):
 * the vertex shader's projection, in JS. Returns the picture point and its depth after the turn.
 */
export function project(cam: Camera, x: number, y: number, z: number, yaw: number, pitch: number, roll: number, w = 1): [number, number, number] {
  const lift = (px: number, py: number, pz: number) => {
    const k = (cam.distance - pz) / cam.distance;
    return [(px - cam.cx) * k, (py - cam.cy) * k, pz] as const;
  };
  const p = lift(x, y, z);
  const q = lift(cam.pivotX, cam.pivotY, cam.pivotZ);
  const [rx, ry, rz] = rotate(p[0] - q[0], p[1] - q[1], p[2] - q[2], yaw * w, pitch * w, roll * w);
  const X = q[0] + rx;
  const Y = q[1] + ry;
  const Z = q[2] + rz;
  const k = cam.distance / (cam.distance - Z);
  return [cam.cx + X * k, cam.cy + Y * k, Z];
}

/**
 * How much of a triangle still shows when a turn stretches it across a depth step: a step of
 * `dzx` (along x) slides apart by about `dzx * sin(yaw)` under a yaw, and likewise for pitch.
 * Up to 1.5 cells of stretch it shows in full (a steep but real slope, the side of the skull
 * coming into view, stays); by 3 cells it is gone, so a discontinuity opens as a soft gap instead
 * of a smear of stretched texels.
 */
export function tearFade(dzx: number, dzy: number, cell: number, yaw: number, pitch: number): number {
  const stretch = (Math.abs(dzx * Math.sin(yaw)) + Math.abs(dzy * Math.sin(pitch))) / Math.max(1e-3, cell);
  const t = Math.min(1, Math.max(0, (stretch - 1.5) / 1.5));
  return 1 - t * t * (3 - 2 * t);
}

/**
 * Fit the art into the canvas: `zoom` of the box, centred across and standing on the bottom
 * edge, so a turned head and the crown keep a margin above. Returns `u_view` (scale x, scale y,
 * offset x, offset y) mapping source px to clip space.
 */
export function fitView(cw: number, ch: number, width: number, height: number, zoom = 0.94): Float32Array {
  const s = (Math.min(cw / width, ch / height) * zoom) || 1;
  const ox = cw / 2 - (width / 2) * s;
  const oy = ch - height * s;
  return new Float32Array([(2 * s) / cw, (-2 * s) / ch, (2 * ox) / cw - 1, 1 - (2 * oy) / ch]);
}

/** The head's grid, cells a side, and the torso's. */
export const HEAD_CELLS = 128;
const TORSO_CELLS: [number, number] = [64, 36];

/** The layer kinds drawn on the head's grid. */
const HEAD_GROUP = new Set(["head", "aperture_upper", "aperture_lower", "aperture_glow", "eye", "seam", "pulse"]);

/** The meshes for a rig: [0] the head grid, [1] the torso grid, then one card per floating part; and which mesh each layer draws on. */
export function rigMeshes(form: LoadedForm, field: DepthField, meta: DepthMeta) {
  const { spec } = form;
  const W = spec.width;
  const H = spec.height;
  const R = spec.rig;
  const ap = R.aperture;
  const at = (x: number, y: number) => sampleDepth(field, x, y);
  const headBoxes = spec.layers.filter((l) => HEAD_GROUP.has(l.kind) && l.bbox).map((l) => l.bbox!);
  if (ap) headBoxes.push([ap.x0 - 10, ap.top - 20, ap.x1 - ap.x0 + 20, Math.min(H, ap.bottom) - ap.top + 40]);
  const hb = headBoxes.length ? unionBox(headBoxes) : ([R.head.center[0] - R.head.radius[0], R.head.center[1] - R.head.radius[1], 2 * R.head.radius[0], 2 * R.head.radius[1]] as [number, number, number, number]);
  const headBox: [number, number, number, number] = [hb[0], hb[1], hb[2], Math.min(hb[3], H - hb[1])];
  const meshes: Mesh[] = [displacedGrid(headBox, HEAD_CELLS, HEAD_CELLS, at)];
  const body = spec.layers.find((l) => l.kind === "body");
  meshes.push(displacedGrid(body?.bbox ?? [0, 0, W, H], TORSO_CELLS[0], TORSO_CELLS[1], at));
  const which = new Map<string, number>();
  for (const l of spec.layers) {
    if (HEAD_GROUP.has(l.kind)) which.set(l.name, 0);
    else if (l.kind === "body") which.set(l.name, 1);
    else if (l.kind === "float") {
      const bb = l.bbox ?? [0, 0, W, H];
      // a little margin, so a card's edge never cuts its texture
      const box: [number, number, number, number] = [bb[0] - 2, bb[1] - 2, bb[2] + 4, bb[3] + 4];
      const z = meta.planes[l.name] ?? at(bb[0] + bb[2] / 2, bb[1] + bb[3] / 2);
      which.set(l.name, meshes.length);
      meshes.push(displacedGrid(box, 2, 2, () => z));
    }
  }
  return { meshes, which, headBox };
}
