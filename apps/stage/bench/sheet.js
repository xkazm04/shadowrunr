// The expressiveness sheet: each face's renderer held at one channel per row, side by side. The
// renderers are driven directly with an explicit pose, look and performance (the same values for
// every face), each through the face's own look mapping, so a row shows how far that face goes for
// the same request. Loaded lazily by harness.js; dev server only.
//
// The look rows are a full look (|x| or |y| = 1), the furthest the stage asks; `limits` lists, per
// face, the head turn that gives and the hard cap the face clamps to (driver and gestures included).

import { CUT_LOOK, withLook } from "../src/avatar/cut/look.ts";
import { createCutRenderer } from "../src/avatar/cut/renderer.ts";
import { fetchDepth } from "../src/avatar/depth/index.ts";
import { createLookFollower, HEAD_PITCH_MAX, HEAD_YAW_MAX } from "../src/avatar/depth/look.ts";
import { createDepthRenderer } from "../src/avatar/depth/renderer.ts";
import { createDriver } from "../src/avatar/driver.ts";
import { loadForm } from "../src/avatar/layers.ts";
import { loadBackdrop, loadLichAsset } from "../src/avatar/lich3d/asset.ts";
import { LOOK_GAIN as LICH_GAIN, lichFrame, PITCH_MAX_DEG, YAW_MAX_DEG } from "../src/avatar/lich3d/channels.ts";
import { createLichRenderer } from "../src/avatar/lich3d/renderer.ts";
import { createPerformanceDriver, PERFORMANCE_REST } from "../src/avatar/performance.ts";
import { buildPlaceholder } from "../src/avatar/placeholder.ts";
import { LOOK_GAIN as RELIT_GAIN, PITCH_LIMIT, YAW_LIMIT } from "../src/avatar/relit/geom.ts";
import { createRelitRenderer } from "../src/avatar/relit/renderer.ts";

const DEG = 180 / Math.PI;
const BG = "#060807";
const FACES = ["cut", "depth", "relit", "lich3d"];

/** The rows: a label and what differs from rest (pose fields, a look, performance fields, or a gesture at its peak). */
export const ROWS = [
  ["rest", {}],
  ["yaw left (look x -1)", { look: { x: -1, y: 0 } }],
  ["yaw right (look x +1)", { look: { x: 1, y: 0 } }],
  ["pitch up (look y -1)", { look: { x: 0, y: -1 } }],
  ["pitch down (look y +1)", { look: { x: 0, y: 1 } }],
  ["jaw open (open 1)", { pose: { open: 1, voice: 0.8 } }],
  ["eyeLight 0.3", { perf: { eyeLight: 0.3 } }],
  ["eyeLight 2.5", { perf: { eyeLight: 2.5 } }],
  ["brow -1", { perf: { brow: -1 } }],
  ["brow +1", { perf: { brow: 1 } }],
  ["flare (peak, 0.05 s)", { gesture: "flare", at: 0.05 }],
  ["lean_in (peak, 0.55 s)", { gesture: "lean_in", at: 0.55 }],
];

/** Head turn at a full look and the hard cap, degrees, per face (read from each face's own constants). */
export const LIMITS = {
  cut: { lookYaw: CUT_LOOK.lookYaw, lookPitch: CUT_LOOK.lookPitch, capYaw: CUT_LOOK.yawMax, capPitch: CUT_LOOK.pitchMax },
  // depth/renderer.ts clamps at 22 x 12 (its YAW_LIMIT, PITCH_LIMIT are module-private)
  depth: { lookYaw: HEAD_YAW_MAX, lookPitch: HEAD_PITCH_MAX, capYaw: 22, capPitch: 12 },
  relit: { lookYaw: HEAD_YAW_MAX * RELIT_GAIN[0], lookPitch: HEAD_PITCH_MAX * RELIT_GAIN[1], capYaw: YAW_LIMIT, capPitch: PITCH_LIMIT },
  lich3d: { lookYaw: HEAD_YAW_MAX * LICH_GAIN, lookPitch: HEAD_PITCH_MAX * LICH_GAIN, capYaw: YAW_MAX_DEG, capPitch: PITCH_MAX_DEG },
};

/** The performance pose for a row: rest with its fields, or a gesture's real envelope at `at` s. */
function perfFor(row) {
  if (!row.gesture) return { ...PERFORMANCE_REST, ...row.perf };
  const d = createPerformanceDriver();
  d.setCue({ id: 1, emotion: "neutral", intensity: 1, gesture: row.gesture });
  let p = d.pose;
  for (let t = 0; t < row.at - 1e-9; t += 1 / 120) p = d.step(Math.min(1 / 120, row.at - t));
  return { ...p };
}

/** The glow-family pose (cut, depth) carries the performance as withPerformance hangs it. */
function hang(pose, perf) {
  pose.perf = perf;
  pose.yaw += perf.gestureYaw * DEG;
  pose.pitch += perf.gesturePitch * DEG;
  pose.roll += perf.gestureRoll * DEG;
  return pose;
}

/** Each face's draw for one row, given its renderer. */
const DRAW = {
  cut(r, row) {
    const look = row.look ?? { x: 0, y: 0 };
    const pose = withLook(createDriver({ seed: 7 }), { get: () => look }).rest();
    Object.assign(pose, row.pose);
    r.render(hang(pose, perfFor(row)));
  },
  depth(r, row) {
    const pose = Object.assign(createDriver({ seed: 7 }).rest(), row.pose);
    r.render(hang(pose, perfFor(row)), createLookFollower().snap(row.look ?? { x: 0, y: 0 }));
  },
  relit(r, row) {
    const pose = Object.assign(createDriver({ seed: 7 }).rest(), row.pose);
    r.render(pose, createLookFollower().snap(row.look ?? { x: 0, y: 0 }), perfFor(row));
  },
  lich3d(r, row, asset) {
    const pose = Object.assign(createDriver({ seed: 7 }).rest(), row.pose);
    r.render(lichFrame(pose, perfFor(row), createLookFollower().snap(row.look ?? { x: 0, y: 0 }), asset.meta.jaw_open_rad));
  },
};

async function makeRenderer(face, canvas, rig, form) {
  if (face === "cut") return { r: await createCutRenderer(canvas, form) };
  if (face === "lich3d") {
    const [asset, backdrop] = await Promise.all([loadLichAsset(rig), loadBackdrop(rig)]);
    return { r: createLichRenderer(canvas, asset, { bloom: true, texture: true, backdrop }), asset };
  }
  const { meta, field } = await fetchDepth(rig, form.spec.width, form.spec.height);
  if (face === "depth") return { r: await createDepthRenderer(canvas, form, field, meta) };
  return { r: await createRelitRenderer(canvas, form, field, meta) };
}

/**
 * Draw the sheet: rows x faces, each cell `cell` px, faces rendered at `size` px. `order` names
 * the columns (default the four faces). Returns a PNG data URL and the limits.
 */
export async function sheet({ rig, size = 440, cell = 300, order = FACES } = {}) {
  const form = await loadForm(rig, buildPlaceholder);
  if (form.form !== "rig") throw new Error("the rig did not load");
  const label = 230;
  const head = 64;
  const out = Object.assign(document.createElement("canvas"), { width: label + cell * order.length, height: head + cell * ROWS.length });
  const c = out.getContext("2d");
  c.fillStyle = "#111514";
  c.fillRect(0, 0, out.width, out.height);
  c.fillStyle = "#f2efe6";
  c.font = "600 22px sans-serif";
  c.textBaseline = "middle";
  order.forEach((f, j) => {
    const L = LIMITS[f];
    c.fillText(f, label + j * cell + 12, head * 0.32);
    c.font = "15px sans-serif";
    c.fillText(`look ${L.lookYaw.toFixed(1)}/${L.lookPitch.toFixed(1)} deg, cap ${L.capYaw}/${L.capPitch}`, label + j * cell + 12, head * 0.72);
    c.font = "600 22px sans-serif";
  });
  c.font = "18px sans-serif";
  ROWS.forEach(([name], i) => c.fillText(name, 12, head + i * cell + cell / 2));

  for (const [j, face] of order.entries()) {
    const canvas = Object.assign(document.createElement("canvas"), { width: size, height: size });
    const { r, asset } = await makeRenderer(face, canvas, rig, form);
    for (const [i, [, row]] of ROWS.entries()) {
      // twice: three.js finishes its first-frame uploads on the first, and the bench keeps the second
      DRAW[face](r, row, asset);
      DRAW[face](r, row, asset);
      c.fillStyle = BG;
      c.fillRect(label + j * cell, head + i * cell, cell, cell);
      c.drawImage(canvas, label + j * cell, head + i * cell, cell, cell);
    }
    r.dispose();
  }
  c.strokeStyle = "#2a302e";
  for (let j = 0; j <= order.length; j++) c.strokeRect(label + j * cell, 0, 0, out.height);
  for (let i = 0; i <= ROWS.length; i++) c.strokeRect(0, head + i * cell, out.width, 0);
  return { url: out.toDataURL("image/png"), limits: LIMITS, rows: ROWS.map(([n]) => n) };
}
