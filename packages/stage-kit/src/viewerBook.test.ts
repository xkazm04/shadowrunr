import { describe, expect, it } from "vitest";

import { BOOK_YOUNG_MAX, ViewerBook } from "./viewerBook";

/** A book built by writing `ids` in order, each id's value its write index. */
function build(ids: readonly string[]): ViewerBook<number> {
  let b = ViewerBook.empty<number>();
  ids.forEach((id, i) => {
    b = b.set(id, i);
  });
  return b;
}

describe("ViewerBook", () => {
  it("gets, overwrites and counts distinct viewers across merges", () => {
    const n = BOOK_YOUNG_MAX * 5 + 7;
    let b = ViewerBook.empty<string>();
    for (let i = 0; i < n; i++) b = b.set(`v${i}`, "A");
    // Re-write every third viewer after its first write has merged down.
    for (let i = 0; i < n; i += 3) b = b.set(`v${i}`, "B");
    expect(b.size).toBe(n);
    for (let i = 0; i < n; i++) expect(b.get(`v${i}`)).toBe(i % 3 === 0 ? "B" : "A");
    expect(b.get("nobody")).toBeUndefined();
    expect(b.has("v0")).toBe(true);
    expect(b.has("nobody")).toBe(false);
  });

  it("returns each viewer's latest value after its first write merged down", () => {
    const ids = Array.from({ length: BOOK_YOUNG_MAX * 3 }, (_, i) => `v${i}`);
    const b = build([...ids, "v5", "v0", "late"]);
    expect(b.size).toBe(ids.length + 1);
    expect(b.get("v5")).toBe(ids.length);
    expect(b.get("v0")).toBe(ids.length + 1);
    expect(b.get("v1")).toBe(1);
    expect(b.get("late")).toBe(ids.length + 2);
  });

  it("is persistent: every earlier book keeps its contents after later writes and merges", () => {
    const books: ViewerBook<number>[] = [];
    let b = ViewerBook.empty<number>();
    for (let i = 0; i < BOOK_YOUNG_MAX * 4; i++) {
      books.push(b);
      b = b.set(`v${i % 50}`, i);
    }
    books.forEach((old, k) => {
      expect(old.size).toBe(Math.min(k, 50));
      // Viewer v(k-1) was last written at write k-1 in the book after it.
      if (k > 0) expect(old.get(`v${(k - 1) % 50}`)).toBe(k - 1);
      expect(old.has(`v${k % 50}`)).toBe(k >= 50);
    });
    // Branching from an old book never disturbs the newer one.
    const branch = books[10]!.set("v3", -1);
    expect(branch.get("v3")).toBe(-1);
    expect(b.get("v3")).not.toBe(-1);
    expect(ViewerBook.empty<number>().size).toBe(0);
  });

  it("keeps reads logarithmic: 100k distinct writes leave few levels and fold in linear-ish time", () => {
    const n = 100_000;
    let b = ViewerBook.empty<number>();
    let deepest = 0;
    const t0 = performance.now();
    for (let i = 0; i < n; i++) {
      b = b.set(`v${i}`, i);
      deepest = Math.max(deepest, b.depth);
    }
    const ms = performance.now() - t0;
    expect(b.size).toBe(n);
    // Geometric levels: at most one per halving of n / BOOK_YOUNG_MAX, plus the young map.
    expect(deepest).toBeLessThanOrEqual(Math.ceil(Math.log2(n / BOOK_YOUNG_MAX)) + 2);
    // Copying a 100k-entry object per write would take minutes; the leveled book takes ~100 ms.
    expect(ms).toBeLessThan(5_000);
    expect(b.get("v0")).toBe(0);
    expect(b.get(`v${n - 1}`)).toBe(n - 1);
  }, 60_000);
});
