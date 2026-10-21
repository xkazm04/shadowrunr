// simulator/runs: every run across tournaments, virtualised, with a text filter, a verdict
// facet and sortable columns. Rows open the run's verdict.
import type { RunRow } from "@shadowrunr/contract";
import { QueryView } from "../../kit";
import { pageRoute, useRouter } from "../../router";
import { Chip } from "../../ui/Chip";
import { FilterTable, type Column, type Facets } from "../../ui/FilterTable";
import { Head } from "../../ui/Section";
import { usePaged } from "../../data";
import { PAGE_LIMIT } from "./data";
import { int } from "../../kit/events";
import styles from "./l2.module.css";

type VerdictFacet = "all" | "judged" | "unjudged";

const columns: readonly Column<RunRow>[] = [
  { id: "run", header: "Run", cell: (r) => <span className={styles.code}>{r.run_id}</span>, sortValue: (r) => r.run_id },
  { id: "t", header: "Tournament", cell: (r) => r.tournament_id, sortValue: (r) => r.tournament_id },
  { id: "tone", header: "Tone", cell: (r) => r.spec.tone_variant, sortValue: (r) => r.spec.tone_variant },
  { id: "tier", header: "Tier", cell: (r) => r.spec.core_tier, sortValue: (r) => r.spec.core_tier },
  { id: "seg", header: "Segment", cell: (r) => r.spec.segment, sortValue: (r) => r.spec.segment },
  { id: "aud", header: "Viewers", cell: (r) => int(r.spec.audience_size), sortValue: (r) => r.spec.audience_size, align: "end" },
  { id: "seed", header: "Seed", cell: (r) => r.spec.seed, sortValue: (r) => r.spec.seed, align: "end" },
  {
    id: "verdict",
    header: "Verdict",
    cell: (r) => (r.has_verdict ? <Chip tone="ok">judged</Chip> : <Chip tone="muted">not judged</Chip>),
    sortValue: (r) => (r.has_verdict ? 0 : 1),
  },
];

const haystack = (r: RunRow) => `${r.run_id} ${r.tournament_id} ${r.spec.tone_variant} ${r.spec.core_tier} ${r.spec.segment} ${r.spec.audience_size}`;

const FACETS: Facets<RunRow, VerdictFacet> = {
  label: "Verdict",
  initial: "all",
  options: [
    { id: "all", label: "All" },
    { id: "judged", label: "Judged" },
    { id: "unjudged", label: "Not judged" },
  ],
  predicate: (r, f) => f === "all" || (f === "judged") === r.has_verdict,
};

export function RunsPage() {
  const router = useRouter();
  const paged = usePaged("runs.page", { limit: PAGE_LIMIT });
  return (
    <>
      <Head kicker="Simulator · layer 2" title="Runs" lede="Each run is one simulated stream with a spec, a transcript, a judge verdict and a ledger." />
      <QueryView query={paged.query} isEmpty={(r) => r.length === 0} emptyTitle="No runs yet" emptyHint="Runs appear here as a tournament simulates them.">
        {(all) => (
          <FilterTable
            rows={all}
            columns={columns}
            getKey={(r) => `${r.tournament_id}/${r.run_id}`}
            haystack={haystack}
            label="Runs"
            viewKey="simulator.runs"
            placeholder={`Filter ${int(all.length)} runs${paged.done ? "" : " (loading more)"}`}
            facets={FACETS}
            onActivate={(r) => router.navigate(pageRoute("simulator", "verdicts", r.run_id))}
          />
        )}
      </QueryView>
    </>
  );
}
