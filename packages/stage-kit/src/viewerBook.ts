// A viewer-keyed book (viewer id -> value) for the scene's ballot and grade books: immutable, so
// every scene the Timeline, its keyframes or a page retains stays valid, and cheap to extend.
// Copying a whole object per ballot made a big vote fold quadratically (8000 distinct `!vote`
// lines took 10 s at 829eb0a); here a write copies at most the young generation (BOOK_YOUNG_MAX
// entries), and full young generations merge into levels whose sizes fall geometrically from
// oldest to newest, like a binary counter, so each entry is copied O(log n) times over a whole
// fold and a read checks O(log n) levels.
//
// ChatMemory (chatMemory.ts) keeps its own two-generation map: its old generation is bounded by
// CHAT_MEMORY_MAX and evicts, so one merge level suffices there. A book is unbounded - it holds
// one entry per distinct voter, as the Record books did. A value is never undefined: a read
// takes undefined to mean "not in this map, look older".

/** Entries written between merges (the per-write copy never exceeds this). */
export const BOOK_YOUNG_MAX = 32;

export class ViewerBook<V extends NonNullable<unknown>> {
  /** Merged levels, oldest first, each at most half the one before; the young generation last. */
  private readonly maps: readonly ReadonlyMap<string, V>[];
  /** Distinct viewer ids. */
  readonly size: number;

  private constructor(maps: readonly ReadonlyMap<string, V>[], size: number) {
    this.maps = maps;
    this.size = size;
  }

  private static readonly NONE = new ViewerBook<never>([new Map<string, never>()], 0);

  /** The empty book (one shared instance; it is immutable). */
  static empty<V extends NonNullable<unknown>>(): ViewerBook<V> {
    return ViewerBook.NONE as ViewerBook<V>;
  }

  /** How many maps a read may check: the merged levels plus the young generation. */
  get depth(): number {
    return this.maps.length;
  }

  get(id: string): V | undefined {
    for (let i = this.maps.length; i--; ) {
      const v = this.maps[i]!.get(id);
      if (v !== undefined) return v;
    }
    return undefined;
  }

  has(id: string): boolean {
    return this.get(id) !== undefined;
  }

  /** This book with `id` set to `value` (a later write for the same viewer replaces the earlier). */
  set(id: string, value: V): ViewerBook<V> {
    const size = this.size + (this.has(id) ? 0 : 1);
    const levels = this.maps.slice(0, -1);
    let run = new Map(this.maps[levels.length]).set(id, value);
    if (run.size < BOOK_YOUNG_MAX) return new ViewerBook([...levels, run], size);
    // Merge the full young generation down: absorb every newest level no bigger than the run,
    // so level sizes stay geometric and each entry is copied O(log n) times in all.
    while (levels.length > 0 && levels[levels.length - 1]!.size <= run.size) {
      const older = new Map(levels.pop());
      for (const [k, v] of run) older.set(k, v);
      run = older;
    }
    return new ViewerBook([...levels, run, new Map()], size);
  }
}
