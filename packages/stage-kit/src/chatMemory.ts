// The scene's chat memory: an immutable map of recent chat lines by id, cheap to extend. New
// lines go into a small young generation that is copied per line; every YOUNG_MAX lines it is
// merged into the shared old generation, and the oldest lines nothing on screen points at are
// forgotten. A 100k-line transcript therefore folds in linear time with bounded memory
// (copying a 600-entry object per line made it quadratic).

import type { ChatMsg } from "@shadowrunr/contract";

/** Lines added between merges (the per-line copy never exceeds this). */
export const YOUNG_MAX = 64;

export class ChatMemory {
  private readonly old: ReadonlyMap<string, ChatMsg>;
  private readonly young: ReadonlyMap<string, ChatMsg>;
  /** Distinct ids across both generations. */
  readonly size: number;

  private constructor(old: ReadonlyMap<string, ChatMsg>, young: ReadonlyMap<string, ChatMsg>, size: number) {
    this.old = old;
    this.young = young;
    this.size = size;
  }

  static readonly EMPTY = new ChatMemory(new Map(), new Map(), 0);

  get(id: string): ChatMsg | undefined {
    return this.young.get(id) ?? this.old.get(id);
  }

  has(id: string): boolean {
    return this.young.has(id) || this.old.has(id);
  }

  /** Ids, oldest first. */
  *keys(): IterableIterator<string> {
    for (const id of this.old.keys()) yield id;
    for (const id of this.young.keys()) if (!this.old.has(id)) yield id;
  }

  /** Lines, oldest first (a re-sent id keeps its first position with its latest text). */
  *values(): IterableIterator<ChatMsg> {
    for (const id of this.keys()) {
      const m = this.get(id);
      if (m) yield m;
    }
  }

  /**
   * This memory plus `msg`. On a merge, lines beyond the newest `max` are forgotten unless
   * `keep` holds their id (on screen, or lifted into a decree).
   */
  with(msg: ChatMsg, max: number, keep: () => ReadonlySet<string>): ChatMemory {
    const known = this.has(msg.id);
    const size = this.size + (known ? 0 : 1);
    if (this.young.size < YOUNG_MAX) {
      return new ChatMemory(this.old, new Map(this.young).set(msg.id, msg), size);
    }
    const merged = new Map(this.old);
    for (const [id, m] of this.young) merged.set(id, m);
    merged.set(msg.id, msg);
    if (merged.size <= max) return new ChatMemory(merged, new Map(), merged.size);
    const pinned = keep();
    const ids = [...merged.keys()];
    const recent = new Set(ids.slice(-max));
    const kept = new Map<string, ChatMsg>();
    for (const [id, m] of merged) if (recent.has(id) || pinned.has(id)) kept.set(id, m);
    return new ChatMemory(kept, new Map(), kept.size);
  }
}
