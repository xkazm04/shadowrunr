// The lifecycle vocabulary and counts. Kept apart from model.ts because the wall's glance chips
// need them, and whatever the glance imports ships with the entry chunk.
import type { SegmentStatus, SegmentTableInfo } from "@shadowrunr/contract";

/** The winner's lifecycle columns, in order (sr-kit.js `SR.LIFECYCLE`). */
export const LIFECYCLE = ["pilot", "trial", "regular", "flagship", "retired"] as const;
export type Lifecycle = (typeof LIFECYCLE)[number];

export function statusCounts(segments: readonly SegmentTableInfo[]): Readonly<Record<SegmentStatus, number>> {
  const out: Record<SegmentStatus, number> = { pilot: 0, trial: 0, regular: 0, flagship: 0, retired: 0, descoped: 0 };
  for (const s of segments) out[s.status] += 1;
  return out;
}

/** The glance chips on the wall poster ("2 pilot · 2 descoped"), only the non-zero states. */
export function glance(segments: readonly SegmentTableInfo[]): string[] {
  const c = statusCounts(segments);
  return ([...LIFECYCLE, "descoped"] as const).filter((s) => c[s] > 0).map((s) => `${c[s]} ${s}`);
}
