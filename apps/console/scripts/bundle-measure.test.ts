// Acceptance cases for the bundle gate's compare mode (synthetic manifests; no build needed).
// Vite's shared-chunk hashes are 8 characters, so the synthetic keys use 8-character hashes.
import { describe, expect, it } from "vitest";
// @ts-expect-error - a plain .mjs build script with no type declarations
import { baselineOf, BASELINE_HINT, chunkKey, diffRoutes, measureRoutes, report } from "./bundle-measure.mjs";

type Manifest = Record<string, { file: string; isEntry?: boolean; isDynamicEntry?: boolean; imports?: string[]; css?: string[] }>;

const SIZES: Record<string, number> = {
  "assets/index.js": 1000,
  "assets/Button-AAAAAAAA.js": 4000,
  "assets/Layer2.js": 60000,
  "assets/Compare-BBBBBBBB.js": 572,
  "assets/index.css": 300,
  "assets/Layer2.css": 50,
};
const sizeOf = (f: string) => SIZES[f] ?? 0;

const manifest: Manifest = {
  "index.html": { file: "assets/index.js", isEntry: true, imports: ["_Button-AAAAAAAA.js"], css: ["assets/index.css"] },
  "_Button-AAAAAAAA.js": { file: "assets/Button-AAAAAAAA.js" },
  "src/pages/review/Layer2.tsx": {
    file: "assets/Layer2.js",
    isDynamicEntry: true,
    imports: ["_Button-AAAAAAAA.js", "_Compare-BBBBBBBB.js"],
    css: ["assets/Layer2.css"],
  },
  "_Compare-BBBBBBBB.js": { file: "assets/Compare-BBBBBBBB.js" },
};

describe("bundle-measure", () => {
  it("measures a route beyond the entry with a hash-free per-chunk list", () => {
    const { routes } = measureRoutes(manifest, sizeOf);
    expect(routes["review.l2"].js).toBe(60572);
    expect(routes["review.l2"].css).toBe(50);
    expect(routes["review.l2"].chunks).toEqual({ "src/pages/review/Layer2.tsx": 60000, "_Compare.js": 572 });
    expect(routes.entry.chunks).toEqual({ "index.html": 1000, "_Button.js": 4000 });
  });

  it("chunkKey strips a shared chunk's content hash and leaves source keys alone", () => {
    expect(chunkKey("_FilterTable-C7LN3YTt.js")).toBe("_FilterTable.js");
    expect(chunkKey("_FilterTable-Zz9Qq1Aa.js")).toBe("_FilterTable.js");
    expect(chunkKey("_StatBand-1bNxvU-n.js")).toBe("_StatBand.js");
    expect(chunkKey("src/pages/review/Layer2.tsx")).toBe("src/pages/review/Layer2.tsx");
  });

  it("two distinct shared chunks with the same stem stay two entries, never summed", () => {
    const m: Manifest = {
      "index.html": { file: "assets/index.js", isEntry: true },
      "src/pages/review/Layer2.tsx": {
        file: "assets/Layer2.js",
        isDynamicEntry: true,
        imports: ["_index-AAAAAAAA.js", "_index-BBBBBBBB.js"],
      },
      "_index-AAAAAAAA.js": { file: "assets/index-AAAAAAAA.js" },
      "_index-BBBBBBBB.js": { file: "assets/index-BBBBBBBB.js", imports: ["_index-AAAAAAAA.js"] },
    };
    const sizes: Record<string, number> = { "assets/Layer2.js": 10, "assets/index-AAAAAAAA.js": 100, "assets/index-BBBBBBBB.js": 200 };
    const { routes } = measureRoutes(m, (f: string) => sizes[f] ?? 0);
    const chunks = routes["review.l2"].chunks as Record<string, number>;
    expect(Object.keys(chunks)).toHaveLength(3);
    expect(chunks["_index.js"]).toBeUndefined();
    expect(Object.values(chunks).sort((a, b) => a - b)).toEqual([10, 100, 200]);
    // stable across builds: the same graph under new hashes gets the same names
    const rehashed = JSON.parse(JSON.stringify(m).replaceAll("AAAAAAAA", "CCCCCCCC").replaceAll("BBBBBBBB", "DDDDDDDD")) as Manifest;
    const sizes2: Record<string, number> = { "assets/Layer2.js": 10, "assets/index-CCCCCCCC.js": 100, "assets/index-DDDDDDDD.js": 200 };
    const again = measureRoutes(rehashed, (f: string) => sizes2[f] ?? 0).routes["review.l2"].chunks;
    expect(again).toEqual(chunks);
  });

  it("diffRoutes names what grew, sorted by |delta|, unchanged chunks omitted", () => {
    const d = diffRoutes(
      { "review.l2": { js: 95630, chunks: { "src/pages/review/Layer2.tsx": 60000, "_Button.js": 4000 } } },
      { "review.l2": { js: 104202, chunks: { "src/pages/review/Layer2.tsx": 68000, "_Button.js": 4000, "_Compare.js": 572 } } },
    );
    expect(d["review.l2"].delta).toBe(8572);
    expect(d["review.l2"].rows).toEqual([
      { chunk: "src/pages/review/Layer2.tsx", delta: 8000 },
      { chunk: "_Compare.js", delta: 572, new: true },
    ]);
  });

  it("an over-budget route is followed by its top-3 chunk deltas and the slack line; exit stays 1", () => {
    const routes = {
      "review.l2": { js: 104202, css: 0, gzip: 0, chunks: { "a.tsx": 68000, "_B.js": 4000, "_C.js": 572, "_D.js": 31630 } },
    };
    const baseline = { "review.l2": { js: 90000, css: 0, chunks: { "a.tsx": 60000, "_B.js": 3000, "_D.js": 27000 } } };
    const r = report(routes, { "review.l2": { js: 100000 } }, baseline);
    expect(r.code).toBe(1);
    expect(r.err).toEqual([
      "  review.l2: js 104202 B > budget 100000 B",
      "    grew +14202 B vs bundle.measured.json: a.tsx +8000 B, _D.js +4630 B, _B.js +1000 B",
      "    slack -4202 B (budget 100000 - measured 104202)",
    ]);
  });

  it("with no baseline the report says how to write one and the verdict is unchanged", () => {
    const routes = { entry: { js: 10, css: 0, gzip: 0, chunks: { "index.html": 10 } } };
    const r = report(routes, { entry: { js: 100 } }, null);
    expect(r.code).toBe(0);
    expect(r.out).toContain(BASELINE_HINT);
    expect(BASELINE_HINT).toBe("no baseline: run pnpm -C apps/console budget:bundle --write-baseline");
  });

  it("a written baseline carries js/css/per-chunk js and reads back as delta 0 everywhere", () => {
    const { routes } = measureRoutes(manifest, sizeOf);
    const base = JSON.parse(JSON.stringify(baselineOf(routes)));
    expect(base["review.l2"]).toEqual({ js: 60572, css: 50, chunks: { "_Compare.js": 572, "src/pages/review/Layer2.tsx": 60000 } });
    const d = diffRoutes(base, routes);
    for (const name of Object.keys(routes)) expect(d[name]).toEqual({ delta: 0, rows: [] });
  });

  // GUARD: the verdict and failure lines are today's, whatever the baseline says.
  it("guard: the verdict is budget-only and the failure heads are unchanged", () => {
    const routes = {
      entry: { js: 10, css: 50, gzip: 0, chunks: {} },
      "x.l1": { js: 5, css: 0, gzip: 0, chunks: {} },
    };
    const budgets = { entry: { js: 100, css: 40 }, "y.l1": { js: 1 } };
    const stale = { entry: { js: 99999, css: 0, chunks: { zzz: 99999 } } };
    for (const baseline of [null, stale]) {
      const r = report(routes, budgets, baseline);
      expect(r.code).toBe(1);
      expect(r.err).toEqual(["  entry: css 50 B > budget 40 B", "  x.l1: no budget in budgets/bundle.json", "  y.l1: budgeted but no such route was built"]);
      expect(r.out).toContain("check-bundle: 2 routes measured, 3 over or unbudgeted");
    }
    expect(report({ entry: routes.entry }, { entry: { js: 100 } }, stale).code).toBe(0);
  });
});
