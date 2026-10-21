// The simulator's data hooks over the shared data layer: the cursor chains and the call sets
// of `data/paged`, folded into what the grid and the tournament pages read.
import { useMemo } from "react";
import type { TournamentRow } from "@shadowrunr/contract";
import { usePaged, usePagedMany, useSameList, type Paged } from "../../data";
import { buildTile, tilesInView, type Tile } from "./grid";

/** The daemon caps a page at 500 rows. */
export const PAGE_LIMIT = 500;

export interface SimGrid {
  /** The tournaments page chain: drives the page states. */
  readonly tournaments: Paged<TournamentRow>;
  /** One tile per tournament in view (the latest 40). */
  readonly tiles: readonly Tile[];
  /** Every page has arrived. */
  readonly complete: boolean;
}

/** The tournament rows, by id: the list and the detail read one tournament's held from here. */
export function useTournamentRow(id: string): TournamentRow | undefined {
  const paged = usePaged("tournaments.page", { limit: PAGE_LIMIT });
  return useMemo(() => paged.query.data?.find((t) => t.tournament_id === id), [paged.query.data, id]);
}

/**
 * Layer 1's data: the latest 40 tournaments and all their runs. A tile's held and red-line fails
 * are its row's; each run row carries its own (from its verdict), so the grid needs no
 * `tournament.results`.
 */
export function useSimGrid(): SimGrid {
  const tournaments = usePaged("tournaments.page", { limit: PAGE_LIMIT });
  const all = tournaments.query.data;
  const inView = useMemo(() => tilesInView(all ?? []), [all]);
  const runBases = useMemo(() => inView.map((t) => ({ tournament_id: t.tournament_id, limit: PAGE_LIMIT })), [inView]);
  const runs = usePagedMany("runs.page", runBases);
  // Keyed on the replies, not the chains: a refresh flag flipping must not redraw the grid.
  const runData = useSameList(runs.map((r) => r.query.data));
  const tiles = useMemo(() => inView.map((t, i) => buildTile(t, runData[i] ?? [])), [inView, runData]);
  const complete = tournaments.done && runs.every((r) => r.done);
  return { tournaments, tiles, complete };
}
