// The rig spec: what checkSpec refuses, the placeholder's validity and channels, the rig-or-
// placeholder load order, and the renderer's pure parts (context, palette, ground line).
import { afterEach, describe, expect, it, vi } from "vitest";

import { TOKENS_COLOR } from "@shadowrunr/tokens";

import { createGlowRenderer } from "./glow";
import { CONTEXT_ATTRIBUTES, gapCentre, gapHalf, groundLine, groundMask, LINE_CORE, MOUTH_LIGHT, rigColors, tokenRgb } from "./glow-geom";
import { checkSpec, fetchRig, loadForm, type Fetcher, type FormSpec, type LoadedForm } from "./layers";
import { ART, placeholderSpec } from "./placeholder";

const spec = placeholderSpec();
const clone = (): Record<string, unknown> => structuredClone(spec) as unknown as Record<string, unknown>;

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("checkSpec", () => {
  it("accepts the placeholder", () => {
    expect(checkSpec(clone())).toEqual(spec);
  });

  it.each([
    ["not an object", () => 3, /not an object/],
    ["not a glow rig", () => ({ ...clone(), rig_type: "puppet" }), /puppet rig, not a glow rig/],
    ["no size", () => ({ ...clone(), width: 0 }), /no size/],
    ["no head", () => ({ ...clone(), rig: {} }), /no head/],
    ["no layers", () => ({ ...clone(), layers: [] }), /no layers/],
    ["an unknown kind", () => ({ ...clone(), layers: [{ name: "x", kind: "tail" }] }), /unknown kind tail/],
    ["a path in a file name", () => ({ ...clone(), layers: [{ name: "x", kind: "head", file: "../secret.png" }] }), /single \.png/],
    ["a malformed bbox", () => ({ ...clone(), layers: [{ name: "x", kind: "head", bbox: [1, 2] }] }), /malformed bbox/],
  ])("refuses %s", (_what, make, why) => {
    expect(() => checkSpec(make())).toThrow(why);
  });
});

describe("the placeholder", () => {
  it("is a square glow rig whose layers sit inside the art, with unique files", () => {
    expect(spec.width).toBe(ART);
    expect(spec.height).toBe(ART);
    const files = spec.layers.flatMap((l) => (l.file ? [l.file] : []));
    expect(new Set(files).size).toBe(files.length);
    for (const l of spec.layers) {
      if (!l.bbox) continue;
      const [x, y, w, h] = l.bbox;
      expect(x, l.name).toBeGreaterThanOrEqual(0);
      expect(y, l.name).toBeGreaterThanOrEqual(0);
      expect(x + w, l.name).toBeLessThanOrEqual(ART);
      expect(y + h, l.name).toBeLessThanOrEqual(ART);
    }
  });

  it("exercises every channel: blink, mouth, sway, floats, the voice", () => {
    const kinds = (k: string) => spec.layers.filter((l) => l.kind === k);
    // blink: two eye lights that dim
    expect(kinds("eye").map((l) => [l.name, l.blink])).toEqual([
      ["eye_l", "dim"],
      ["eye_r", "dim"],
    ]);
    // the mouth: a hinged jaw, light in its gap, and a seam that brightens with the voice
    expect(kinds("aperture_lower")).toHaveLength(1);
    expect(kinds("aperture_glow")).toHaveLength(1);
    expect(kinds("seam")[0]?.voice_gain).toBeGreaterThan(1);
    expect(spec.rig.aperture?.seam_y).toBe(kinds("aperture_lower")[0]?.bbox?.[1]);
    // head sway and breath: a head and a body
    expect(kinds("head")).toHaveLength(1);
    expect(kinds("body")).toHaveLength(1);
    // floats: the crown shards, each with its own motion
    const floats = kinds("float");
    expect(floats.length).toBeGreaterThanOrEqual(3);
    expect(new Set(floats.map((f) => f.float?.freq)).size).toBe(floats.length);
    // the eyes and the orb warm with the voice
    for (const e of kinds("eye")) expect(e.voice_gain).toBeGreaterThan(0.12);
    expect(kinds("pulse")[0]?.voice_gain).toBeGreaterThan(0);
    // and a ground line it dissolves into, below the face and inside the art
    const g = spec.rig.ground!;
    expect(g.y).toBeGreaterThan(spec.rig.head.center[1] + spec.rig.head.radius[1]);
    expect(g.y).toBeLessThan(ART);
  });

  it("is the same every call", () => {
    expect(placeholderSpec()).toEqual(spec);
  });
});

describe("rig or placeholder", () => {
  const placeholder = async (): Promise<LoadedForm> => ({ spec, images: new Map(), form: "placeholder" });
  const json = (body: unknown, status = 200): Awaited<ReturnType<Fetcher>> => ({
    ok: status === 200,
    status,
    json: async () => body,
    blob: async () => new Blob([]),
  });

  it("a rig the daemon serves wins, its pictures fetched beside layers.json", async () => {
    const rig: FormSpec = { ...spec, layers: [{ name: "skull", kind: "head", file: "skull.png", bbox: [0, 0, 10, 10] }] };
    const asked: string[] = [];
    vi.stubGlobal("createImageBitmap", async () => ({ width: 1, height: 1 }));
    const form = await fetchRig("http://d/avatar/", async (u) => (asked.push(u), u.endsWith(".json") ? json(rig) : json(null)));
    expect(form.form).toBe("rig");
    expect(asked).toEqual(["http://d/avatar/layers.json", "http://d/avatar/skull.png"]);
    expect([...form.images.keys()]).toEqual(["skull.png"]);
  });

  it("no daemon, a 404, or a bad spec all fall back to the placeholder; only the bad spec is said", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    expect((await loadForm(null, placeholder)).form).toBe("placeholder");
    vi.stubGlobal("fetch", async () => json(null, 404));
    expect((await loadForm("http://a/avatar/", placeholder)).form).toBe("placeholder");
    expect(warn).not.toHaveBeenCalled();
    vi.stubGlobal("fetch", async () => json({ rig_type: "glow" }));
    expect((await loadForm("http://b/avatar/", placeholder)).form).toBe("placeholder");
    vi.stubGlobal("fetch", async () => json({ rig_type: "nope" }));
    expect((await loadForm("http://c/avatar/", placeholder)).form).toBe("placeholder");
    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0]?.[0])).toMatch(/no size/);
  });
});

describe("the renderer's pure parts", () => {
  it("asks for an alpha, premultiplied WebGL2 context and rejects without one", async () => {
    const asked: [string, unknown][] = [];
    const canvas = { getContext: (kind: string, attrs?: unknown) => (asked.push([kind, attrs]), null) } as unknown as HTMLCanvasElement;
    await expect(createGlowRenderer(canvas, { spec, images: new Map(), form: "placeholder" })).rejects.toThrow(/WebGL2 unavailable/);
    expect(asked).toEqual([["webgl2", CONTEXT_ATTRIBUTES]]);
    expect(CONTEXT_ATTRIBUTES.alpha && CONTEXT_ATTRIBUTES.premultipliedAlpha).toBe(true);
  });

  it("lights in the stage palette unless the rig names its own", () => {
    expect(tokenRgb(TOKENS_COLOR.signal).map((c) => Math.round(c * 255))).toEqual([0xe8, 0x54, 0x2a]);
    expect(rigColors(spec.rig)).toEqual({ mouth: MOUTH_LIGHT, lineCore: LINE_CORE, lineGlow: rigColors(spec.rig).lineGlow });
    expect(rigColors({ ...spec.rig, colors: { mouth: [0, 1, 0] } }).mouth).toEqual([0, 1, 0]);
  });

  it("the figure dissolves into the ground: whole above the fade, gone below the line, earlier at its ends", () => {
    const g = { y: 880, half_width: 300, fade: 280, curve: 160 };
    expect(groundMask(g, 1024, 512, 500)).toBe(1);
    expect(groundMask(g, 1024, 512, 1000)).toBe(0);
    const mid = groundMask(g, 1024, 512, 760);
    expect(mid).toBeGreaterThan(0);
    expect(mid).toBeLessThan(1);
    expect(groundMask(g, 1024, 512 + 300, 760)).toBeLessThan(mid);
  });

  it("the ground line breathes, brightens with the voice, and is still at rest", () => {
    const g = spec.rig.ground!;
    const rest = (t: number) => groundLine(g, ART, { t, activity: 0, voice: 0 }).gain;
    expect(rest(0)).toBe(rest(1.7));
    const breathing = [0, 1.3, 2.6, 3.9].map((t) => groundLine(g, ART, { t, activity: 1, voice: 0 }).gain);
    expect(Math.max(...breathing) - Math.min(...breathing)).toBeGreaterThan(0.05);
    expect(groundLine(g, ART, { t: 0, activity: 0, voice: 0.8 }).gain).toBeGreaterThan(rest(0) * 1.2);
  });

  it("the mouth light fills the gap the aperture opens: around the seam with two lips, below it with a jaw alone", () => {
    // both lips part from the seam: the gap is seam +- lift
    expect([gapCentre(500, 20, true), gapHalf(20, true)]).toEqual([500, 20]);
    // a jaw alone drops from the seam: the gap is seam .. seam + lift, so nothing lights the upper teeth
    expect([gapCentre(500, 20, false), gapHalf(20, false)]).toEqual([510, 10]);
    expect(gapCentre(500, 20, false) - gapHalf(20, false)).toBe(500);
  });
});
