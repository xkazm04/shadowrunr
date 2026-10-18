// brain/transcripts: every run the daemon lists, paged in by cursor and virtualised; a row opens
// the run's transcript. Links carry the qualified `<tournament>/<run>` id so a run id that
// repeats across tournaments is never ambiguous.
import { useMemo, useState } from "react";
import type { RunRow } from "@shadowrunr/contract";
import { pageRoute, RouteLink, useRouter } from "../../router";
import { FilterTable, OfflineState, Section, StatePanel, State, type Column, type Facets } from "../../ui";
import { usePagedRpc } from "./queries";
import { int } from "./events";
import styles from "./Layer2.module.css";

const PAGE = 500;
const STEP = 4;

export const qualifiedRun = (r: RunRow): string => `${r.tournament_id}/${r.run_id}`;

const COLUMNS: readonly Column<RunRow>[] = [
  {
    id: "run",
    header: "Run",
    cell: (r) => (
      <RouteLink to={pageRoute("brain", "transcripts", qualifiedRun(r))} className={styles.link}>
        {r.run_id}
      </RouteLink>
    ),
    sortValue: (r) => r.run_id,
  },
  { id: "tournament", header: "Tournament", cell: (r) => r.tournament_id, sortValue: (r) => r.tournament_id },
  { id: "tone", header: "Tone", cell: (r) => r.spec.tone_variant, sortValue: (r) => r.spec.tone_variant },
  { id: "tier", header: "Core tier", cell: (r) => r.spec.core_tier, sortValue: (r) => r.spec.core_tier },
  { id: "segment", header: "Segment", cell: (r) => r.spec.segment, sortValue: (r) => r.spec.segment },
  { id: "aud", header: "Audience", align: "end", cell: (r) => `${r.spec.audience_size} sim.`, sortValue: (r) => r.spec.audience_size },
  { id: "verdict", header: "Verdict", cell: (r) => (r.has_verdict ? <State value="pass" label="judged" /> : <State value="not_provoked" label="not judged" />) },
];

const haystack = (r: RunRow) => `${r.run_id} ${r.tournament_id} ${r.spec.tone_variant} ${r.spec.core_tier} ${r.spec.segment} a${r.spec.audience_size}`;

export function Runs() {
  const router = useRouter();
  const [pages, setPages] = useState(STEP);
  const runs = usePagedRpc("runs.page", { limit: PAGE }, pages);
  const facets = useMemo<Facets<RunRow, string>>(() => {
    const tiers = new Map<string, number>();
    for (const r of runs.data.items) tiers.set(r.spec.core_tier, (tiers.get(r.spec.core_tier) ?? 0) + 1);
    return {
      label: "Filter by core tier",
      initial: "all",
      options: [{ id: "all", label: "all", count: runs.data.items.length }, ...[...tiers].map(([id, count]) => ({ id, label: id, count }))],
      predicate: (r, f) => f === "all" || r.spec.core_tier === f,
    };
  }, [runs.data.items]);

  if (runs.data.pages === 0) {
    const v = runs.errorView;
    if (!v) return <StatePanel state="loading" title="Loading the runs" />;
    return v.state === "offline" ? <OfflineState onRetry={runs.reload} detail={v.message} /> : <StatePanel state={v.state} title={v.title} hint={v.hint} action={{ label: "Retry", onClick: runs.reload }} />;
  }
  if (runs.data.items.length === 0) return <StatePanel state="empty" title="No runs yet" hint="Simulated streams appear here once a tournament has run." />;
  return (
    <Section
      title={`Runs · ${int(runs.data.items.length)}${runs.data.more ? "+" : ""}`}
      right={
        runs.data.more ? (
          <button type="button" className={styles.more} onClick={() => setPages((p) => p + STEP)}>
            Load {int(STEP * PAGE)} more
          </button>
        ) : undefined
      }
    >
      <div data-role="brain-runs">
        <FilterTable
          rows={runs.data.items}
          columns={COLUMNS}
          getKey={qualifiedRun}
          haystack={haystack}
          label="Runs"
          facets={facets}
          placeholder="Filter by run, tournament, tone, tier or segment"
          onActivate={(r) => router.navigate(pageRoute("brain", "transcripts", qualifiedRun(r)))}
        />
      </div>
    </Section>
  );
}
