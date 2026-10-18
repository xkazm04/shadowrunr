// The rundown: one run's anatomy when one is chosen, every run (paged from the daemon, 500 at a
// time, virtualised), and the segment cards from the segment tables.
import type { RunRow, SegmentId } from "@shadowrunr/contract";
import { useState } from "react";
import { useRpc } from "../../data";
import { QueryView } from "../../kit";
import { pageRoute, useRouter } from "../../router";
import { Chip } from "../../ui/Chip";
import { FilterTable, type Column } from "../../ui/FilterTable";
import { Section } from "../../ui/Section";
import styles from "./Layer2.module.css";
import { RunDetail } from "./RunDetail";
import { span as clock } from "../../kit/events";
import { runLabel } from "./beats";
import { SegmentCards } from "./SegmentCards";

/** Runs per page: the daemon caps `runs.page` at 500. */
export const RUN_PAGE = 500;

type SegFacet = "all" | SegmentId;

const COLUMNS: readonly Column<RunRow>[] = [
  { id: "run", header: "Run", cell: (r) => runLabel(r), sortValue: (r) => runLabel(r) },
  { id: "seg", header: "Segment", cell: (r) => r.spec.segment, sortValue: (r) => r.spec.segment },
  { id: "dur", header: "Length", cell: (r) => clock(r.spec.duration_s * 1000), sortValue: (r) => r.spec.duration_s, align: "end" },
  { id: "tour", header: "Tournament", cell: (r) => r.tournament_id, sortValue: (r) => r.tournament_id },
  { id: "verdict", header: "Verdict", cell: (r) => (r.has_verdict ? <Chip tone="ok">judged</Chip> : <Chip tone="muted">not judged</Chip>), sortValue: (r) => (r.has_verdict ? 1 : 0) },
];

const FACETS = {
  label: "Segment",
  options: [
    { id: "all", label: "All" },
    { id: "SEG-01", label: "SEG-01" },
    { id: "SEG-02A", label: "SEG-02A" },
  ],
  initial: "all",
  predicate: (r: RunRow, f: SegFacet) => f === "all" || r.spec.segment === f,
} as const;

const keyOf = (r: RunRow) => `${r.tournament_id}/${r.run_id}`;
const haystack = (r: RunRow) => `${r.run_id} ${r.tournament_id} ${r.spec.tone_variant} ${r.spec.core_tier} ${r.spec.segment} ${r.spec.population}`;

function Runs(props: { selected: string | undefined }) {
  const router = useRouter();
  // A stack of cursors: the last one is the page on screen.
  const [cursors, setCursors] = useState<readonly string[]>([]);
  const cursor = cursors[cursors.length - 1];
  const page = useRpc("runs.page", cursor === undefined ? { limit: RUN_PAGE } : { limit: RUN_PAGE, cursor });
  const next = page.data?.next_cursor;
  const pager = (
    <span className={styles.pager}>
      <button type="button" disabled={cursors.length === 0} onClick={() => setCursors((c) => c.slice(0, -1))}>
        Previous {RUN_PAGE}
      </button>
      <span>Page {cursors.length + 1}</span>
      <button type="button" disabled={next === undefined} onClick={() => next !== undefined && setCursors((c) => [...c, next])}>
        Next {RUN_PAGE}
      </button>
    </span>
  );
  return (
    <Section title="The night, as it ran" right={pager}>
      <QueryView query={page} isEmpty={(d) => d.items.length === 0} emptyTitle="No runs yet" emptyHint="Start a simulation from the Simulator: every run it records shows here.">
        {(d) => (
          <div className={styles.runs} data-role="onair-runs">
            <FilterTable<RunRow, SegFacet>
              rows={d.items}
              columns={COLUMNS}
              getKey={keyOf}
              haystack={haystack}
              label="Runs"
              placeholder="Filter runs: tone, tier, segment, run id"
              facets={FACETS}
              onActivate={(r) => router.navigate(pageRoute("onair", "rundown", r.tournament_id, r.run_id))}
              {...(props.selected === undefined ? {} : { selectedKey: props.selected })}
            />
          </div>
        )}
      </QueryView>
      <p className={styles.note}>Each run is a few minutes of simulated time. Choose one to see its segments and beats, then replay it on the screen.</p>
    </Section>
  );
}

export function Rundown({ run }: { run: { tournamentId: string; runId: string } | null }) {
  return (
    <>
      {run && <RunDetail tournamentId={run.tournamentId} runId={run.runId} />}
      <Runs selected={run ? `${run.tournamentId}/${run.runId}` : undefined} />
      <Section title="The segments">
        <SegmentCards />
      </Section>
    </>
  );
}
