// The pre-flight board's reading of `preflight.run`: each check's title, whether an amber is the
// media-less phase's expected one, and the worst state (what the lamp shows).
import type { HealthReport, HealthState } from "@shadowrunr/contract";

/** The one preflight query's params: Brain's lamp, the board and On Air's transport share its entry. */
export const PREFLIGHT_PARAMS = {} as const;

/** The daemon's checks (crates/shadowrunr-daemon preflight.rs; PF-09 with Personas activity). */
export const CHECK_TITLES: Readonly<Record<string, string>> = {
  "PF-01": "Engine health",
  "PF-02": "Character package",
  "PF-03": "Memory store",
  "PF-04": "Segment tables",
  "PF-05": "Platform rules",
  "PF-06": "Audience calibration",
  "PF-07": "Runs directory and disk",
  "PF-08": "Media stack",
  "PF-09": "Integrations",
};

export const titleOf = (check: string): string => CHECK_TITLES[check] ?? check;

/** How a row reads: a media check's amber in the media-less phase is expected, not a fault. */
export type RowState = HealthState | "expected";

export function isExpected(r: HealthReport): boolean {
  return r.state === "amber" && /not in media-less phase/i.test(r.detail);
}

export const rowState = (r: HealthReport): RowState => (isExpected(r) ? "expected" : r.state);

const RANK: Readonly<Record<HealthState, number>> = { green: 0, amber: 1, red: 2 };

/** The worst state over the checks that count (expected ambers do not); green when none do. */
export function worstState(reports: readonly HealthReport[]): HealthState {
  let worst: HealthState = "green";
  for (const r of reports) if (!isExpected(r) && RANK[r.state] > RANK[worst]) worst = r.state;
  return worst;
}

export type Tally = Readonly<Record<RowState, number>>;

export function tally(reports: readonly HealthReport[]): Tally {
  const t = { green: 0, amber: 0, red: 0, expected: 0 };
  for (const r of reports) t[rowState(r)] += 1;
  return t;
}

/** The board's verdict line. */
export function verdictOf(reports: readonly HealthReport[]): string {
  const w = worstState(reports);
  if (w === "red") return "Red: a stream start is refused until every red check clears";
  if (w === "amber") return "Amber: a stream may start; read the amber checks first";
  return "Clear: every check that counts is green";
}
