// A page's declared URL grammar: one segment tree read by the parser, the breadcrumbs, the
// shell's rise key and the unknown-sub fallback.
import { describe, expect, it } from "vitest";
import { matchSub, schemaLabel, type RouteSchema } from "./schema";

/** A toy grammar: `list`, `item/:id[/:n]` (n numeric, in place), `about`. */
const TOY: RouteSchema = [
  { seg: "list", label: "The list" },
  {
    seg: "item",
    children: [{ param: "id", label: (v) => `#${v}`, children: [{ param: "n", test: (v) => /^\d+$/.test(v), label: (v) => `n${v}`, inPlace: true }] }],
  },
  { seg: "about" },
];

describe("matchSub", () => {
  it("matches the empty sub (layer 1) with no params", () => {
    expect(matchSub(TOY, [])).toEqual({ ok: true, params: {}, riseDepth: 0 });
  });

  it("reads params by name and stops the rise at the last segment that is not in place", () => {
    expect(matchSub(TOY, ["item", "a/b", "3"])).toEqual({ ok: true, params: { id: "a/b", n: "3" }, riseDepth: 2 });
    expect(matchSub(TOY, ["item", "x"])).toEqual({ ok: true, params: { id: "x" }, riseDepth: 2 });
    expect(matchSub(TOY, ["list"])).toEqual({ ok: true, params: {}, riseDepth: 1 });
  });

  it("prefers a literal over a param at the same depth", () => {
    const s: RouteSchema = [{ param: "id" }, { seg: "new", label: "New" }];
    expect(matchSub(s, ["new"])).toEqual({ ok: true, params: {}, riseDepth: 1 });
    expect(matchSub(s, ["old"])).toEqual({ ok: true, params: { id: "old" }, riseDepth: 1 });
  });

  it("falls back to the deepest valid prefix when a segment names nothing", () => {
    expect(matchSub(TOY, ["nonsense"])).toEqual({ ok: false, fallback: [] });
    expect(matchSub(TOY, ["item", "x", "notanumber"])).toEqual({ ok: false, fallback: ["item", "x"] });
    expect(matchSub(TOY, ["list", "extra", "more"])).toEqual({ ok: false, fallback: ["list"] });
  });
});

describe("schemaLabel", () => {
  it("labels literals by name, params by their label function, and leaves the rest to humanize", () => {
    const label = schemaLabel(TOY);
    const parts = ["page", "item", "a/b", "3"];
    expect(label("item", 1, parts)).toBeUndefined();
    expect(label("a/b", 2, parts)).toBe("#a/b");
    expect(label("3", 3, parts)).toBe("n3");
    expect(label("list", 1, ["page", "list"])).toBe("The list");
    expect(label("x", 2, ["page", "nope", "x"])).toBeUndefined();
  });
});
