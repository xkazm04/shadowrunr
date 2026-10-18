// The heatmap cell's route key and label. Its own small module because the page index reads
// it for breadcrumbs, and the index sits in the entry chunk: nothing heavier may ride along.
import type { CellKey } from "./results";

export function cellLabel(k: CellKey): string {
  return `${k.tone} · ${k.core_tier} · ${k.segment} · a${k.audience_size}`;
}

/** Heatmap route key: the tournament plus the full cell key, one route segment. */
export function cellRouteKey(tournamentId: string, k: CellKey): string {
  return [tournamentId, k.instrument_hash, k.tone, k.core_tier, k.segment, String(k.audience_size)].join("|");
}

export interface ParsedCellKey {
  readonly tournamentId: string;
  readonly cell: CellKey;
}

export function parseCellRouteKey(key: string): ParsedCellKey | undefined {
  const parts = key.split("|");
  if (parts.length !== 6) return undefined;
  const [tournamentId, instrument_hash, tone, core_tier, segment, aud] = parts;
  const audience_size = Number(aud);
  if (!tournamentId || tone === undefined || core_tier === undefined || segment === undefined || !Number.isInteger(audience_size)) {
    return undefined;
  }
  return { tournamentId, cell: { instrument_hash: instrument_hash ?? "", tone, core_tier, segment, audience_size } };
}
