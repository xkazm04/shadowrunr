// Brain's URL grammar, declared once. Brain's layer 2 reads it, the shell derives the
// breadcrumbs, the rise key and the unknown-sub fallback from it, and other pages link in through
// its builders. It lives in the router, not pages/brain, because a page never imports another
// page: the deep links into Brain are part of the URL contract every page shares.
//   brain/engine
//   brain/transcripts[/<run>[/<line>]]    <run> is `<tournament>/<run_id>` whenever it is known
//   brain/preflight
import { pageRoute, type Route } from "./route";
import type { RouteSchema } from "./schema";

export const FLOORS = [
  { id: "engine", label: "The engine" },
  { id: "transcripts", label: "Transcripts" },
  { id: "preflight", label: "Pre-flight" },
] as const;

export type Floor = (typeof FLOORS)[number]["id"];

const label = (id: Floor): string => FLOORS.find((f) => f.id === id)?.label ?? id;

/** A 1-based transcript line. */
const isLine = (s: string): boolean => /^[1-9]\d*$/.test(s);

export const BRAIN_ROUTES: RouteSchema = [
  { seg: "engine", label: label("engine") },
  {
    seg: "transcripts",
    label: label("transcripts"),
    children: [
      {
        param: "run",
        // the run id verbatim, qualified or not
        label: (run) => run,
        // the line is a scroll target in the transcript: the viewer keeps its state
        children: [{ param: "line", test: isLine, label: (line) => `Line ${line}`, inPlace: true }],
      },
    ],
  },
  { seg: "preflight", label: label("preflight") },
];

export const floorRoute = (floor: Floor): Route => pageRoute("brain", floor);

/** The run a transcript link names: the tournament when it is known, the run alone otherwise. */
export interface TranscriptRun {
  readonly tournamentId?: string | undefined;
  /** A run id, or an already qualified `<tournament>/<run_id>` ref. */
  readonly runId: string;
}

/**
 * The transcript of a run, optionally at a line. Run ids repeat across tournaments (one per
 * matrix cell), and the daemon refuses a bare id it finds in more than one, so a caller that
 * knows the tournament always qualifies.
 */
export function transcriptRoute(run: TranscriptRun, line?: number): Route {
  const ref = run.tournamentId !== undefined && !run.runId.includes("/") ? `${run.tournamentId}/${run.runId}` : run.runId;
  return line === undefined ? pageRoute("brain", "transcripts", ref) : pageRoute("brain", "transcripts", ref, String(line));
}
