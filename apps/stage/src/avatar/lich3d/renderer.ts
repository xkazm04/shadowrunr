// The lich3d renderer: one three.js scene that draws a `LichFrame`. Pure draw: it owns no loop and
// no clock (the face's loop calls `render`, a capture tool can call it at any frame).
//
// The scene, back to front: the rig's painted torso as a backdrop card (drawn first, never
// occluding), then the head on a pivot at `neck`: the bone
// (flat-shaded, rough, warm ivory, or the GLB's own texture), the jaw on its hinge, an eye fire in
// each socket (an additive flame card turned to the camera) with a teal point light inside that
// lights the bone around it, the orange monocle ring with its own light, and the crown of
// procedural crystals (dark, glossy, teal veins) floating about the head. Bloom picks up what
// burns above its threshold (eye fire, monocle, veins), then ACES tone mapping to sRGB.

import * as THREE from "three";
import { EffectComposer } from "three/addons/postprocessing/EffectComposer.js";
import { OutputPass } from "three/addons/postprocessing/OutputPass.js";
import { RenderPass } from "three/addons/postprocessing/RenderPass.js";
import { UnrealBloomPass } from "three/addons/postprocessing/UnrealBloomPass.js";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";

import { contextLossGuard } from "../gl-kit";
import type { LichFrame } from "./channels";
import type { LichMeta } from "./contract";
import { crystalAt, crystalCrown, type CrystalSpec } from "./crystals";

/** The head as loaded: a group holding the contract's nodes, and its numbers. */
export interface LichAsset {
  kind: "glb" | "placeholder";
  root: THREE.Object3D;
  meta: LichMeta;
}

/** The painted torso and where it sits in the portrait (px of a `width` x `height` portrait). */
export interface Backdrop {
  texture: THREE.Texture;
  bbox: [number, number, number, number];
  width: number;
  height: number;
}

/** How the bone is lit: `paint` banded like the portrait, `real` smooth (spark lich3d-quality). */
export type LichLook = "paint" | "real";

export interface LichRendererOptions {
  /** The bone's look (default `paint`). */
  look?: LichLook;
  /** Bloom on (the default) or off (a plain, antialiased draw). */
  bloom?: boolean;
  /** Use the GLB's own texture when it has one (the default); false draws the flat bone. */
  texture?: boolean;
  backdrop?: Backdrop | null;
  /** The crown's seed. */
  seed?: number;
  /** MSAA samples of the bloom path's scene target (default 4; the plain path uses the canvas's). */
  samples?: number;
}

export interface LichRenderer {
  render(frame: LichFrame): void;
  /** The canvas changed size. */
  resize(): void;
  /** Wait for the GPU to finish the frame (the harness's timing only). */
  finish(): void;
  readonly lost: boolean;
  /** three.js's own counts, for the bench. */
  info(): { calls: number; triangles: number; geometries: number; textures: number };
  dispose(): void;
}

const TEAL = new THREE.Color(0.2, 1.0, 0.82);
const TEAL_CORE = new THREE.Color(0.75, 1.0, 0.95);
const ORANGE = new THREE.Color(1.0, 0.45, 0.08);
const BONE = 0xc9b994;
/** Candela at eye light 1, scaled by the skull's height (a unit skull). */
const EYE_CANDELA = 0.05;
const MONOCLE_CANDELA = 0.025;
const BLOOM = { strength: 0.55, radius: 0.35, threshold: 0.85 };

const FLAME_VERT = /* glsl */ `
varying vec2 vUv;
void main() {
  vUv = uv;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}`;

// A teardrop of fire with a hot core, tongues rising and flickering, and a soft halo: additive.
const FLAME_FRAG = /* glsl */ `
uniform float uTime;
uniform float uIntensity;
uniform float uH;
uniform float uW;
uniform float uFlicker;
uniform float uSeed;
uniform vec3 uColor;
uniform vec3 uCore;
varying vec2 vUv;
void main() {
  vec2 p = (vUv * 2.0 - 1.0) * 1.6;
  float r = length(p);
  p.x /= uW;
  p.y /= uH;
  float t = uTime + uSeed;
  float rise = clamp(p.y + 0.6, 0.0, 2.0);
  float wob = (sin(p.y * 6.0 - t * 7.0) * 0.09 + sin(p.y * 13.0 - t * 12.0 + 1.3) * 0.045 * (1.0 + uFlicker * 2.0)) * rise;
  vec2 q = vec2(p.x + wob, p.y);
  float w = mix(0.5, 0.1, clamp((q.y + 0.6) / 1.5, 0.0, 1.0));
  float body = 1.0 - smoothstep(0.0, 1.0, length(vec2(q.x / w, (q.y + 0.2) / 0.85)));
  vec2 c = vec2(q.x * 3.4, (q.y + 0.3) * 2.6);
  float core = exp(-dot(c, c));
  float halo = exp(-r * r * 3.0) * 0.08;
  float flick = 1.0 + (sin(t * 17.0) * 0.5 + sin(t * 29.0 + 1.7) * 0.5) * 0.1 * (0.4 + uFlicker);
  vec3 col = (uColor * (body * 0.9 + halo) + uCore * core * 1.3) * uIntensity * flick;
  gl_FragColor = vec4(col, 1.0);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
}`;

function flameMaterial(seed: number): THREE.ShaderMaterial {
  return new THREE.ShaderMaterial({
    uniforms: {
      uTime: { value: 0 },
      uIntensity: { value: 1 },
      uH: { value: 1 },
      uW: { value: 1 },
      uFlicker: { value: 0 },
      uSeed: { value: seed },
      uColor: { value: TEAL.clone() },
      uCore: { value: TEAL_CORE.clone() },
    },
    vertexShader: FLAME_VERT,
    fragmentShader: FLAME_FRAG,
    blending: THREE.AdditiveBlending,
    transparent: true,
    depthWrite: false,
  });
}

/** Dark glossy stone with teal veins traced in its own space, brightest toward the base. */
function crystalMaterial(env: THREE.Texture | null, vein: { value: number }): THREE.MeshStandardMaterial {
  const m = new THREE.MeshStandardMaterial({ color: 0x0d1317, roughness: 0.2, metalness: 0.55, flatShading: true, envMap: env, envMapIntensity: 0.45 });
  m.onBeforeCompile = (shader) => {
    shader.uniforms.uVein = vein;
    shader.uniforms.uVeinColor = { value: new THREE.Color(0.1, 0.95, 0.78) };
    shader.vertexShader = shader.vertexShader
      .replace("#include <common>", "#include <common>\nvarying vec3 vCrystal;")
      .replace("#include <begin_vertex>", "#include <begin_vertex>\nvCrystal = position;");
    shader.fragmentShader = shader.fragmentShader
      .replace("#include <common>", "#include <common>\nvarying vec3 vCrystal;\nuniform float uVein;\nuniform vec3 uVeinColor;")
      .replace(
        "#include <emissivemap_fragment>",
        `#include <emissivemap_fragment>
        float vn = sin(vCrystal.y * 7.0 + sin(vCrystal.x * 9.0 + vCrystal.z * 7.0) * 1.8 + sin(vCrystal.z * 11.0) * 0.6);
        float vein = 1.0 - smoothstep(0.04, 0.22, abs(vn));
        float base = smoothstep(0.1, -0.5, vCrystal.y);
        totalEmissiveRadiance += uVeinColor * (vein * (0.3 + 0.7 * base) * 1.6 + base * base * 0.6) * uVein;`,
      );
  };
  return m;
}

/** The face's world bounding box over the meshes under `o`. */
function meshBox(o: THREE.Object3D): THREE.Box3 {
  const box = new THREE.Box3();
  o.updateWorldMatrix(true, true);
  o.traverse((c) => {
    if ((c as THREE.Mesh).isMesh) box.expandByObject(c);
  });
  return box;
}

export function createLichRenderer(canvas: HTMLCanvasElement, asset: LichAsset, opts: LichRendererOptions = {}): LichRenderer {
  const bloomOn = opts.bloom ?? true;
  const useTexture = opts.texture ?? true;
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: !bloomOn, alpha: false, powerPreference: "high-performance" });
  renderer.setPixelRatio(1);
  renderer.setSize(canvas.width, canvas.height, false);
  renderer.setClearColor(0x000000, 1);
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.0;
  renderer.outputColorSpace = THREE.SRGBColorSpace;

  // three.js restores its own GL state and re-uploads what it holds after a reset; the guard only
  // says when the face is dark and asks for a draw once it is back
  let lastFrame: LichFrame | null = null;
  const guard = contextLossGuard(
    canvas,
    () => Promise.resolve(),
    () => lastFrame && render(lastFrame),
    "the lich3d face",
  );

  const scene = new THREE.Scene();
  const pmrem = new THREE.PMREMGenerator(renderer);
  const room = new RoomEnvironment();
  const env = pmrem.fromScene(room, 0.04).texture;
  room.dispose();
  pmrem.dispose();

  const { meta } = asset;
  const camera = new THREE.PerspectiveCamera(meta.camera.fov, canvas.width / Math.max(1, canvas.height), 0.05, 100);
  camera.position.set(...meta.camera.pos);
  const target = new THREE.Vector3(...meta.camera.target);
  camera.lookAt(target);
  camera.updateMatrixWorld();

  // ---- the head: the contract's nodes moved under one pivot at the neck
  const root = asset.root;
  scene.add(root);
  root.updateWorldMatrix(true, true);
  const node = (name: string) => {
    const o = root.getObjectByName(name);
    if (!o) throw new Error(`the head has no ${name}`);
    return o;
  };
  const skull = node("skull");
  const jaw = node("jaw");
  const eyeL = node("eye_l");
  const eyeR = node("eye_r");
  const monocle = node("monocle");
  const neckAt = node("neck").getWorldPosition(new THREE.Vector3());

  const pivot = new THREE.Group();
  pivot.name = "head-pivot";
  pivot.position.copy(neckAt);
  pivot.rotation.order = "YXZ";
  scene.add(pivot);
  pivot.updateMatrixWorld();
  for (const o of [skull, jaw, eyeL, eyeR, monocle]) pivot.attach(o);
  const jawRest = jaw.quaternion.clone();

  const box = meshBox(pivot);
  const headH = Math.max(1e-3, box.max.y - box.min.y);
  const headC = box.getCenter(new THREE.Vector3());
  const eyeR0 = meta.eye_radius;

  // the bone: one material per (texture, vertex colour) the meshes need
  const materials = new Map<string, THREE.MeshStandardMaterial>();
  const disposables: { dispose(): void }[] = [];
  const boneFor = (mesh: THREE.Mesh): THREE.MeshStandardMaterial => {
    const old = (Array.isArray(mesh.material) ? mesh.material[0] : mesh.material) as THREE.MeshStandardMaterial | undefined;
    const map = useTexture && old && "map" in old ? old.map : null;
    const vc = !!mesh.geometry.getAttribute("color");
    const key = `${map ? map.uuid : "-"}:${vc}`;
    let m = materials.get(key);
    if (!m) {
      m = new THREE.MeshStandardMaterial({ color: map ? 0xf4ecdc : BONE, map, vertexColors: vc, roughness: 0.88, metalness: 0, flatShading: true, envMap: env, envMapIntensity: 0.18 });
      materials.set(key, m);
    }
    return m;
  };
  const oldMaterials = new Set<THREE.Material>();
  for (const part of [skull, jaw]) {
    part.traverse((c) => {
      const mesh = c as THREE.Mesh;
      if (!mesh.isMesh) return;
      for (const m of Array.isArray(mesh.material) ? mesh.material : [mesh.material]) oldMaterials.add(m);
      mesh.material = boneFor(mesh);
    });
  }
  // the old materials go, but not the textures the new ones still use
  const keptMaps = new Set([...materials.values()].map((m) => m.map).filter(Boolean));
  for (const m of oldMaterials) {
    for (const v of Object.values(m)) if (v instanceof THREE.Texture && !keptMaps.has(v)) v.dispose();
    m.dispose();
  }

  // ---- eye fire and socket lights
  const flameGeo = new THREE.PlaneGeometry(1, 1);
  disposables.push(flameGeo);
  const eyes = [eyeL, eyeR].map((anchor, i) => {
    const mat = flameMaterial(i * 3.7);
    const flame = new THREE.Mesh(flameGeo, mat);
    flame.scale.setScalar(eyeR0 * 2.2);
    flame.renderOrder = 2;
    anchor.add(flame);
    const light = new THREE.PointLight(TEAL, EYE_CANDELA * headH * headH, 0, 2);
    light.position.set(0, -eyeR0 * 0.2, eyeR0 * 2.0);
    anchor.add(light);
    disposables.push(mat);
    return { anchor, flame, mat, light };
  });

  // ---- the monocle: an emissive ring and its orange light
  const ringGeo = new THREE.TorusGeometry(eyeR0 * 1.12, eyeR0 * 0.085, 6, 40);
  const ringMat = new THREE.MeshBasicMaterial({ color: ORANGE.clone().multiplyScalar(2.2) });
  const ring = new THREE.Mesh(ringGeo, ringMat);
  // a little proud of the anchor, so the brow and cheek do not swallow the ring
  ring.position.z = eyeR0 * 0.8;
  monocle.add(ring);
  const monocleLight = new THREE.PointLight(ORANGE, MONOCLE_CANDELA * headH * headH, 0, 2);
  monocleLight.position.set(eyeR0 * 0.9, 0, eyeR0 * 0.3);
  monocle.add(monocleLight);
  disposables.push(ringGeo, ringMat);

  // ---- the crown, in skull-height units about the skull's centre, turning with the head
  const crown = new THREE.Group();
  crown.position.copy(pivot.worldToLocal(headC.clone()));
  crown.scale.setScalar(headH);
  pivot.add(crown);
  const vein = { value: 1 };
  const crystalMat = crystalMaterial(env, vein);
  disposables.push(crystalMat);
  const crystals: { spec: CrystalSpec; mesh: THREE.Mesh }[] = crystalCrown(opts.seed ?? 11).map((spec) => {
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.BufferAttribute(spec.positions, 3));
    geo.computeVertexNormals();
    disposables.push(geo);
    const mesh = new THREE.Mesh(geo, crystalMat);
    mesh.position.set(spec.x, spec.y, spec.z);
    mesh.scale.set(spec.width, spec.length, spec.width);
    mesh.rotation.order = "ZXY";
    mesh.rotation.set(spec.lean, spec.spin0, spec.tilt);
    crown.add(mesh);
    return { spec, mesh };
  });

  // ---- lights on the bone: a warm key from the upper left, a cool fill, a teal rim from behind
  const hemi = new THREE.HemisphereLight(0x8d9099, 0x16120e, 0.35);
  const key = new THREE.DirectionalLight(0xffe7c8, 1.7);
  key.position.set(-1.5, 2.1, 1.5);
  const fill = new THREE.DirectionalLight(0x7090a8, 0.4);
  fill.position.set(1.6, 0.1, 1.2);
  const rim = new THREE.DirectionalLight(0x48ffd8, 0.9);
  rim.position.set(0.3, 1.2, -2.0);
  scene.add(hemi, key, fill, rim);

  // ---- the painted torso: a card filling the portrait's frame at the head's depth, drawn first
  if (opts.backdrop) {
    const b = opts.backdrop;
    const dist = camera.position.distanceTo(target);
    const frameH = 2 * dist * Math.tan(THREE.MathUtils.degToRad(meta.camera.fov / 2));
    const px = frameH / b.height;
    const [bx, by, bw, bh] = b.bbox;
    b.texture.colorSpace = THREE.SRGBColorSpace;
    const geo = new THREE.PlaneGeometry(bw * px, bh * px);
    // blended but kept in the opaque list (three draws every transparent object after the opaque
    // ones, whatever its renderOrder), so the card goes down first and the head draws over it
    const mat = new THREE.MeshBasicMaterial({
      map: b.texture,
      transparent: false,
      blending: THREE.CustomBlending,
      blendSrc: THREE.SrcAlphaFactor,
      blendDst: THREE.OneMinusSrcAlphaFactor,
      depthTest: false,
      depthWrite: false,
      toneMapped: false,
    });
    disposables.push(geo, mat, b.texture);
    const backdropMesh = new THREE.Mesh(geo, mat);
    backdropMesh.renderOrder = -10;
    const card = new THREE.Group();
    card.position.copy(target);
    card.quaternion.copy(camera.quaternion);
    backdropMesh.position.set((bx + bw / 2 - b.width / 2) * px, (b.height / 2 - (by + bh / 2)) * px, 0);
    card.add(backdropMesh);
    scene.add(card);
  }

  // ---- post
  let composer: EffectComposer | null = null;
  let bloom: UnrealBloomPass | null = null;
  const size = new THREE.Vector2(canvas.width, canvas.height);
  if (bloomOn) {
    const rt = new THREE.WebGLRenderTarget(size.x, size.y, { type: THREE.HalfFloatType, samples: opts.samples ?? 4 });
    composer = new EffectComposer(renderer, rt);
    composer.setPixelRatio(1);
    composer.addPass(new RenderPass(scene, camera));
    bloom = new UnrealBloomPass(size.clone(), BLOOM.strength, BLOOM.radius, BLOOM.threshold);
    composer.addPass(bloom);
    composer.addPass(new OutputPass());
  }

  // ---- per frame
  const neckRest = neckAt.clone();
  const pivotQ = new THREE.Quaternion();
  const jawQ = new THREE.Quaternion();
  const xAxis = new THREE.Vector3(1, 0, 0);
  let lastT: number | null = null;
  let crownClock = 0;

  function render(f: LichFrame): void {
    lastFrame = f;
    if (guard.lost) return;
    const dt = lastT === null ? 0 : Math.min(0.1, Math.max(0, f.t - lastT));
    lastT = f.t;
    crownClock += dt * f.float;

    pivot.rotation.set(f.pitch, f.yaw, f.roll);
    pivot.position.set(neckRest.x, neckRest.y + 0.02 * f.lean * headH, neckRest.z + 0.12 * f.lean * headH);
    pivot.scale.setScalar(1 + 0.05 * f.lean);
    jaw.quaternion.copy(jawRest).multiply(jawQ.setFromAxisAngle(xAxis, f.jaw));
    pivot.updateMatrixWorld(true);

    const flare = f.flare;
    const lightOf = [f.eyeL, f.eyeR];
    eyes.forEach((e, i) => {
      const level = (lightOf[i] ?? 1) * (1 + 0.6 * flare);
      const u = e.mat.uniforms;
      u.uTime!.value = f.t;
      u.uIntensity!.value = level;
      u.uH!.value = f.flameH;
      u.uW!.value = f.flameW;
      u.uFlicker!.value = f.flicker;
      e.flame.position.set(f.flameX * eyeR0 * 0.35, f.flameY * eyeR0 * 0.3, 0);
      // turned to the camera: undo the anchor's world turn, take the camera's
      e.anchor.getWorldQuaternion(pivotQ).invert();
      e.flame.quaternion.copy(pivotQ).multiply(camera.quaternion);
      e.light.intensity = EYE_CANDELA * headH * headH * level * (1 + 0.06 * f.flicker * Math.sin(f.t * 23 + i));
    });
    monocleLight.intensity = MONOCLE_CANDELA * headH * headH * (1 + 0.5 * flare);

    crown.rotation.y = Math.sin(crownClock * 0.25) * 0.12;
    for (const { spec, mesh } of crystals) {
      const at = crystalAt(spec, crownClock, f.float);
      mesh.position.y = at.y;
      mesh.rotation.y = at.spin;
    }
    vein.value = 1 + flare * 2.2 * (0.6 + 0.4 * Math.sin(f.t * 12));

    if (composer && bloom) {
      bloom.strength = BLOOM.strength + 0.9 * flare;
      composer.render();
    } else {
      renderer.render(scene, camera);
    }
  }

  return {
    render,
    resize() {
      renderer.setSize(canvas.width, canvas.height, false);
      camera.aspect = canvas.width / Math.max(1, canvas.height);
      camera.updateProjectionMatrix();
      composer?.setSize(canvas.width, canvas.height);
    },
    finish: () => renderer.getContext().finish(),
    get lost() {
      return guard.lost || renderer.getContext().isContextLost();
    },
    info() {
      const i = renderer.info;
      return { calls: i.render.calls, triangles: i.render.triangles, geometries: i.memory.geometries, textures: i.memory.textures };
    },
    dispose() {
      guard.dispose();
      scene.traverse((o) => {
        const mesh = o as THREE.Mesh;
        if (mesh.isMesh) mesh.geometry.dispose();
      });
      for (const m of materials.values()) {
        m.map?.dispose();
        m.dispose();
      }
      for (const d of disposables) d.dispose();
      env.dispose();
      composer?.renderTarget1.dispose();
      composer?.renderTarget2.dispose();
      bloom?.dispose();
      composer?.dispose();
      renderer.dispose();
    },
  };
}
