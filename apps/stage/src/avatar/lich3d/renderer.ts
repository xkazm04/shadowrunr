// The lich3d renderer: one three.js scene that draws a `LichFrame`. Pure draw: it owns no loop and
// no clock (the face's loop calls `render`, a capture tool can call it at any frame).
//
// The scene, back to front: the rig's painted torso as a backdrop card (drawn first, never
// occluding), then the head on a pivot at `neck`: the bone
// (warm ivory or the GLB's own texture, in one of two looks: `paint`, a banded toon ramp with
// cavity darkening and teal/orange rims like the portrait, or `real`, smooth PBR), the optional
// collar (the GLB's `collar` node, turned a fraction of the head's turn), the jaw on its hinge, an eye fire in
// each socket (an additive flame card turned to the camera) with a teal point light inside that
// lights the bone around it, the orange monocle ring with its own light, and the crown: the
// portrait's eleven obsidian shards in its fan (dark, faceted, a teal core low in each), each on its
// depth plane and turning with its share of the head's turn. Bloom picks up what burns above its
// threshold (eye fire, monocle, crystal cores), then ACES tone mapping to sRGB.
//
// The canvas is transparent (premultiplied): where nothing is drawn the page shows through, and
// the glow is light over it (COMPOSE_FRAG), so in the corner the face covers only itself.

import * as THREE from "three";
import { FullScreenQuad } from "three/addons/postprocessing/Pass.js";
import { UnrealBloomPass } from "three/addons/postprocessing/UnrealBloomPass.js";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";

import { contextLossGuard } from "../gl-kit";
import { collarMotion, leanShift, type LichFrame } from "./channels";
import type { LichMeta } from "./contract";
import { crystalAt, crystalCrown, PORTRAIT_PX, type CrystalSpec } from "./crystals";

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
/** A mesh with at least this many triangles is dense enough to draw smooth in the real look. */
const DENSE_TRIS = 12000;
const BLOOM = { strength: 0.55, radius: 0.35, threshold: 0.85 };
/** How much the crystal cores follow the eye light (x1 + CORE_EYE per unit above the rig's own). */
const CORE_EYE = 0.12;

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
  // coverage as bright as the fire (its colour is added, its alpha laid over): on a transparent
  // canvas the fire's halo past the bone is light over whatever is behind, not a dark card
  gl_FragColor.a = clamp(max(gl_FragColor.r, max(gl_FragColor.g, gl_FragColor.b)), 0.0, 1.0);
}`;

const QUAD_VERT = /* glsl */ `
precision highp float;
uniform mat4 modelViewMatrix;
uniform mat4 projectionMatrix;
attribute vec3 position;
attribute vec2 uv;
varying vec2 vUv;
void main() {
  vUv = uv;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}`;

/**
 * The bloom path's last pass, for a transparent canvas. The scene target holds premultiplied linear
 * colour (cleared to transparent black) and the bloom composite's texture the glow, which three's
 * additive blend would have added as rgb x a. A pixel is its covered share `a` (the surface, with
 * the glow on it, tone mapped and encoded as the opaque canvas did) and its uncovered rest (the
 * glow alone, as light over the page: its alpha its brightest channel, so it never darkens what
 * is behind more than it lights it). Opaque pixels come out exactly as before; over black,
 * everything does.
 */
const COMPOSE_FRAG = /* glsl */ `
precision highp float;
uniform sampler2D tScene;
uniform sampler2D tBloom;
#include <tonemapping_pars_fragment>
#include <colorspace_pars_fragment>
varying vec2 vUv;
vec3 finish(vec3 c) {
  return sRGBTransferOETF(vec4(ACESFilmicToneMapping(c), 1.0)).rgb;
}
void main() {
  vec4 s = texture2D(tScene, vUv);
  vec4 g = texture2D(tBloom, vUv);
  vec3 b = max(g.rgb * g.a, vec3(0.0));
  float a = clamp(s.a, 0.0, 1.0);
  vec3 glow = finish(b);
  float ga = clamp(max(glow.r, max(glow.g, glow.b)), 0.0, 1.0);
  vec3 surface = a > 0.0 ? finish(s.rgb / a + b) : vec3(0.0);
  gl_FragColor = vec4(a * surface + (1.0 - a) * glow, a + (1.0 - a) * ga);
}`;

/**
 * The paint look's diffuse ramp: three soft-edged bands (deep shadow, mid, lit) along n.l, read by
 * MeshToonMaterial. 128 texels wide, linear filtered, so the edge between bands is a few pixels of
 * blend, not a hard line.
 */
export function paintRamp(): THREE.DataTexture {
  const n = 128;
  const data = new Uint8Array(n * 4);
  const step = (x: number, at: number, w: number) => {
    const t = Math.min(1, Math.max(0, (x - (at - w)) / (2 * w)));
    return t * t * (3 - 2 * t);
  };
  for (let i = 0; i < n; i++) {
    const x = i / (n - 1);
    // n.l = 2x - 1: the terminator sits near x 0.5
    const v = 0.07 + 0.38 * step(x, 0.5, 0.035) + 0.55 * step(x, 0.66, 0.035);
    const b = Math.round(Math.min(1, v) * 255);
    data.set([b, b, b, 255], i * 4);
  }
  const tex = new THREE.DataTexture(data, n, 1, THREE.RGBAFormat);
  tex.minFilter = tex.magFilter = THREE.LinearFilter;
  tex.generateMipmaps = false;
  tex.needsUpdate = true;
  return tex;
}

/**
 * The bone in the portrait's painted style: a banded toon ramp, the texture's darks pushed into
 * cavities, the colour pushed warm and contrasty, a crisp teal rim on the eye side and an orange
 * kiss on the monocle side. Lit by the same lights as the real look (eye and monocle included).
 */
function paintMaterial(map: THREE.Texture | null, vc: boolean, ramp: THREE.Texture, flat: boolean): THREE.MeshToonMaterial {
  const m = new THREE.MeshToonMaterial({ color: map ? 0xf4ecdc : BONE, map, vertexColors: vc, gradientMap: ramp });
  // (not a constructor parameter of the toon material, but the shader honours it)
  (m as THREE.MeshToonMaterial & { flatShading: boolean }).flatShading = flat;
  m.onBeforeCompile = (shader) => {
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <opaque_fragment>",
      `float pLum = dot(diffuseColor.rgb, vec3(0.3, 0.59, 0.11));
      float pCav = smoothstep(0.08, 0.5, pLum);
      outgoingLight *= mix(0.3, 1.0, pCav);
      float pLit = dot(outgoingLight, vec3(0.3, 0.59, 0.11));
      outgoingLight = mix(vec3(pLit), outgoingLight, 0.8);
      outgoingLight *= 1.45 * vec3(1.04, 0.99, 0.93) * mix(vec3(0.78, 0.9, 1.18), vec3(1.0), smoothstep(0.04, 0.35, pLit));
      float pFr = pow(1.0 - clamp(dot(normalize(normal), normalize(vViewPosition)), 0.0, 1.0), 4.0);
      float pSide = normalize(normal).x;
      outgoingLight += vec3(0.08, 0.95, 0.78) * pFr * smoothstep(0.1, -0.6, pSide) * 0.6 * (0.3 + pCav);
      outgoingLight += vec3(1.0, 0.45, 0.08) * pFr * smoothstep(0.1, 0.7, pSide) * 0.6 * (0.3 + pCav);
      #include <opaque_fragment>`,
    );
  };
  return m;
}

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
    // the colour added as it always was; the alpha laid over, so the bone stays opaque
    blending: THREE.CustomBlending,
    blendSrc: THREE.OneFactor,
    blendDst: THREE.OneFactor,
    blendSrcAlpha: THREE.OneFactor,
    blendDstAlpha: THREE.OneMinusSrcAlphaFactor,
    transparent: true,
    depthWrite: false,
  });
}

/**
 * Obsidian: near-black glossy stone, faceted (flat normals), its facet edges catching the room
 * light grey-blue; a teal glow in its lower part only (the portrait's veins), strongest at the
 * base and on the facets turned away from the camera, with veins traced in the stone's own space.
 * `core` scales the glow (flare, eye light).
 */
function crystalMaterial(env: THREE.Texture | null, core: { value: number }): THREE.MeshStandardMaterial {
  const m = new THREE.MeshStandardMaterial({ color: 0x0b0e13, roughness: 0.22, metalness: 0.65, flatShading: true, envMap: env, envMapIntensity: 0.85 });
  m.onBeforeCompile = (shader) => {
    shader.uniforms.uCore = core;
    shader.uniforms.uCoreColor = { value: new THREE.Color(0.06, 0.95, 0.74) };
    shader.vertexShader = shader.vertexShader
      .replace("#include <common>", "#include <common>\nvarying vec3 vStone;")
      .replace("#include <begin_vertex>", "#include <begin_vertex>\nvStone = position;");
    shader.fragmentShader = shader.fragmentShader
      .replace("#include <common>", "#include <common>\nvarying vec3 vStone;\nuniform float uCore;\nuniform vec3 uCoreColor;")
      .replace(
        "#include <emissivemap_fragment>",
        `#include <emissivemap_fragment>
        float sFace = clamp(dot(normalize(normal), normalize(vViewPosition)), 0.0, 1.0);
        float sEdge = pow(1.0 - sFace, 2.0);
        float sLow = smoothstep(0.05, -0.45, vStone.y);
        float sVn = sin(vStone.y * 9.0 + sin(vStone.x * 11.0 + vStone.z * 7.0) * 1.5);
        float sVein = 1.0 - smoothstep(0.1, 0.45, abs(sVn));
        totalEmissiveRadiance += uCoreColor * sLow * sLow * (0.12 + 1.5 * sEdge + 0.35 * sVein) * 1.3 * uCore;
        totalEmissiveRadiance += vec3(0.2, 0.25, 0.32) * sEdge * (1.0 - sLow) * 0.7;`,
      );
  };
  return m;
}

interface BloomPost {
  render(scene: THREE.Scene, camera: THREE.Camera, strength: number): void;
  setSize(w: number, h: number): void;
  dispose(): void;
}

type Uniforms = Record<string, THREE.IUniform>;
// UnrealBloomPass.BlurDirectionX / Y (not in its type declarations)
const BLUR_X = new THREE.Vector2(1, 0);
const BLUR_Y = new THREE.Vector2(0, 1);

/**
 * Bloom onto a transparent canvas. The scene goes into an MSAA half-float target cleared to
 * transparent black (so it holds premultiplied colour and coverage); UnrealBloomPass's own
 * materials extract, blur and composite the glow (its steps 1-3, run here because its last step
 * would add the glow into the scene target and make it opaque); COMPOSE_FRAG puts both on the canvas.
 */
function createBloomPost(renderer: THREE.WebGLRenderer, w: number, h: number, samples: number): BloomPost {
  const sceneRT = new THREE.WebGLRenderTarget(w, h, { type: THREE.HalfFloatType, samples });
  const bloom = new UnrealBloomPass(new THREE.Vector2(w, h), BLOOM.strength, BLOOM.radius, BLOOM.threshold);
  const quad = new FullScreenQuad();
  const compose = new THREE.RawShaderMaterial({
    name: "LichCompose",
    uniforms: { tScene: { value: sceneRT.texture }, tBloom: { value: null }, toneMappingExposure: { value: renderer.toneMappingExposure } },
    vertexShader: QUAD_VERT,
    fragmentShader: COMPOSE_FRAG,
    depthTest: false,
    depthWrite: false,
  });
  const high = bloom.highPassUniforms as Uniforms;
  const clear = new THREE.Color();
  const pass = (material: THREE.Material, target: THREE.WebGLRenderTarget | null) => {
    quad.material = material;
    renderer.setRenderTarget(target);
    renderer.clear();
    quad.render(renderer);
  };
  return {
    render(scene, camera, strength) {
      const oldAlpha = renderer.getClearAlpha();
      renderer.getClearColor(clear);
      renderer.setClearColor(0x000000, 0);
      renderer.setRenderTarget(sceneRT);
      renderer.render(scene, camera);
      const autoClear = renderer.autoClear;
      renderer.autoClear = false;
      // 1. the bright areas
      high.tDiffuse!.value = sceneRT.texture;
      high.luminosityThreshold!.value = bloom.threshold;
      pass(bloom.materialHighPassFilter, bloom.renderTargetBright);
      // 2. blurred down the mips
      let input = bloom.renderTargetBright;
      for (let i = 0; i < bloom.nMips; i++) {
        const blur = bloom.separableBlurMaterials[i]!;
        const u = blur.uniforms as Uniforms;
        u.colorTexture!.value = input.texture;
        u.direction!.value = BLUR_X;
        pass(blur, bloom.renderTargetsHorizontal[i]!);
        u.colorTexture!.value = bloom.renderTargetsHorizontal[i]!.texture;
        u.direction!.value = BLUR_Y;
        pass(blur, bloom.renderTargetsVertical[i]!);
        input = bloom.renderTargetsVertical[i]!;
      }
      // 3. the mips composited
      const cu = bloom.compositeMaterial.uniforms as Uniforms;
      cu.bloomStrength!.value = strength;
      cu.bloomRadius!.value = bloom.radius;
      cu.bloomTintColors!.value = bloom.bloomTintColors;
      pass(bloom.compositeMaterial, bloom.renderTargetsHorizontal[0]!);
      // 4. both onto the canvas
      const ou = compose.uniforms as Uniforms;
      ou.tBloom!.value = bloom.renderTargetsHorizontal[0]!.texture;
      ou.toneMappingExposure!.value = renderer.toneMappingExposure;
      pass(compose, null);
      renderer.autoClear = autoClear;
      renderer.setClearColor(clear, oldAlpha);
    },
    setSize(w, h) {
      sceneRT.setSize(w, h);
      bloom.setSize(w, h);
    },
    dispose() {
      sceneRT.dispose();
      bloom.dispose();
      compose.dispose();
      quad.dispose();
    },
  };
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
  // a transparent canvas (premultiplied): only the head, the torso card, the crown and their glow
  // cover what is behind the face
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: !bloomOn, alpha: true, premultipliedAlpha: true, powerPreference: "high-performance" });
  renderer.setPixelRatio(1);
  renderer.setSize(canvas.width, canvas.height, false);
  renderer.setClearColor(0x000000, 0);
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
  // the optional collar stays a root node (its position is its pivot), off the head's pivot
  const collar = root.getObjectByName("collar") ?? null;
  const collarRest = collar ? collar.position.clone() : null;
  if (collar) collar.rotation.order = "YXZ";

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
  const disposables: { dispose(): void }[] = [];

  // the bone: one material per (texture, normal map, vertex colour) the meshes need, in the look
  const look: LichLook = opts.look ?? "paint";
  const ramp = look === "paint" ? paintRamp() : null;
  if (ramp) disposables.push(ramp);
  const materials = new Map<string, THREE.MeshStandardMaterial | THREE.MeshToonMaterial>();
  const boneFor = (mesh: THREE.Mesh) => {
    const old = (Array.isArray(mesh.material) ? mesh.material[0] : mesh.material) as THREE.MeshStandardMaterial | undefined;
    const map = useTexture && old && "map" in old ? old.map : null;
    const normalMap = look === "real" && useTexture && old && "normalMap" in old ? old.normalMap : null;
    const vc = !!mesh.geometry.getAttribute("color");
    // a dense mesh reads smooth in the real look; the paint look keeps its facets
    const tris = (mesh.geometry.index ? mesh.geometry.index.count : mesh.geometry.getAttribute("position").count) / 3;
    const smooth = look === "real" && tris >= DENSE_TRIS;
    if (smooth && !mesh.geometry.getAttribute("normal")) mesh.geometry.computeVertexNormals();
    const key = `${map ? map.uuid : "-"}:${normalMap ? normalMap.uuid : "-"}:${vc}:${smooth}`;
    let m = materials.get(key);
    if (!m) {
      m =
        look === "paint"
          ? paintMaterial(map, vc, ramp!, true)
          : new THREE.MeshStandardMaterial({
              color: map ? 0xf4ecdc : BONE,
              map,
              normalMap,
              vertexColors: vc,
              roughness: 0.8,
              metalness: 0,
              flatShading: !smooth,
              envMap: env,
              envMapIntensity: 0.25,
            });
      materials.set(key, m);
    }
    return m;
  };
  const oldMaterials = new Set<THREE.Material>();
  for (const part of collar ? [skull, jaw, collar] : [skull, jaw]) {
    part.traverse((c) => {
      const mesh = c as THREE.Mesh;
      if (!mesh.isMesh) return;
      for (const m of Array.isArray(mesh.material) ? mesh.material : [mesh.material]) oldMaterials.add(m);
      mesh.material = boneFor(mesh);
    });
  }
  // the old materials go, but not the textures the new ones still use
  const keptMaps = new Set([...materials.values()].flatMap((m) => [m.map, "normalMap" in m ? m.normalMap : null]).filter(Boolean));
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

  // ---- the crown: each portrait shard projected through the camera onto its depth plane, so at
  // rest it covers the painted one; it turns about the neck with its share of the head's turn
  const core = { value: 1 };
  const crystalMat = crystalMaterial(env, core);
  disposables.push(crystalMat);
  const frameAt = 2 * camera.position.distanceTo(target) * Math.tan(THREE.MathUtils.degToRad(meta.camera.fov / 2));
  const pxAt = frameAt / PORTRAIT_PX;
  const crystals: { spec: CrystalSpec; holder: THREE.Group; mesh: THREE.Mesh; rest: THREE.Vector3; px: number }[] = crystalCrown(opts.seed ?? 11).map((spec) => {
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.BufferAttribute(spec.positions, 3));
    disposables.push(geo);
    // the shard's centre on the portrait's plane at the target, then along the camera's ray to its depth
    const onCard = new THREE.Vector3((spec.cx - PORTRAIT_PX / 2) * pxAt, (PORTRAIT_PX / 2 - spec.cy) * pxAt, 0)
      .applyQuaternion(camera.quaternion)
      .add(target);
    const ray = onCard.sub(camera.position);
    const k = (headC.z + spec.layout.depth * headH - camera.position.z) / ray.z;
    const at = camera.position.clone().addScaledVector(ray, k);
    const px = pxAt * k;
    const mesh = new THREE.Mesh(geo, crystalMat);
    mesh.scale.set(spec.width * px, spec.length * px, spec.width * px * 0.8);
    mesh.rotation.order = "ZXY";
    const holder = new THREE.Group();
    holder.rotation.order = "YXZ";
    holder.position.copy(neckAt);
    holder.add(mesh);
    scene.add(holder);
    return { spec, holder, mesh, rest: at.sub(neckAt), px };
  });

  // ---- lights on the bone: a warm key from the upper left, a cool fill, a teal rim from behind
  const painted = look === "paint";
  const hemi = painted ? new THREE.HemisphereLight(0x6f86b0, 0x1a1a2a, 0.5) : new THREE.HemisphereLight(0x8d9099, 0x16120e, 0.35);
  const key = new THREE.DirectionalLight(painted ? 0xffe0b0 : 0xffe7c8, painted ? 3.2 : 1.7);
  key.position.set(painted ? -0.7 : -1.5, painted ? 2.5 : 2.1, painted ? 1.8 : 1.5);
  const fill = new THREE.DirectionalLight(0x7090a8, painted ? 0.25 : 0.4);
  fill.position.set(1.6, 0.1, 1.2);
  const rim = new THREE.DirectionalLight(0x48ffd8, painted ? 0.7 : 0.9);
  rim.position.set(painted ? -1.2 : 0.3, 1.2, -2.0);
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
      // its coverage, laid over (not squared): the card's own soft edge on a transparent canvas
      blendSrcAlpha: THREE.OneFactor,
      blendDstAlpha: THREE.OneMinusSrcAlphaFactor,
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
  // the scene into a premultiplied HDR target, UnrealBloomPass's own blur of it, and one pass that
  // tone maps both onto the transparent canvas (COMPOSE_FRAG); no EffectComposer, whose additive
  // blend and OutputPass would make every pixel the bloom touches opaque
  const post = bloomOn ? createBloomPost(renderer, canvas.width, canvas.height, opts.samples ?? 4) : null;

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
    const lean = leanShift(f.lean, headH);
    pivot.position.set(neckRest.x, neckRest.y + lean.y, neckRest.z + lean.z);
    pivot.scale.setScalar(lean.scale);
    if (collar && collarRest) {
      const c = collarMotion(f, headH);
      collar.rotation.set(c.pitch, c.yaw, 0);
      collar.position.set(collarRest.x, collarRest.y + c.y, collarRest.z + c.z);
      collar.scale.setScalar(c.scale);
    }
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

    for (const { spec, holder, mesh, rest, px } of crystals) {
      const share = spec.layout.follow;
      holder.position.copy(pivot.position);
      holder.rotation.set(f.pitch * share, f.yaw * share, f.roll * share);
      holder.scale.setScalar(lean.scale);
      const at = crystalAt(spec, crownClock, f.float);
      mesh.position.set(rest.x + at.dx * px, rest.y - at.dy * px, rest.z);
      mesh.rotation.set(0, at.turn, at.roll);
    }
    // the cores pulse with a flare and burn a little with the eye light
    core.value = (1 + flare * 2.2 * (0.6 + 0.4 * Math.sin(f.t * 12))) * (1 + CORE_EYE * (Math.max(f.eyeL, f.eyeR) - 1));

    if (post) {
      post.render(scene, camera, BLOOM.strength + 0.9 * flare);
    } else {
      renderer.setRenderTarget(null);
      renderer.render(scene, camera);
    }
  }

  return {
    render,
    resize() {
      renderer.setSize(canvas.width, canvas.height, false);
      camera.aspect = canvas.width / Math.max(1, canvas.height);
      camera.updateProjectionMatrix();
      post?.setSize(canvas.width, canvas.height);
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
        if ("normalMap" in m) m.normalMap?.dispose();
        m.dispose();
      }
      for (const d of disposables) d.dispose();
      env.dispose();
      post?.dispose();
      renderer.dispose();
    },
  };
}
