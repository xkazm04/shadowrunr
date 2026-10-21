// brain/transcripts: every run the daemon lists, paged in by cursor and virtualised; a row opens
// the run's transcript. Links carry the qualified `<tournament>/<run>` id so a run id that
// repeats across tournaments is never ambiguous.
import { useMemo, useState } from "react";
import type { RunRow } from "@shadowrunr/contract";
import { usePaged } from "../../data";
import { pageRoute, RouteLink, useRouter } from "../../router";
import { Button } from "../../ui/Button";
import { State } from "../../ui/Chip";
import { FilterTable, type Column, type Facets } from "../../ui/FilterTable";
import { Section } from "../../ui/Section";
import { OfflineState, StatePanel } from "../../ui/States";
import { int } from "../../kit/events";
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
  const runs = usePaged("runs.page", { limit: PAGE }, pages);
  const items = runs.query.data;
  const facets = useMemo<Facets<RunRow, string>>(() => {
    const tiers = new Set((items ?? []).map((r) => r.spec.core_tier));
    return {
      label: "Filter by core tier",
      initial: "all",
      options: [{ id: "all", label: "all" }, ...[...tiers].map((id) => ({ id, label: id }))],
      predicate: (r, f) => f === "all" || r.spec.core_tier === f,
    };
  }, [items]);

  if (items === undefined) {
    const v = runs.query.errorView;
    if (!v) return <StatePanel state="loading" title="Loading the runs" />;
    return v.state === "offline" ? <OfflineState onRetry={runs.query.reload} detail={v.message} /> : <StatePanel state={v.state} title={v.title} hint={v.hint} action={{ label: "Retry", onClick: runs.query.reload }} />;
  }
  if (items.length === 0) return <StatePanel state="empty" title="No runs yet" hint="Simulated streams appear here once a tournament has run." />;
  return (
    <Section
      title={`Runs · ${int(items.length)}${runs.more ? "+" : ""}`}
      right={
        runs.more ? (
          <Button variant="ink" size="compact" onClick={() => setPages((p) => p + STEP)}>
            Load {int(STEP * PAGE)} more
          </Button>
        ) : undefined
      }
    >
      <div data-role="brain-runs">
        <FilterTable
          rows={items}
          columns={COLUMNS}
          getKey={qualifiedRun}
          haystack={haystack}
          label="Runs"
          viewKey="brain.runs"
          facets={facets}
          placeholder="Filter by run, tournament, tone, tier or segment"
          onActivate={(r) => router.navigate(pageRoute("brain", "transcripts", qualifiedRun(r)))}
        />
      </div>
    </Section>
  );
}
