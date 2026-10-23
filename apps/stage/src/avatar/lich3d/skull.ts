// The procedural placeholder skull: what the lich3d face draws until (or unless) the rig has a
// usable `lich.glb`. A faceted low-poly skull in M8's style, built from primitives in code and named
// as the GLB contract names its nodes (`contract.ts`), so the renderer drives either the same way.
//
// One unit tall about the origin, face toward +z, up +y, framed by PLACEHOLDER_META's camera as the
// portrait frames the head. The cranium is a deformed icosphere: real dents for the sockets and the
// nasal cavity (darkened by vertex colour, lit by the eye fire), ridges for the brow and the
// cheekbones, a flat palate at the teeth line. The jaw is a separate horseshoe with its rami up to
// the hinge, its origin at the hinge. A per-vertex jitter keyed by position chips the surface into
// facets without tearing shared edges.

import * as THREE from "three";
import { mergeGeometries } from "three/addons/utils/BufferGeometryUtils.js";

import { mulberry32 } from "../driver";

const smooth = (a: number, b: number, x: number) => {
  const t = Math.min(1, Math.max(0, (x - a) / (b - a)));
  return t * t * (3 - 2 * t);
};

/** A stable pseudo-random value per position, so welded vertices move together. */
function hash3(x: number, y: number, z: number): number {
  const s = Math.sin(Math.round(x * 1e4) * 12.9898 + Math.round(y * 1e4) * 78.233 + Math.round(z * 1e4) * 37.719) * 43758.5453;
  return s - Math.floor(s);
}

/** Keep only the position (and a colour), so primitives of any kind merge. */
function bare(geo: THREE.BufferGeometry): THREE.BufferGeometry {
  for (const k of Object.keys(geo.attributes)) if (k !== "position" && k !== "color") geo.deleteAttribute(k);
  return geo;
}

/** Socket centres (x, y) on the face plane, and their half sizes. `_l` is the viewer's left. */
export const SOCKETS = { l: [-0.205, 0.07], r: [0.205, 0.07], rx: 0.125, ry: 0.115, depth: 0.2 } as const;
const HINGE = new THREE.Vector3(0, -0.15, -0.08);
/** The face plane the brow, sockets, cheekbones and maxilla sit on. */
const FACE_Z = 0.36;

/** The cranium with the upper face: positions and a darkness (vertex colour) per vertex. */
function cranium(): THREE.BufferGeometry {
  // about 3900 facets: fine enough for real sockets, coarse enough to read as cut stone
  const geo = new THREE.IcosahedronGeometry(1, 13);
  const pos = geo.getAttribute("position") as THREE.BufferAttribute;
  const colors = new Float32Array(pos.count * 3);
  const d = new THREE.Vector3();
  for (let i = 0; i < pos.count; i++) {
    d.fromBufferAttribute(pos, i).normalize();
    const jitter = (hash3(d.x, d.y, d.z) - 0.5) * 0.007;
    let x = d.x * (0.42 + jitter);
    let y = 0.03 + d.y * (0.47 + jitter);
    let z = -0.05 + d.z * (0.46 + jitter);

    // the lower face narrows and the back of the head tucks under
    const low = smooth(-0.15, -0.9, d.y);
    x *= 1 - 0.12 * low;
    if (d.z < 0) z *= 1 - 0.35 * low;
    // temples hollow a little above the cheekbones
    x *= 1 - 0.07 * smooth(0.35, 0.0, Math.abs(d.y - 0.15)) * smooth(0.3, 0.8, Math.abs(d.x)) * smooth(-0.6, 0.2, d.z);

    // the zygomatic arches run back from the cheekbones toward the jaw's hinge
    const arch = Math.exp(-(((y + 0.1) / 0.07) ** 2)) * smooth(0.55, 0.85, Math.abs(d.x)) * smooth(-0.5, 0.0, d.z);
    x += Math.sign(d.x) * 0.035 * arch;

    let dark = 0;
    if (d.z > 0.1) {
      const front = smooth(0.1, 0.5, d.z);
      // the face is a plane, not a ball: flatten the front below the forehead
      const plane = smooth(0.45, 0.85, d.z) * smooth(0.45, 0.1, d.y);
      z += (Math.min(z, FACE_Z) - z) * plane;
      for (const sx of [-1, 1]) {
        // a heavy brow ridge over each socket, lower toward the nose: he scowls at rest
        const ax = Math.abs(x);
        const browY = 0.205 + 0.05 * smooth(0.06, 0.32, ax);
        const brow = Math.exp(-(((x - sx * 0.19) / 0.17) ** 2) - ((y - browY) / 0.05) ** 2);
        z += 0.05 * brow * front;
        // broad cheekbones under and outside the sockets
        const cheek = Math.exp(-(((x - sx * 0.29) / 0.1) ** 2) - ((y + 0.08) / 0.07) ** 2);
        z += 0.03 * cheek * front;
        x += sx * 0.04 * cheek;
      }
      // the sockets: steep dents, black toward the back
      for (const [cx, cy] of [SOCKETS.l, SOCKETS.r]) {
        // each socket tilts, its inner corner lower: the scowl the portrait has
        const a = Math.sign(cx) * 0.22;
        const u = (x - cx) * Math.cos(a) + (y - cy) * Math.sin(a);
        const v = -(x - cx) * Math.sin(a) + (y - cy) * Math.cos(a);
        const r2 = (u / SOCKETS.rx) ** 2 + (v / SOCKETS.ry) ** 2;
        if (r2 < 1) {
          const k = (1 - r2) ** 0.3;
          z -= SOCKETS.depth * k * front;
          dark = Math.max(dark, Math.min(1, k * 2.4));
        }
      }
      // the nasal cavity: an inverted heart between the cheekbones
      if (y < -0.02 && y > -0.2) {
        const halfW = 0.015 + 0.06 * smooth(-0.02, -0.16, y);
        const u = Math.abs(x) / halfW;
        if (u < 1) {
          const k = (1 - u * u) ** 0.4 * smooth(-0.2, -0.175, y) * smooth(-0.02, -0.045, y);
          z -= 0.12 * k * front;
          dark = Math.max(dark, k);
        }
      }
      // the maxilla comes forward to carry the upper teeth
      const max = Math.exp(-((x / 0.2) ** 2)) * smooth(-0.17, -0.24, y);
      z += 0.025 * max * front;
    }
    // a flat palate at the teeth line in front, the skull's base further back
    const floor = -0.285 - 0.15 * Math.max(smooth(0.3, -0.4, d.z), smooth(0.18, 0.32, Math.abs(x)) * 0.4);
    if (y < floor) y = floor;

    pos.setXYZ(i, x, y, z);
    const c = 0.93 - 0.9 * dark;
    colors[i * 3] = c;
    colors[i * 3 + 1] = c;
    colors[i * 3 + 2] = c;
  }
  // one shade per facet (the triangle's mean), so the sockets' edges are cut, not smeared
  for (let f = 0; f < colors.length; f += 9) {
    // and a little per-facet variation, as cut stone catches the light unevenly
    const mean = ((colors[f]! + colors[f + 3]! + colors[f + 6]!) / 3) * (0.94 + 0.1 * hash3(f, 0.5, 1.5));
    for (let k = 0; k < 9; k++) colors[f + k] = mean;
  }
  geo.setAttribute("color", new THREE.BufferAttribute(colors, 3));
  return bare(geo);
}

/** A row of teeth along the dental arc, tops at `top`, each `h` tall. */
function teeth(top: number, h: number, back: number, rnd: () => number): THREE.BufferGeometry {
  const parts: THREE.BufferGeometry[] = [];
  const n = 10;
  for (let i = 0; i < n; i++) {
    const x = ((i + 0.5) / n - 0.5) * 0.34;
    const w = 0.031 + rnd() * 0.004 - Math.abs(x) * 0.02;
    const th = h * (0.88 + rnd() * 0.16) * (1 - Math.abs(x) * 0.6);
    // a square prism turned 45 degrees and tapered to the root: a faceted tooth
    const box = bare(new THREE.CylinderGeometry(w * 0.72, w * 0.55, th, 4, 1).toNonIndexed());
    box.rotateY(Math.PI / 4);
    box.scale(1, 1, 0.75);
    if (top < -0.3) box.rotateX(Math.PI);
    const z = 0.335 - back - x * x * 2.6;
    box.rotateY(-x * 2.2);
    box.translate(x, top - th / 2, z);
    parts.push(box);
  }
  const merged = mergeGeometries(parts);
  for (const p of parts) p.dispose();
  return merged;
}

/** The mandible in skull space: a horseshoe body and two rami up to the hinge. */
function mandible(): THREE.BufferGeometry {
  const segs = 10;
  const ring: [THREE.Vector3, THREE.Vector3, THREE.Vector3, THREE.Vector3][] = [];
  for (let i = 0; i <= segs; i++) {
    const a = (i / segs - 0.5) * 2 * 1.35; // -77 .. 77 deg around the arc
    const s = Math.sin(a);
    const c = Math.cos(a);
    const out = new THREE.Vector3(0.27 * s, 0, -0.04 + 0.37 * c);
    const inn = new THREE.Vector3(0.19 * s, 0, -0.04 + 0.28 * c);
    const side = Math.abs(s);
    const top = -0.325 + 0.01 * side;
    // the chin is deepest at the front, with a squared jaw line toward the angles
    const bottom = -0.52 + 0.08 * side * side;
    ring.push([
      // the chin juts: the outer face leans out toward its foot at the front
      out.clone().setY(top).setZ(out.z - 0.02 * c),
      inn.clone().setY(top),
      inn.clone().setY(bottom + 0.03),
      out.clone().setY(bottom).setZ(out.z + 0.025 * c * c),
    ]);
  }
  const verts: number[] = [];
  const quad = (a: THREE.Vector3, b: THREE.Vector3, c: THREE.Vector3, d: THREE.Vector3) => verts.push(a.x, a.y, a.z, b.x, b.y, b.z, c.x, c.y, c.z, a.x, a.y, a.z, c.x, c.y, c.z, d.x, d.y, d.z);
  for (let i = 0; i < segs; i++) {
    const p = ring[i]!;
    const q = ring[i + 1]!;
    for (let k = 0; k < 4; k++) {
      const k1 = (k + 1) % 4;
      quad(p[k]!, p[k1]!, q[k1]!, q[k]!);
    }
  }
  const body = new THREE.BufferGeometry();
  body.setAttribute("position", new THREE.Float32BufferAttribute(verts, 3));
  const parts: THREE.BufferGeometry[] = [body];
  // the rami: slabs from the angle of the jaw up to the condyles
  for (const sx of [-1, 1]) {
    const ramus = bare(new THREE.BoxGeometry(0.045, 0.3, 0.1).toNonIndexed());
    ramus.rotateX(-0.3);
    ramus.translate(sx * 0.225, -0.28, -0.13);
    parts.push(ramus);
  }
  const merged = mergeGeometries(parts);
  for (const p of parts) p.dispose();
  return merged;
}

function painted(geo: THREE.BufferGeometry, shade: number): THREE.BufferGeometry {
  const n = geo.getAttribute("position").count;
  const colors = new Float32Array(n * 3);
  for (let f = 0; f < colors.length; f += 9) colors.fill(shade * (0.94 + 0.1 * hash3(f, 2.5, shade)), f, f + 9);
  geo.setAttribute("color", new THREE.BufferAttribute(colors, 3));
  return geo;
}

/**
 * Build the placeholder: a group holding `skull`, `jaw`, `eye_l`, `eye_r`, `monocle` and `neck`,
 * the meshes sharing `bone` (which must read vertex colours). The caller owns the geometries.
 */
export function buildPlaceholderSkull(bone: THREE.Material, seed = 8): THREE.Group {
  const rnd = mulberry32(seed);
  const group = new THREE.Group();
  group.name = "lich-placeholder";

  const upper = mergeGeometries([cranium(), painted(teeth(-0.262, 0.06, 0, rnd), 0.8)]);
  upper.computeVertexNormals();
  const skull = new THREE.Mesh(upper, bone);
  skull.name = "skull";
  group.add(skull);

  const lower = mergeGeometries([painted(mandible(), 0.85), painted(teeth(-0.312, 0.055, 0.012, rnd), 0.78)]);
  lower.translate(-HINGE.x, -HINGE.y, -HINGE.z);
  lower.computeVertexNormals();
  const jaw = new THREE.Mesh(lower, bone);
  jaw.name = "jaw";
  jaw.position.copy(HINGE);
  group.add(jaw);

  const anchor = (name: string, x: number, y: number, z: number) => {
    const o = new THREE.Object3D();
    o.name = name;
    o.position.set(x, y, z);
    group.add(o);
  };
  anchor("eye_l", SOCKETS.l[0], SOCKETS.l[1], 0.22);
  anchor("eye_r", SOCKETS.r[0], SOCKETS.r[1], 0.22);
  anchor("monocle", SOCKETS.r[0], SOCKETS.r[1], 0.34);
  anchor("neck", 0, -0.36, -0.12);
  return group;
}
