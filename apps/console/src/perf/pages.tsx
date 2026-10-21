// The measured page set (test-only): one entry per registered page, keyed by PageId, so a page
// the shell ships cannot land without its 10x probes (or a written exemption). dom.test.tsx,
// rpc.test.tsx and timings.test.tsx iterate this table; coverage.test.ts holds it against
// PAGE_IDS and budgets/perf.json both ways. A probe's line names are `<page>.l1` and
// `<page>.l2.<list>`.
import type { ReactNode } from "react";
import type { Script } from "../kit/testing";
import { Layer1 as BrainL1 } from "../pages/brain/Layer1";
import { Layer2 as BrainL2 } from "../pages/brain/Layer2";
import { Layer1 as CharactersL1 } from "../pages/characters/Layer1";
import { Layer2 as CharactersL2 } from "../pages/characters/Layer2";
import { Layer1 as OnAirL1 } from "../pages/onair/Layer1";
import { Layer2 as OnAirL2 } from "../pages/onair/Layer2";
import { Layer1 as ReconcileL1 } from "../pages/reconcile/Layer1";
import { Layer2 as ReconcileL2 } from "../pages/reconcile/Layer2";
import { session } from "../pages/reconcile/session";
import { Layer1 as ReviewL1 } from "../pages/review/Layer1";
import { Layer2 as ReviewL2 } from "../pages/review/Layer2";
import { Layer1 as SegmentsL1 } from "../pages/segments/Layer1";
import { Layer2 as SegmentsL2 } from "../pages/segments/Layer2";
import { genTables } from "../pages/segments/testgen";
import { Layer1 as SimulatorL1 } from "../pages/simulator/Layer1";
import { Layer2 as SimulatorL2 } from "../pages/simulator/Layer2";
import { PAGE_IDS, type PageId } from "../router/route";
import {
  BRAIN_BIG_RUN,
  brain10x,
  brainBigTranscript,
  brainRuns10x,
  brainScript,
  charactersScript,
  onairScript,
  reconcileScript,
  REPORT_10X,
  REVIEW_BIG_SESSION,
  reviewScript,
  segmentsScript,
  sim10x,
  simGrid,
  simScript,
} from "./fixture";
import type { Line, RpcLine } from "./harness";

export interface Probe {
  /** What the probe holds at 10x, for the test title. */
  readonly what: string;
  readonly ui: () => ReactNode;
  /** The scripted daemon, built per mount. */
  readonly script: () => Script;
  readonly hash: string;
  /** True once the 10x content arrived (a count over an empty page proves nothing). */
  readonly ready?: () => boolean;
  /** Page-local state to seed before mounting. */
  readonly before?: () => void;
  readonly timeout?: number;
}

export interface Layer1Probe extends Probe {
  /** The daemon for the first-paint RPC count when it differs from the DOM count's. */
  readonly rpcScript?: () => Script;
}

export interface Measured {
  readonly l1: Layer1Probe;
  readonly l2?: Readonly<Record<string, Probe>>;
}
export type PerfPage = Measured | { readonly exempt: string };

const has = (sel: string) => () => document.querySelector(sel) !== null;
const text = (re: RegExp) => () => re.test(document.body.textContent ?? "");

export const PERF_PAGES = {
  onair: {
    l1: { what: "the newest of 48k runs, a 100k-line transcript", ui: () => <OnAirL1 active />, script: () => onairScript(), hash: "#/onair", ready: text(/Replay 4×/), timeout: 120_000 },
    l2: {
      rundown: { what: "a 500-run page of 48k runs", ui: () => <OnAirL2 sub={["rundown"]} />, script: () => onairScript([]), hash: "#/onair/rundown", ready: text(/500 of 500/) },
    },
  },
  characters: {
    l1: { what: "a 2,000-character roster", ui: () => <CharactersL1 active />, script: charactersScript, hash: "#/characters", ready: text(/1 of 2,000/) },
    l2: {
      all: { what: "2,000 characters", ui: () => <CharactersL2 sub={["all"]} />, script: charactersScript, hash: "#/characters/all", ready: text(/2,000 characters/) },
    },
  },
  brain: {
    l1: { what: "the newest of 40 tournaments, 1,200 runs, 156k ledger rows", ui: () => <BrainL1 active />, script: () => brainScript(brain10x()), hash: "#/brain", ready: text(/first pages only/), timeout: 60_000 },
    l2: {
      runs: { what: "48k runs paged in by cursor", ui: () => <BrainL2 sub={["transcripts"]} />, script: () => brainScript(brainRuns10x()), hash: "#/brain/transcripts", ready: text(/Runs · 2,000\+/), timeout: 60_000 },
      transcript: {
        what: "a 100k-line transcript",
        ui: () => <BrainL2 sub={["transcripts", BRAIN_BIG_RUN, "50000"]} />,
        script: () => brainScript(brainRuns10x(), new Map([[BRAIN_BIG_RUN, brainBigTranscript()]])),
        hash: "#/brain/transcripts/gen-t0000%2Fr1/50000",
        ready: text(/100,000 of 100,000 lines/),
        timeout: 120_000,
      },
    },
  },
  segments: {
    l1: { what: "2,000 segments in the tables", ui: () => <SegmentsL1 active />, script: () => segmentsScript(), hash: "#/segments", ready: has("[data-role='segments-tick-more']") },
    l2: {
      tables: {
        what: "the compiled-in tables (SegmentId is a closed enum of four, so they do not scale)",
        ui: () => <SegmentsL2 sub={["tables"]} />,
        script: () => segmentsScript(genTables()),
        hash: "#/segments/tables",
        ready: has("[data-role='segments-card']"),
      },
    },
  },
  simulator: {
    l1: {
      what: "40 tournaments x 1,200 runs (48k)",
      ui: () => <SimulatorL1 active />,
      script: () => simScript(sim10x()),
      // The RPC count runs on A/1's grid: runs.page per tournament and no tournament.results.
      rpcScript: () => simScript(simGrid()),
      hash: "#/simulator",
      ready: text(/48,000runs/),
      timeout: 60_000,
    },
    l2: {
      runs: { what: "48k runs", ui: () => <SimulatorL2 sub={["runs"]} />, script: () => simScript(sim10x()), hash: "#/simulator/runs", ready: text(/48,000 of 48,000/), timeout: 60_000 },
    },
  },
  review: {
    l1: {
      what: "2,000 sandbox sessions, 2,000 marks",
      ui: () => <ReviewL1 active />,
      script: () => reviewScript(),
      hash: "#/review",
      ready: () => document.querySelector("[data-role='review-wall-all'] b")?.textContent === "2,000",
      timeout: 60_000,
    },
    l2: {
      timeline: {
        what: "a 100k-event session's timeline",
        ui: () => <ReviewL2 sub={[REVIEW_BIG_SESSION, "timeline"]} />,
        script: () => reviewScript(),
        hash: `#/review/${REVIEW_BIG_SESSION}/timeline`,
        ready: has("[data-role='review-tl-row']"),
        timeout: 120_000,
      },
    },
  },
  reconcile: {
    l1: {
      what: "2,000 held, 2,000 failed, 4,170 incidents",
      ui: () => <ReconcileL1 active />,
      script: reconcileScript,
      hash: "#/reconcile",
      ready: () => document.querySelector("[data-role='reconcile-held'] [data-n]")?.getAttribute("data-n") === "2000",
    },
    l2: {
      report: {
        what: "4,170 incidents, 2,000 + 2,000 findings",
        ui: () => <ReconcileL2 sub={["report"]} />,
        script: reconcileScript,
        hash: "#/reconcile/report",
        before: () => session.showReport(REPORT_10X),
        ready: text(/4,170 of 4,170/),
      },
    },
  },
} satisfies Record<PageId, PerfPage>;

/** A measured layer with its line name. */
export interface Named {
  readonly page: PageId;
  readonly name: string;
  readonly probe: Probe;
}

const isMeasured = (p: PerfPage): p is Measured => !("exempt" in p);

/** Every layer 1, named `<page>.l1` (the DOM and RPC gates). */
export const LAYER1: readonly (Named & { readonly probe: Layer1Probe })[] = PAGE_IDS.flatMap((page) => {
  const p: PerfPage = PERF_PAGES[page];
  return isMeasured(p) ? [{ page, name: `${page}.l1`, probe: p.l1 }] : [];
});

/** Every main layer-2 list, named `<page>.l2.<list>` (the DOM gate). */
export const LAYER2: readonly Named[] = PAGE_IDS.flatMap((page) => {
  const p: PerfPage = PERF_PAGES[page];
  return isMeasured(p) ? Object.entries(p.l2 ?? {}).map(([list, probe]) => ({ page, name: `${page}.l2.${list}`, probe })) : [];
});

/**
 * The two-way coverage of `pages` against the registered page ids and the perf budgets: every
 * page has an entry, every entry is a page, every measured name has its dom line (and every
 * layer 1 its rpc line), every dom and rpc line is measured, and every exemption says why.
 */
export function perfCoverage(
  pages: Readonly<Record<string, PerfPage>>,
  budgets: { readonly dom: Readonly<Record<string, Line>>; readonly rpc: Readonly<Record<string, RpcLine>> },
): string[] {
  const problems: string[] = [];
  const dom = new Set<string>();
  const rpc = new Set<string>();
  for (const id of PAGE_IDS) if (!(id in pages)) problems.push(`${id}: a registered page with no PERF_PAGES entry`);
  for (const [id, p] of Object.entries(pages)) {
    if (!(PAGE_IDS as readonly string[]).includes(id)) problems.push(`${id}: not a registered page`);
    if (!isMeasured(p)) {
      if (p.exempt.trim() === "") problems.push(`${id}: an exemption must say why`);
      continue;
    }
    dom.add(`${id}.l1`);
    rpc.add(`${id}.l1`);
    for (const list of Object.keys(p.l2 ?? {})) dom.add(`${id}.l2.${list}`);
  }
  for (const name of dom) if (!(name in budgets.dom)) problems.push(`${name}: measured but no line in budgets/perf.json`);
  for (const name of rpc) if (!(name in budgets.rpc)) problems.push(`${name}: measured but no rpc line in budgets/perf.json`);
  for (const name of Object.keys(budgets.dom)) if (!dom.has(name)) problems.push(`${name}: budgeted but no page measures it`);
  for (const name of Object.keys(budgets.rpc)) if (!rpc.has(name)) problems.push(`${name}: rpc budgeted but no page measures it`);
  return problems;
}
