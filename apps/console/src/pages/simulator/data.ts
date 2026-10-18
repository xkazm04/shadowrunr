// The simulator's data hooks over the shared data layer: the cursor chains and the call sets
// of `data/paged`, folded into what the grid and the tournament pages read.
import { useMemo } from "react";
import type { TournamentRow } from "@shadowrunr/contract";
import { usePaged, usePagedMany, useRpcMany, type Paged } from "../../data";
import { buildTile, tilesInView, type Tile } from "./grid";
import type { SimResults } from "./results";

/** The daemon caps a page at 500 rows. */
export const PAGE_LIMIT = 500;

/** Tournament results by id, for the ids whose reply has arrived. */
export function useResults(ids: readonly string[]): ReadonlyMap<string, SimResults> {
  const idsKey = ids.join("\n");
  // `idsKey` carries `ids` by value.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const list = useMemo(() => ids.map((tournament_id) => ({ tournament_id })), [idsKey]);
  const many = useRpcMany("tournament.results", list);
  return useMemo(() => {
    const out = new Map<string, SimResults>();
    many.data.forEach((r, i) => {
      const id = list[i]?.tournament_id;
      if (id !== undefined && r !== undefined) out.set(id, r);
    });
    return out;
  }, [many.data, list]);
}

export interface SimGrid {
  /** The tournaments page chain: drives the page states. */
  readonly tournaments: Paged<TournamentRow>;
  /** One tile per tournament in view (the latest 40). */
  readonly tiles: readonly Tile[];
  /** Every page has arrived. */
  readonly complete: boolean;
}

/**
 * Layer 1's data: the latest 40 tournaments and all their runs. Each run row carries its own
 * health and red-line fails (from its verdict), so the grid needs no `tournament.results`.
 */
export function useSimGrid(): SimGrid {
  const tournaments = usePaged("tournaments.page", { limit: PAGE_LIMIT });
  const all = tournaments.query.data;
  const inView = useMemo(() => tilesInView(all ?? []), [all]);
  const runBases = useMemo(() => inView.map((t) => ({ tournament_id: t.tournament_id, limit: PAGE_LIMIT })), [inView]);
  const runs = usePagedMany("runs.page", runBases);
  const tiles = useMemo(() => inView.map((t, i) => buildTile(t.tournament_id, t.runs, runs[i]?.query.data ?? [])), [inView, runs]);
  const complete = tournaments.done && runs.every((r) => r.done);
  return { tournaments, tiles, complete };
}
