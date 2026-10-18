// The 10x fixture (test-only, imported by src/perf/*.test.tsx only): one deterministic world at
// the scale the port is held to, built from the page tests' own generators (imported, never
// copied) and served through the data-layer test double as one scripted daemon per page.
// Every value is generated and fictional; nothing here reaches a bundle.
//
// Scale (port addendum, Q4): 2,000 streams, 48k runs, 100k transcript lines, and the simulator
// grid at A/1's 40 tournaments x 120 runs. "Streams" are what a stream needs a row for: 2,000
// characters on the roster, 2,000 held and 2,000 failed findings in one Reconcile report, 2,000
// segments in the tables and 2,000 standing directives.
import type { RunRow, SegmentTables, StreamEvent } from "@shadowrunr/contract";
import { fail, ok, page, param, type Script } from "../kit/testing";
import { daemon as brainDaemon, season, transcript as brainTranscript, type Season } from "../pages/brain/testgen";
import { genResults, genRoster, malakorDetail, malakorRules } from "../pages/characters/testgen";
import { runRow as onairRunRow, TABLES as ONAIR_TABLES, transcript as onairTranscript } from "../pages/onair/testdata";
import { genDirectives, genProposals, genReport } from "../pages/reconcile/testgen";
import { genRanks, genTables } from "../pages/segments/testgen";
import { daemon as simDaemon, world, type World } from "../pages/simulator/testgen";

export const SCALE = {
  streams: 2_000,
  runs: 48_000,
  transcriptLines: 100_000,
  /** The simulator grid at A/1's scale. */
  tournaments: 40,
  runsPerTournament: 120,
  /** 48k runs as 40 tournaments of 1,200 (the run lists and the 10x grid). */
  runsPerTournament10x: 1_200,
  /** Reconcile incidents at 10x the pilot's 417. */
  incidents: 4_170,
  /** Ledger rows per run the brain's window reads (the pilot logged ~130 per run). */
  ledgerRowsPerRun: 130,
  /** Brain layer 1 reads one sample transcript per lane; the 100k line count is the viewer's. */
  brainSampleLines: 2_000,
} as const;

/** Builds once per test file: the generators are deterministic, so sharing them is sound. */
function lazy<T>(make: () => T): () => T {
  let v: T | undefined;
  return () => (v ??= make());
}

// --- simulator -------------------------------------------------------------------------------

/** A/1's grid: 40 tournaments x 120 runs (4,800 runs). */
export const simGrid = lazy((): World => world(SCALE.tournaments, SCALE.runsPerTournament));
/** 48k runs: 40 tournaments x 1,200. */
export const sim10x = lazy((): World => world(SCALE.tournaments, SCALE.runsPerTournament10x));
export const simScript = (w: World): Script => simDaemon(w);

// --- brain -----------------------------------------------------------------------------------

/** The newest of 40 tournaments holds 1,200 runs (the season's 48k runs; the brain reads the newest). */
export const brain10x = lazy(
  (): Season => season({ tournaments: SCALE.tournaments, runs: SCALE.runsPerTournament10x, rowsPerRun: SCALE.ledgerRowsPerRun, transcriptLines: SCALE.brainSampleLines }),
);
/** 48k runs in one tournament, for the run list (layer 2). */
export const brainRuns10x = lazy((): Season => season({ tournaments: 1, runs: SCALE.runs, rowsPerRun: 0, transcriptLines: 10 }));
export const BRAIN_BIG_RUN = "gen-t0000/r1";
export const brainBigTranscript = lazy((): StreamEvent[] => brainTranscript(SCALE.transcriptLines, "r1"));

/** The brain's daemon; `next()` swaps the season it serves (an input change). */
export function brainScript(first: Season, transcripts?: Map<string, StreamEvent[]>): Script & { serve: (s: Season) => void } {
  let current = brainDaemon(first, transcripts);
  const script: Script = (method, sent) => {
    if (method === "reconcile.list") return ok([]);
    return current(method, sent);
  };
  return Object.assign(script, { serve: (s: Season) => (current = brainDaemon(s, transcripts)) });
}

// --- on air ----------------------------------------------------------------------------------

export const onairRuns = lazy((): RunRow[] => Array.from({ length: SCALE.runs }, (_, i) => onairRunRow(i)));
export const onairTranscript10x = lazy((): StreamEvent[] => onairTranscript(SCALE.transcriptLines));

/** On Air's daemon: 48k runs paged by cursor (500 a page), the newest run's 100k-line transcript. */
export function onairScript(events: StreamEvent[] = onairTranscript10x()): Script {
  return (method, sent) => {
    switch (method) {
      case "runs.page":
        return page(onairRuns(), sent);
      case "run.transcript":
        return ok(events);
      case "segments.tables":
        return ok(ONAIR_TABLES);
      case "character.list":
        return ok(genRoster(SCALE.streams));
      case "directive.list":
      case "reconcile.list":
        return ok([]);
      default:
        return fail("method_not_found", method);
    }
  };
}

// --- characters ------------------------------------------------------------------------------

export const roster10x = lazy(() => genRoster(SCALE.streams));

export function charactersScript(): Script {
  return (method, sent) => {
    switch (method) {
      case "character.list":
        return ok(roster10x());
      case "character.get":
        return /"id":"malakor"/.test(sent) ? ok(malakorDetail()) : fail("not_found", "no such character");
      case "character.rules":
        return ok(malakorRules());
      case "tournament.list":
        return ok([{ tournament_id: "t-scale", runs: SCALE.streams, completed: SCALE.streams, instrument_hash: "h" }]);
      case "tournament.results":
        return ok(genResults(SCALE.streams, 1));
      default:
        return fail("method_not_found", method);
    }
  };
}

// --- segments --------------------------------------------------------------------------------

export const tables10x = lazy(() => genTables(SCALE.streams));

export function segmentsScript(tables: SegmentTables = tables10x()): Script {
  return (method) => {
    switch (method) {
      case "segments.tables":
        return ok(tables);
      case "ranks.table":
        return ok(genRanks());
      case "reconcile.list":
        return ok([]);
      default:
        return fail("method_not_found", method);
    }
  };
}

// --- reconcile -------------------------------------------------------------------------------

export const REPORT_10X = "rec-10x";
export const report10x = lazy(() => genReport({ id: REPORT_10X, held: SCALE.streams, failed: SCALE.streams, incidents: SCALE.incidents }));

export function reconcileScript(): Script {
  const list = [{ report_id: REPORT_10X, created_ms: 1792284840627, source: { tournament_id: "pilot-d1ef98d3a6" }, proposals: 60 }];
  return (method, sent) => {
    switch (method) {
      case "reconcile.list":
        return ok(list);
      case "reconcile.report":
        return param(sent, "report_id") === REPORT_10X ? ok(report10x()) : fail("not_found", "no such report");
      case "plan.proposals":
        return ok(genProposals(60));
      case "directive.list":
        return ok(genDirectives(SCALE.streams));
      case "tournament.list":
        return ok([{ tournament_id: "pilot-d1ef98d3a6", instrument_hash: "abc", runs: 4, completed: 4 }]);
      default:
        return fail("method_not_found", method);
    }
  };
}
