// Gates on the token source and its generated outputs. Fidelity pins are byte copies of the
// winning prototype (The Poster Wall): poster.css :root, app.js PAGES + SA, sr-kit.js
// RANK_COLORS + GRADE_COLORS. A pin changes only when the owner changes the design.

import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import source from "../tokens.source.json";
import { BREAKPOINTS_PX, GRADE_COLORS, PAGE_IDS, PAGE_PALETTES, RANK_COLORS, TOKENS, cssRef, cssVar } from "./index";
import type { TokenGroup, TokenName } from "./index";

interface Role {
  value: string | string[];
  alpha?: string;
  at?: Record<string, string>;
  description: string;
}
interface Source {
  primitive: Record<string, Record<string, string>>;
  role: Record<string, Record<string, Role>>;
  pageRole: Record<string, string>;
  page: Record<string, Record<string, string>>;
  rank: Record<string, string>;
  grade: Record<string, string>;
  fontFace: { family: string; weights: string[]; file: string; subsets: Record<string, string> };
}
const src: Source = source;

const pkg = join(dirname(fileURLToPath(import.meta.url)), "..");
const css = readFileSync(join(pkg, "src", "tokens.css"), "utf8").replace(/\r\n/g, "\n");
const REF = /^\{([a-z]+)\.([^}]+)\}$/;

/** Every reference in the source, with where it sits. */
function allRefs(): Array<{ where: string; ref: string }> {
  const out: Array<{ where: string; ref: string }> = [];
  for (const [g, roles] of Object.entries(src.role)) {
    for (const [n, r] of Object.entries(roles)) {
      for (const ref of Array.isArray(r.value) ? r.value : [r.value]) out.push({ where: `role.${g}.${n}`, ref });
      for (const [bp, ref] of Object.entries(r.at ?? {})) out.push({ where: `role.${g}.${n}.at.${bp}`, ref });
    }
  }
  for (const [id, palette] of Object.entries(src.page)) {
    for (const [r, ref] of Object.entries(palette)) out.push({ where: `page.${id}.${r}`, ref });
  }
  for (const block of ["rank", "grade"] as const) {
    for (const [k, ref] of Object.entries(src[block])) if (!k.startsWith("$")) out.push({ where: `${block}.${k}`, ref });
  }
  out.push({ where: "fontFace.family", ref: src.fontFace.family });
  for (const ref of src.fontFace.weights) out.push({ where: "fontFace.weights", ref });
  return out;
}

function generator(dir: string, ...args: string[]) {
  return spawnSync(process.execPath, [join(dir, "scripts", "generate.mjs"), ...args], { encoding: "utf8" });
}

function copyPackage(): string {
  const dir = mkdtempSync(join(tmpdir(), "sr-tokens-"));
  for (const part of ["scripts", "src", "fonts", "tokens.source.json"]) cpSync(join(pkg, part), join(dir, part), { recursive: true });
  return dir;
}

describe("source integrity", () => {
  it("every role resolves to a primitive", () => {
    const refs = allRefs();
    expect(refs.length).toBeGreaterThan(100);
    const broken = refs.filter(({ ref }) => {
      const m = REF.exec(ref);
      return !m || src.primitive[m[1] ?? ""]?.[m[2] ?? ""] === undefined;
    });
    expect(broken).toEqual([]);
  });

  it("has no orphan primitives", () => {
    const used = new Set(allRefs().map(({ ref }) => ref.slice(1, -1)));
    const orphans = Object.entries(src.primitive).flatMap(([g, e]) =>
      Object.keys(e)
        .map((n) => `${g}.${n}`)
        .filter((k) => !used.has(k)),
    );
    expect(orphans).toEqual([]);
  });

  it("gives every role a one-sentence definition (admission test)", () => {
    const bad: string[] = [];
    const sentence = (s: string) => /^[A-Z].*\.$/.test(s) && !/[.!?]\s+[A-Z]/.test(s);
    for (const [g, roles] of Object.entries(src.role)) {
      for (const [n, r] of Object.entries(roles)) if (!sentence(r.description)) bad.push(`${g}.${n}`);
    }
    for (const [r, d] of Object.entries(src.pageRole)) if (!sentence(d)) bad.push(`pageRole.${r}`);
    expect(bad).toEqual([]);
  });

  it("ships every self-hosted font file and the OFL licence", () => {
    for (const w of ["500", "600", "700"]) {
      for (const s of Object.keys(src.fontFace.subsets)) {
        expect(existsSync(join(pkg, "fonts", `barlow-condensed-${s}-${w}-normal.woff2`)), `${s} ${w}`).toBe(true);
      }
    }
    expect(readFileSync(join(pkg, "fonts", "OFL.txt"), "utf8")).toContain("SIL OPEN FONT LICENSE Version 1.1");
  });
});

describe("generated outputs", () => {
  it("are in sync with the source (drift gate is green)", () => {
    const r = generator(pkg, "--check");
    expect(r.stderr).toBe("");
    expect(r.status).toBe(0);
    expect(r.stdout).toMatch(/check OK, 2 of 2 files/);
  });

  it("are deterministic: generating twice gives the committed bytes", () => {
    const dir = copyPackage();
    try {
      const committed = ["tokens.css", "tokens.ts"].map((f) => readFileSync(join(dir, "src", f), "utf8").replace(/\r\n/g, "\n"));
      expect(generator(dir).status).toBe(0);
      const first = ["tokens.css", "tokens.ts"].map((f) => readFileSync(join(dir, "src", f), "utf8"));
      expect(generator(dir).status).toBe(0);
      const second = ["tokens.css", "tokens.ts"].map((f) => readFileSync(join(dir, "src", f), "utf8"));
      expect(second).toEqual(first);
      expect(first).toEqual(committed);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("the drift gate fails (exit 1) on a seeded edit of a generated file", () => {
    const dir = copyPackage();
    try {
      const file = join(dir, "src", "tokens.css");
      writeFileSync(file, readFileSync(file, "utf8").replace("--sr-color-surface-paper: #efe6d2;", "--sr-color-surface-paper: #efe6d3;"));
      const r = generator(dir, "--check");
      expect(r.status).toBe(1);
      expect(r.stderr).toContain("DRIFT src/tokens.css");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("scopes a palette for all six pages", () => {
    expect([...PAGE_IDS]).toEqual(["onair", "characters", "brain", "segments", "simulator", "reconcile"]);
    for (const id of PAGE_IDS) {
      const block = new RegExp(`\\[data-page="${id}"\\] \\{([^}]*)\\}`).exec(css)?.[1] ?? "";
      for (const role of Object.keys(src.pageRole)) expect(block, `${id} ${role}`).toMatch(new RegExp(`--sr-page-${role}: #[0-9a-f]{6};`));
    }
  });

  it("carries every role as a custom property, reachable through cssVar()", () => {
    for (const group of Object.keys(TOKENS) as TokenGroup[]) {
      for (const name of Object.keys(TOKENS[group]) as Array<TokenName<typeof group>>) {
        expect(css, `${group}.${name}`).toContain(`  ${cssVar(group, name)}: `);
      }
    }
    expect(cssVar("color", "surfacePaper")).toBe("--sr-color-surface-paper");
    expect(cssVar("texture", "halftoneACell")).toBe("--sr-halftone-a-cell");
    expect(cssRef("z", "toast")).toBe("var(--sr-z-toast)");
  });

  it("self-hosts Barlow Condensed with a real fallback stack", () => {
    expect(css.match(/@font-face \{/g)).toHaveLength(9);
    expect(css).toContain('src: url("../fonts/barlow-condensed-latin-700-normal.woff2") format("woff2");');
    expect(css).not.toMatch(/https?:\/\/(?!www\.w3\.org)/);
  });
});

describe("fidelity: byte-equal to the winner", () => {
  it("keeps the 14 distinct colours of poster.css :root (--bone aliases --paper)", () => {
    const c = TOKENS.color;
    expect([c.surfacePaper, c.surfaceChip, c.surfaceHoverPaper, c.chromeInk, c.textInk, c.textMute]).toEqual([
      "#efe6d2", "#e3d6b8", "#d4c5a1", "#0b0f0e", "#161411", "#6b5f4a",
    ]);
    expect([c.signal, c.signalDeep, c.okBright, c.foil, c.cautionEdge, c.fail]).toEqual([
      "#e8542a", "#b53a14", "#86d3bb", "#d6a85a", "#7d5f12", "#8e2a1a",
    ]);
    expect(src.primitive.color?.teal).toBe("#2f7d6a");
    expect(src.primitive.color?.["teal-deep"]).toBe("#14463d");
  });

  it("keeps app.js PAGES (f, t, o, o2, k) and the spread accents SA", () => {
    const winner: Record<string, string[]> = {
      onair: ["#e8542a", "#0b0f0e", "#2f7d6a", "#efe6d2", "#0b0f0e", "#e8542a", "#b53a14"],
      characters: ["#efe6d2", "#0b0f0e", "#e8542a", "#2f7d6a", "#0b0f0e", "#2f7d6a", "#1b6150"],
      brain: ["#14463d", "#efe6d2", "#e8542a", "#86d3bb", "#06110e", "#3a9b86", "#1b6150"],
      segments: ["#d6a85a", "#0b0f0e", "#a8321c", "#efe6d2", "#0b0f0e", "#d6a85a", "#7d5f12"],
      simulator: ["#8e2a1a", "#efe6d2", "#d6a85a", "#e8542a", "#0b0f0e", "#c9452c", "#8e2a1a"],
      reconcile: ["#0b0f0e", "#efe6d2", "#2f7d6a", "#e8542a", "#0b0f0e", "#e8542a", "#b53a14"],
    };
    for (const id of PAGE_IDS) {
      const p = PAGE_PALETTES[id];
      expect([p.face, p.type, p.overprint, p.overprint2, p.key, p.accent, p.accentText], id).toEqual(winner[id]);
    }
  });

  it("keeps sr-kit.js RANK_COLORS and GRADE_COLORS", () => {
    expect(RANK_COLORS).toEqual({
      thrall: "#8f877a",
      acolyte: "#cdbf9f",
      cultist: "#8fc4b0",
      warlock: "#3f9c86",
      lich: "#e8913f",
      dread_council: "#ff5a2a",
    });
    expect(GRADE_COLORS).toEqual({ DOC: "#86d3bb", REP: "#cdbf9f", CLM: "#e8913f", SPC: "#ff5a2a" });
  });

  it("keeps the font stacks, easings, print texture and wall geometry", () => {
    expect(TOKENS.font.display).toBe('"Barlow Condensed","Bahnschrift SemiCondensed","Arial Narrow","Roboto Condensed",sans-serif');
    expect(TOKENS.font.serif).toBe('"Palatino Linotype",Palatino,"Iowan Old Style","Book Antiqua",Georgia,serif');
    expect(TOKENS.font.sans).toBe('"Segoe UI",system-ui,-apple-system,"Helvetica Neue",Arial,sans-serif');
    expect(TOKENS.font.mono).toBe('"Cascadia Mono",Consolas,"SFMono-Regular",Menlo,monospace');
    expect([TOKENS.ease.move, TOKENS.ease.settle, TOKENS.ease.slam]).toEqual([
      "cubic-bezier(.7,0,.2,1)",
      "cubic-bezier(.2,.8,.2,1)",
      "cubic-bezier(.2,.95,.25,1.12)",
    ]);
    expect(TOKENS.texture.grainImage).toContain("baseFrequency='.85' numOctaves='2' stitchTiles='stitch'");
    expect([TOKENS.texture.grainOpacityPoster, TOKENS.texture.grainOpacitySpread]).toEqual([".4", ".32"]);
    expect([TOKENS.texture.halftoneACell, TOKENS.texture.halftoneADot, TOKENS.texture.halftoneAEdge]).toEqual([".8rem", "26%", "28%"]);
    expect([TOKENS.texture.halftoneBCell, TOKENS.texture.halftoneBDot, TOKENS.texture.halftoneBEdge]).toEqual(["1.1rem", "22%", "24%"]);
    expect([TOKENS.layout.spine, TOKENS.layout.strip]).toEqual(["18.75rem", "3.6rem"]);
    expect(TOKENS.alpha.line).toBe("rgba(20,17,13,.2)");
    expect(TOKENS.channel.paper).toBe("239 230 210");
    expect(BREAKPOINTS_PX.narrow).toBe(760);
  });

  it("reproduces the winner's viewport type scaling in CSS (16px at 1280x800, max 1.9x, 16px when narrow)", () => {
    expect(TOKENS.root.size).toBe("clamp(16px,min(1.25vw,2vh),30.4px)");
    expect(css).toMatch(/@media \(max-width: 760px\) \{\n {2}:root \{\n {4}--sr-root-size: 16px;/);
  });
});
