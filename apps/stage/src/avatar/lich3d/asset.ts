// Loading what the lich3d face draws: the head (`lich.glb` + `lich.json` beside the rig, else the
// procedural placeholder) and the painted torso card (`torso.png`, placed by its `layers.json` box).
// Every failure here falls back, said once by the caller: the face never fails for want of an asset.

import * as THREE from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";

import { chooseSkull, checkLichMeta, missingNodes, PLACEHOLDER_META } from "./contract";
import type { Backdrop, LichAsset } from "./renderer";
import { buildPlaceholderSkull } from "./skull";

type Get = (url: string) => Promise<Response>;
const defaultGet: Get = (u) => fetch(u);

/** A placeholder material: the renderer replaces it, so only vertex colours matter here. */
function placeholderAsset(): LichAsset {
  const bone = new THREE.MeshStandardMaterial({ vertexColors: true });
  return { kind: "placeholder", root: buildPlaceholderSkull(bone), meta: PLACEHOLDER_META };
}

/**
 * The head for the rig at `rigBase`: the GLB when its json checks and it names every node, else the
 * placeholder, with `why` saying what was wrong with the GLB.
 */
export async function loadLichAsset(rigBase: string | null, get: Get = defaultGet): Promise<LichAsset & { why?: string }> {
  if (!rigBase) return { ...placeholderAsset(), why: "no rig base" };
  let meta: unknown;
  let metaError: string | undefined;
  let glbError: string | undefined;
  let scene: THREE.Object3D | null = null;
  try {
    const [mr, gr] = await Promise.all([get(`${rigBase}lich.json`), get(`${rigBase}lich.glb`)]);
    if (!mr.ok) metaError = `HTTP ${mr.status}`;
    else meta = await mr.json();
    if (!gr.ok) glbError = `HTTP ${gr.status}`;
    else if (!metaError) {
      const gltf = await new GLTFLoader().parseAsync(await gr.arrayBuffer(), rigBase);
      scene = gltf.scene;
    }
  } catch (error) {
    glbError = error instanceof Error ? error.message : String(error);
  }
  const missing = scene ? missingNodes((n) => !!scene.getObjectByName(n)) : [];
  const choice = chooseSkull({ rigBase, meta, ...(metaError ? { metaError } : {}), ...(glbError ? { glbError } : {}), missing });
  if (choice.use === "placeholder" || !scene) return { ...placeholderAsset(), why: choice.use === "placeholder" ? choice.why : "lich.glb did not load" };
  return { kind: "glb", root: scene, meta: checkLichMeta(meta) };
}

interface RigLayer {
  name?: unknown;
  kind?: unknown;
  file?: unknown;
  bbox?: unknown;
}

/** The torso card from the rig's `layers.json`, or null (no rig, no torso layer, no picture). */
export async function loadBackdrop(rigBase: string | null, get: Get = defaultGet): Promise<Backdrop | null> {
  if (!rigBase) return null;
  try {
    const res = await get(`${rigBase}layers.json`);
    if (!res.ok) return null;
    const spec = (await res.json()) as { width?: unknown; height?: unknown; layers?: unknown };
    const layers = Array.isArray(spec.layers) ? (spec.layers as RigLayer[]) : [];
    const torso = layers.find((l) => l.kind === "body" && typeof l.file === "string");
    const bbox = torso?.bbox;
    if (!torso || !Array.isArray(bbox) || bbox.length !== 4 || typeof spec.width !== "number" || typeof spec.height !== "number") return null;
    const texture = await new THREE.TextureLoader().setCrossOrigin("anonymous").loadAsync(`${rigBase}${String(torso.file)}`);
    return { texture, bbox: bbox as [number, number, number, number], width: spec.width, height: spec.height };
  } catch {
    // a missing torso costs the backdrop, not the face
    return null;
  }
}
