// Layer 1's bounded read window: the newest tournament on the first `tournaments.page`, up to
// `RUN_PAGES` pages of its runs and `LEDGER_PAGES` pages of its ledger, and one sample transcript
// per lane. The window is what the network costs; the model aggregates whatever arrives, so the
// canvas cost does not grow with the season (48k runs) either.
import { useMemo } from "react";
import type { RunParams, RunRow, TournamentsPage } from "@shadowrunr/contract";
import { usePaged, useRpc, useRpcMany, type Many, type Query } from "../../data";
import { buildBrainModel, buildLanes, type BrainModel } from "./brainModel";

export const WINDOW = { tournaments: 500, runPages: 4, ledgerPages: 12, pageSize: 500 } as const;

export interface BrainWindow {
  readonly tournaments: Query<TournamentsPage>;
  readonly tournamentId: string | undefined;
  readonly model: BrainModel | undefined;
  /** The ledger or the run list had more pages than the window reads. */
  readonly truncated: boolean;
  /** A follow-up read failed while the first one answered. */
  readonly degraded: { readonly errorView: Many<unknown>["errorView"]; readonly reload: () => void } | undefined;
}

/** The tournament the brain shows by default: the last one the daemon lists. */
export function latestTournament(items: readonly { tournament_id: string }[]): string | undefined {
  return items[items.length - 1]?.tournament_id;
}

/** One sample run per lane, in lane order (the first run the lane holds). */
export function laneSamples(runs: readonly RunRow[]): RunParams[] {
  const lanes = buildLanes(runs, []);
  const byId = new Map(runs.map((r) => [r.run_id, r]));
  const out: RunParams[] = [];
  for (const lane of lanes) {
    const r = lane.runIds[0] === undefined ? undefined : byId.get(lane.runIds[0]);
    if (r) out.push({ run_id: r.run_id, tournament_id: r.tournament_id });
  }
  return out;
}

export function useBrainWindow(): BrainWindow {
  const tournaments = useRpc("tournaments.page", { limit: WINDOW.tournaments });
  const tid = tournaments.data ? latestTournament(tournaments.data.items) : undefined;
  const base = tid === undefined ? null : { tournament_id: tid, limit: WINDOW.pageSize };
  const runs = usePaged("runs.page", base, WINDOW.runPages);
  const ledger = usePaged("ledger.page", base, WINDOW.ledgerPages);
  const runItems = runs.query.data;
  const ledgerItems = ledger.query.data;
  const samples = useMemo(() => laneSamples(runItems ?? []), [runItems]);
  const transcripts = useRpcMany("run.transcript", samples);
  const model = useMemo(
    () => (runItems !== undefined ? buildBrainModel(runItems, ledgerItems ?? [], transcripts.data) : undefined),
    [runItems, ledgerItems, transcripts.data],
  );
  const degraded = [runs.query, ledger.query, transcripts].find((q) => q.error !== undefined);
  return {
    tournaments,
    tournamentId: tid,
    model,
    truncated: runs.more || ledger.more,
    degraded,
  };
}
