// The package's one evidence source: which tournament its verdicts come from and that
// tournament's results, read by the Situations tab (per-situation tallies) and the Tones tab
// (per-tone records). One choice, so the two tabs never name different tournaments: the
// operator's pick on Situations is what the Tones cards and the Make active preview cite.
// The pick lives in memory per character (never localStorage), like ui/tableView: switching
// tabs remounts the page body, and the pick must outlive that.
import { useMemo, useSyncExternalStore } from "react";
import type { CharacterDetail, TournamentResults, TournamentRow } from "@shadowrunr/contract";
import { useRpc } from "../../data";
import { offeredTournaments, pickTournament, toneEvidence, type ToneRecord } from "./model";

const picks = new Map<string, string>();
const listeners = new Set<() => void>();

export const tournamentPick = {
  get: (character: string): string | undefined => picks.get(character),
  set(character: string, tournamentId: string): void {
    picks.set(character, tournamentId);
    for (const l of listeners) l();
  },
  subscribe(l: () => void): () => void {
    listeners.add(l);
    return () => listeners.delete(l);
  },
  /** Tests only: forget every pick. */
  reset(): void {
    picks.clear();
    for (const l of listeners) l();
  },
};

export interface CharacterEvidence {
  /** This character's tournaments, then ones that do not say whose they are; newest first. */
  readonly offered: readonly TournamentRow[];
  /** The tournament read: the operator's pick if still offered, else this character's newest. */
  readonly tid: string | undefined;
  readonly pick: (tournamentId: string) => void;
  /** Rows `tournament.list` served (any character); undefined until it answers. */
  readonly listed: number | undefined;
  readonly listError: string | undefined;
  readonly results: TournamentResults | undefined;
  readonly resultsError: string | undefined;
  /** Each tone's record in `tid`; undefined until the results arrive (or with no tournament). */
  readonly tones: ReadonlyMap<string, ToneRecord> | undefined;
}

export function useCharacterEvidence(d: Pick<CharacterDetail, "id">): CharacterEvidence {
  const tournaments = useRpc("tournament.list", {});
  const picked = useSyncExternalStore(tournamentPick.subscribe, () => tournamentPick.get(d.id));
  const offered = useMemo(() => offeredTournaments(tournaments.data ?? [], d.id), [tournaments.data, d.id]);
  const tid = picked !== undefined && offered.some((t) => t.tournament_id === picked) ? picked : pickTournament(offered, d.id)?.tournament_id;
  const results = useRpc("tournament.results", tid === undefined ? null : { tournament_id: tid });
  const tones = useMemo(() => (results.data ? toneEvidence(results.data) : undefined), [results.data]);
  return {
    offered,
    tid,
    pick: (id) => tournamentPick.set(d.id, id),
    listed: tournaments.data?.length,
    listError: tournaments.errorView?.title,
    results: tid === undefined ? undefined : results.data,
    resultsError: results.errorView?.title,
    tones: tid === undefined ? undefined : tones,
  };
}
