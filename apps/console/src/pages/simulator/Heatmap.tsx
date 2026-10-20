// simulator/heatmap[/<key>]: one tournament's cells worst first (the summary.rs rule, in
// ./cells.ts). Each cell shows its fail rate with n and its 95% bound; a cell opens its rules,
// quality rates and runs.
import { useMemo, useState } from "react";
import { QueryView } from "../../kit";
import { usePaged, useRpc } from "../../data";
import { pageRoute, RouteLink, useRouter } from "../../router";
import { Chip } from "../../ui/Chip";
import { FilterTable, type Column } from "../../ui/FilterTable";
import { RateBar } from "../../ui/RateBar";
import { Head, Section } from "../../ui/Section";
import { StatBand } from "../../ui/StatBand";
import { EmptyState } from "../../ui/States";
import { PAGE_LIMIT } from "./data";
import {
  cellFailRate,
  cellLabel,
  cellRouteKey,
  isRedLine,
  isThin,
  meanQuality,
  notProvoked,
  parseCellRouteKey,
  qualityEntries,
  rateBound,
  ruleEntries,
  sameCell,
  THIN_N,
  worstFirst,
} from "./cells";
import type { CellKey, CellResult, SimResults } from "./results";
import { int, pct } from "../../kit/events";
import { RefList, RunLink, Found } from "./common";
import styles from "./l2.module.css";

/** A cell lists this many runs inline; the rest are one click away in Runs. */
const RUNS_SHOWN = 50;

const bounded = (b: { lo: number; hi: number } | undefined) => (b ? { bound: { lo: b.lo, hi: b.hi } } : {});

const COLUMNS: readonly Column<CellResult>[] = [
    { id: "cell", header: "Cell", cell: (c) => cellLabel(c.cell), sortValue: (c) => cellLabel(c.cell) },
    { id: "runs", header: "Runs", cell: (c) => int(c.runs), sortValue: (c) => c.runs, align: "end" },
    {
      id: "judged",
      header: "Judged",
      cell: (c) => (
        <>
          {int(c.judged)} {isThin(c) && <Chip tone="caution" title={`Fewer than ${THIN_N} judged runs: a hint, not a fact`}>thin</Chip>}
        </>
      ),
      sortValue: (c) => c.judged,
      align: "end",
    },
    {
      id: "rate",
      header: "Fails / provoked",
      cell: (c) => {
        const r = cellFailRate(c);
        return <RateBar rate={r.rate} n={r.n} label={`${cellLabel(c.cell)} fail rate`} {...bounded(r.bound)} />;
      },
      sortValue: (c) => cellFailRate(c).rate ?? -1,
    },
    { id: "np", header: "Not provoked", cell: (c) => int(notProvoked(c)), sortValue: (c) => notProvoked(c), align: "end" },
    { id: "q", header: "Mean quality", cell: (c) => pct(meanQuality(c)), sortValue: (c) => meanQuality(c) ?? -1, align: "end" },
];

export function HeatmapPage({ cellKey }: { cellKey: string | undefined }) {
  const router = useRouter();
  const parsed = cellKey === undefined ? undefined : parseCellRouteKey(cellKey);
  const tournaments = usePaged("tournaments.page", { limit: PAGE_LIMIT });
  const list = tournaments.query.data ?? [];
  const [picked, setPicked] = useState<string | undefined>(undefined);
  const tid = parsed?.tournamentId ?? picked ?? list[list.length - 1]?.tournament_id;
  const res = useRpc("tournament.results", tid === undefined ? null : { tournament_id: tid });

  if (cellKey !== undefined && parsed === undefined) {
    return <EmptyState title="That heatmap cell does not exist" hint="Open the heatmap and pick a cell from the table." />;
  }
  return (
    <>
      <Head
        kicker="Simulator · layer 2"
        title="Heatmap"
        lede="Worst first. Every cell shows failures over provoked checks with its bound: the band is what the sample size lets us say, and at n = 1 that is very little."
      />
      <QueryView query={tournaments.query} isEmpty={(t) => t.length === 0} emptyTitle="No tournaments yet">
        {(all) => (
          <label className={styles.picker}>
            Tournament
            <select
              className={styles.select}
              value={tid ?? ""}
              onChange={(e) => {
                setPicked(e.target.value);
                if (cellKey !== undefined) router.navigate(pageRoute("simulator", "heatmap"));
              }}
            >
              {all.map((t) => (
                <option key={t.tournament_id} value={t.tournament_id}>
                  {t.tournament_id}
                </option>
              ))}
            </select>
          </label>
        )}
      </QueryView>
      {tid !== undefined && (
        <Found query={res} emptyTitle="No results for this tournament yet" emptyHint="results.json is written when the tournament is summarised.">
          {(r) => <Cells r={r} cols={COLUMNS} selected={parsed?.cell} onOpen={(c) => router.navigate(pageRoute("simulator", "heatmap", cellRouteKey(r.tournament_id, c.cell)))} />}
        </Found>
      )}
    </>
  );
}

function Cells(props: { r: SimResults; cols: readonly Column<CellResult>[]; selected: CellKey | undefined; onOpen: (c: CellResult) => void }) {
  const { r, selected } = props;
  const rows = useMemo(() => worstFirst(r.cells), [r.cells]);
  const sel = selected ? rows.find((c) => sameCell(c.cell, selected)) : undefined;
  return (
    <>
      <FilterTable
        rows={rows}
        columns={props.cols}
        getKey={(c) => cellRouteKey(r.tournament_id, c.cell)}
        haystack={(c) => cellLabel(c.cell)}
        label={`Cells of ${r.tournament_id}, worst first`}
        placeholder={`Filter ${int(rows.length)} cells`}
        onActivate={props.onOpen}
        {...(sel ? { selectedKey: cellRouteKey(r.tournament_id, sel.cell) } : {})}
      />
      {selected && !sel && <EmptyState title="This cell is not in the tournament's results" />}
      {sel && <CellDetail tournamentId={r.tournament_id} c={sel} />}
    </>
  );
}

function CellDetail({ tournamentId, c }: { tournamentId: string; c: CellResult }) {
  const runs = usePaged("runs.page", {
    tournament_id: tournamentId,
    limit: PAGE_LIMIT,
    filter: { tone_variant: c.cell.tone, audience_size: c.cell.audience_size },
  });
  const mine = (runs.query.data ?? []).filter((x) => x.spec.core_tier === c.cell.core_tier && x.spec.segment === c.cell.segment);
  const shown = mine.slice(0, RUNS_SHOWN);
  const failing = ruleEntries(c)
    .filter(([, t]) => t.fail > 0)
    .sort((a, b) => Number(isRedLine(b[0])) - Number(isRedLine(a[0])) || b[1].fail - a[1].fail);
  const rate = cellFailRate(c);
  return (
    <Section title={cellLabel(c.cell)} right={`${tournamentId}`}>
      <StatBand
        stats={[
          { label: "runs", value: int(c.runs) },
          { label: "judged", value: int(c.judged) },
          { label: "fails / provoked", value: `${int(rate.k)}/${int(rate.n)}`, ...(rate.k > 0 ? { tone: "fail" as const } : {}) },
          { label: "not provoked", value: int(notProvoked(c)) },
          { label: "max say gap", value: c.max_say_gap_s === undefined ? "–" : `${c.max_say_gap_s} s` },
        ]}
      />
      <p className={styles.dim}>
        Bound on the fail rate: {rate.bound ? `${pct(rate.bound.lo, 1)} to ${pct(rate.bound.hi, 1)} (${rate.bound.method.replace(/_/g, " ")})` : "none, nothing was provoked"}.
        Not provoked checks are left out of n.
      </p>
      <h3>Failing rules</h3>
      {failing.length === 0 ? (
        <EmptyState title="No rule failed in this cell" />
      ) : (
        <table className={styles.table}>
          <tbody>
            {failing.map(([rule, t]) => (
              <tr key={rule}>
                <td className={styles.rule}>{rule}</td>
                <td>
                  <RateBar rate={t.n > 0 ? t.fail / t.n : undefined} n={t.n} label={`${rule} fail rate`} {...bounded(t.fail_bound ?? rateBound(t.fail, t.n))} />
                </td>
                <td>
                  <RefList refs={t.fail_refs ?? []} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <h3>Quality rates · n and bounds</h3>
      <table className={styles.table}>
        <tbody>
          {qualityEntries(c).map(([dim, q]) => (
            <tr key={dim}>
              <td>{dim.replace(/_/g, " ")}</td>
              <td>{q.n > 0 ? <RateBar rate={q.rate} n={q.n} label={dim} {...bounded(q.bound)} /> : <span className={styles.dim}>n=0</span>}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <h3>Runs in this cell</h3>
      <QueryView query={runs.query} isEmpty={() => mine.length === 0} emptyTitle="No runs listed for this cell">
        {() => (
          <ul className={styles.list}>
            {shown.map((x) => (
              <li key={x.run_id}>
                <RunLink runId={x.run_id} /> {x.has_verdict ? "" : <Chip tone="muted">not judged</Chip>}
              </li>
            ))}
            {mine.length > shown.length && (
              <li>
                <RouteLink to={pageRoute("simulator", "runs")} className={styles.ref}>
                  and {int(mine.length - shown.length)} more in Runs
                </RouteLink>
              </li>
            )}
          </ul>
        )}
      </QueryView>
    </Section>
  );
}
