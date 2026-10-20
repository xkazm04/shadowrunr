// A form's layers: the glow-rig `FormSpec` (contract appendix (voice), `GET /avatar/{file}`).
//
// The rig ships as `layers.json` plus its PNGs, a layer split of the owner's portrait: a static
// backplate, the body, the head, floating parts, eye lights, a seam, and optionally a hinged
// aperture. The daemon serves it from `<data>/avatar/rig/`; with none there (404), or when it does
// not load or check, the stage draws its procedural placeholder (`placeholder.ts`) instead. This
// module names the parts of the file the renderer uses, checks it, and fetches the pictures.

export type LayerKind =
  | "static"
  | "body"
  | "head"
  | "float"
  | "eye"
  | "seam"
  | "aperture_upper"
  | "aperture_lower"
  | "aperture_glow"
  | "halo"
  | "pulse"
  /** Not in any layers.json: the ground line the renderer adds when the rig names a `ground`. */
  | "ground";

const KINDS: ReadonlySet<string> = new Set<LayerKind>([
  "static",
  "body",
  "head",
  "float",
  "eye",
  "seam",
  "aperture_upper",
  "aperture_lower",
  "aperture_glow",
  "halo",
  "pulse",
]);

export type Rgb = [number, number, number];

export interface LayerSpec {
  name: string;
  kind: LayerKind;
  file?: string;
  /** x, y, w, h in source pixels. */
  bbox?: [number, number, number, number];
  depth?: number;
  follow?: number;
  /** `light` adds like light; anything else occludes by its coverage. */
  mode?: string;
  occl?: number;
  gain?: number;
  bloom?: number;
  voice_gain?: number;
  shimmer?: boolean;
  glint?: boolean;
  blink?: "iris" | "dim";
  float?: { amp?: [number, number]; freq?: number; rot_deg?: number; pivot?: [number, number] };
  pulse?: { amp?: number; freq?: number };
  ring_w?: number;
  meta?: { center?: [number, number]; radius?: number; color?: Rgb; core_r?: number };
}

export interface RigSpec {
  head: { center: [number, number]; radius: [number, number]; pivot_y?: number; chin_y: number };
  shimmer?: { center: [number, number]; speed_px_s: number; wavelength_px: number; gain: number };
  tip_pulse?: number;
  glint?: { gain: number; width_px: number };
  aperture?: { seam_y: number; open_frac?: number; top: number; bottom: number; x0: number; x1: number };
  /**
   * The ground: a line of light at `y` (source px), `half_width` either side of `x` (default the
   * art's centre), that the lower figure dissolves into. The figure fades out over `fade` px above
   * the line, and `curve` px earlier at the line's ends, so the shoulders narrow into it.
   */
  ground?: { y: number; half_width: number; x?: number; fade: number; curve: number };
  /** The renderer's own lights, linear 0..1: the light in the open mouth and the ground line.
   *  Absent keys take the stage palette (ember mouth, verdigris line). */
  colors?: { mouth?: Rgb; line_core?: Rgb; line_glow?: Rgb };
}

export interface FormSpec {
  version: number;
  width: number;
  height: number;
  rig_type: string;
  layers: LayerSpec[];
  rig: RigSpec;
}

/** Pictures by file name: a decoded bitmap (straight alpha) or any texture source the renderer takes. */
export type LayerImage = ImageBitmap | HTMLCanvasElement | OffscreenCanvas;

export interface LoadedForm {
  spec: FormSpec;
  /** Decoded pictures by file name, straight (not premultiplied) alpha. Kept so a lost context can rebuild. */
  images: Map<string, LayerImage>;
  /** Which form this is. */
  form: "rig" | "placeholder";
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const isNum = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);
const isTuple = (v: unknown, n: number) => Array.isArray(v) && v.length === n && v.every(isNum);
/** A layer's file is one path segment: it is fetched next to layers.json and nowhere else. */
const FILE_RE = /^[A-Za-z0-9_][A-Za-z0-9_.-]*\.png$/;

/**
 * A spec is usable when it is a glow rig with a head, a size and well-formed layers; anything else
 * is refused with the reason, so the stage can say once why it drew the placeholder instead.
 */
export function checkSpec(raw: unknown): FormSpec {
  const value = raw;
  if (!isObj(value)) throw new Error("layers.json is not an object");
  if (value.rig_type !== "glow") throw new Error(`layers.json is a ${String(value.rig_type)} rig, not a glow rig`);
  if (!(isNum(value.width) && value.width > 0 && isNum(value.height) && value.height > 0)) throw new Error("layers.json has no size");
  const rig = value.rig;
  if (!isObj(rig) || !isObj(rig.head)) throw new Error("layers.json has no head");
  const head = rig.head;
  if (!isTuple(head.center, 2) || !isTuple(head.radius, 2) || !isNum(head.chin_y)) throw new Error("layers.json's head is malformed");
  const layers = value.layers;
  if (!Array.isArray(layers) || layers.length === 0) throw new Error("layers.json has no layers");
  const files = new Set<string>();
  layers.forEach((l: unknown, i) => {
    if (!isObj(l) || typeof l.name !== "string") throw new Error(`layer ${i} has no name`);
    if (typeof l.kind !== "string" || !KINDS.has(l.kind)) throw new Error(`layer ${l.name} has an unknown kind ${String(l.kind)}`);
    if (l.bbox !== undefined && !isTuple(l.bbox, 4)) throw new Error(`layer ${l.name} has a malformed bbox`);
    if (l.file !== undefined) {
      if (typeof l.file !== "string" || !FILE_RE.test(l.file)) throw new Error(`layer ${l.name} names a file that is not a single .png`);
      files.add(l.file);
    }
  });
  if (rig.aperture !== undefined) {
    const a = rig.aperture;
    if (!isObj(a) || ![a.seam_y, a.top, a.bottom, a.x0, a.x1].every(isNum)) throw new Error("layers.json's aperture is malformed");
  }
  return raw as FormSpec;
}

/** The decode the rig's pictures need: straight alpha, no colour conversion. */
const DECODE: ImageBitmapOptions = { premultiplyAlpha: "none", colorSpaceConversion: "none" };

/** The fetch the loader uses (tests pass a fake). */
export type Fetcher = (url: string) => Promise<Pick<Response, "ok" | "status" | "json" | "blob">>;

/** Fetch and decode the daemon's rig at `base` (ending in `/`). Rejects with the reason. */
export async function fetchRig(base: string, get: Fetcher = (u) => fetch(u)): Promise<LoadedForm> {
  const r = await get(`${base}layers.json`);
  if (!r.ok) throw new Error(`layers.json: HTTP ${r.status}`);
  const spec = checkSpec(await r.json());
  const images = new Map<string, LayerImage>();
  await Promise.all(
    spec.layers
      .filter((l) => l.file)
      .map(async (l) => {
        const file = l.file as string;
        const res = await get(base + file);
        if (!res.ok) throw new Error(`${file}: HTTP ${res.status}`);
        images.set(file, await createImageBitmap(await res.blob(), DECODE));
      }),
  );
  return { spec, images, form: "rig" };
}

const cache = new Map<string, Promise<LoadedForm>>();
let told = false;

/**
 * The form to draw: the daemon's rig at `rigBase` when there is one, else the placeholder. A 404,
 * a network error or a bad spec all fall back; anything but a plain 404 is logged once with why.
 * Loaded once per page and shared by every face.
 */
export function loadForm(rigBase: string | null, placeholder: () => Promise<LoadedForm>): Promise<LoadedForm> {
  const key = rigBase ?? "";
  let p = cache.get(key);
  if (!p) {
    p = (rigBase ? fetchRig(rigBase) : Promise.reject(new Error("no daemon"))).catch((error: unknown) => {
      const why = error instanceof Error ? error.message : String(error);
      if (rigBase && !told && !/HTTP 404/.test(why)) {
        told = true;
        console.warn(`[stage] the face rig did not load (${why}); drawing the placeholder.`);
      }
      return placeholder();
    });
    cache.set(key, p);
    // A failed placeholder is not remembered, so a later mount may try again.
    p.catch(() => cache.delete(key));
  }
  return p;
}
