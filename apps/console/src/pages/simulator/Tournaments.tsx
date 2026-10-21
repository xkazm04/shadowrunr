// simulator/tournaments[/<id>]: every tournament (virtualised), then one tournament with its
// figures, assumptions and its runs worst first.
import { useMemo } from "react";
import type { RunRow, TournamentRow } from "@shadowrunr/contract";
import { QueryView } from "../../kit";
import { useRpc } from "../../data";
import { pageRoute, useRouter } from "../../router";
import { Chip } from "../../ui/Chip";
import { FilterTable, type Column } from "../../ui/FilterTable";
import { Head, Section } from "../../ui/Section";
import { StatBand } from "../../ui/StatBand";
import { usePaged } from "../../data";
import { PAGE_LIMIT, useResults } from "./data";
import { buildTile, resultsHealth, tilesInView, type RunCell } from "./grid";
import type { SimResults } from "./results";
import { int, usd } from "../../kit/events";
import { Health, Note, shortHash, Found } from "./common";
import styles from "./l2.module.css";

export function TournamentList() {
  const router = useRouter();
  const paged = usePaged("tournaments.page", { limit: PAGE_LIMIT });
  // Health is read for the tournaments layer 1 shows (already cached), not all of them.
  const recent = useMemo(() => tilesInView(paged.query.data ?? []).map((t) => t.tournament_id), [paged.query.data]);
  const results = useResults(recent);
  const columns: readonly Column<TournamentRow>[] = useMemo(
    () => [
      { id: "id", header: "Tournament", cell: (t) => <b>{t.tournament_id}</b>, sortValue: (t) => t.tournament_id },
      { id: "hash", header: "Instrument", cell: (t) => <code className={styles.code}>{shortHash(t.instrument_hash)}</code> },
      { id: "done", header: "Runs done", cell: (t) => `${int(t.completed)} / ${int(t.runs)}`, sortValue: (t) => t.runs, align: "end" },
      {
        id: "held",
        header: "Held",
        cell: (t) => {
          const r = results.get(t.tournament_id);
          return r === undefined ? <span className={styles.dim}>open to read</span> : <Health value={resultsHealth(r)} />;
        },
      },
    ],
    [results],
  );
  return (
    <>
      <Head kicker="Simulator · layer 2" title="Tournaments" lede="A tournament crosses tone, model tier, segment and audience size, runs each cell against simulated chat, and has a different-tier judge read the transcript blind." />
      <QueryView query={paged.query} isEmpty={(rows) => rows.length === 0} emptyTitle="No tournaments yet" emptyHint="Run one with `shadowrunr sim tournament`.">
        {(rows) => (
          <FilterTable
            rows={rows}
            columns={columns}
            getKey={(t) => t.tournament_id}
            haystack={(t) => `${t.tournament_id} ${t.instrument_hash}`}
            label="Tournaments"
            viewKey="simulator.tournaments"
            placeholder={`Filter ${int(rows.length)} tournaments${paged.done ? "" : " (loading more)"}`}
            onActivate={(t) => router.navigate(pageRoute("simulator", "tournaments", t.tournament_id))}
          />
        )}
      </QueryView>
    </>
  );
}

interface RunLine {
  readonly row: RunRow;
  readonly cell: RunCell | undefined;
}

const runColumns: readonly Column<RunLine>[] = [
  { id: "run", header: "Run", cell: (r) => <span className={styles.code}>{r.row.run_id}</span>, sortValue: (r) => r.row.run_id },
  { id: "tone", header: "Tone", cell: (r) => r.row.spec.tone_variant, sortValue: (r) => r.row.spec.tone_variant },
  { id: "tier", header: "Tier", cell: (r) => r.row.spec.core_tier, sortValue: (r) => r.row.spec.core_tier },
  { id: "seg", header: "Segment", cell: (r) => r.row.spec.segment, sortValue: (r) => r.row.spec.segment },
  { id: "aud", header: "Viewers", cell: (r) => int(r.row.spec.audience_size), sortValue: (r) => r.row.spec.audience_size, align: "end" },
  { id: "seed", header: "Seed", cell: (r) => r.row.spec.seed, sortValue: (r) => r.row.spec.seed, align: "end" },
  { id: "held", header: "Held (cell)", cell: (r) => <Health value={r.cell?.health} />, sortValue: (r) => r.cell?.health ?? 2 },
  { id: "red", header: "Red line", cell: (r) => (r.cell?.red ? <Chip tone="fail">fail</Chip> : null), sortValue: (r) => (r.cell?.red ? 0 : 1) },
];

export function TournamentDetail({ id }: { id: string }) {
  const router = useRouter();
  const res = useRpc("tournament.results", { tournament_id: id });
  const runs = usePaged("runs.page", { tournament_id: id, limit: PAGE_LIMIT });
  const lines = useMemo(() => {
    const rows = runs.query.data ?? [];
    const tile = buildTile(id, rows.length, rows);
    const lines: RunLine[] = rows.map((row, i) => ({ row, cell: tile.runs[i] }));
    return lines.sort((a, b) => (a.cell?.health ?? 2) - (b.cell?.health ?? 2));
  }, [id, runs.query.data]);
  return (
    <>
      <Head kicker="Simulator · tournament" title={id} />
      <Found query={res} emptyTitle="No results for this tournament yet" emptyHint="results.json is written when the tournament is summarised.">
        {(r) => <ResultsSummary r={r} />}
      </Found>
      <Section title="Runs · worst first" right={`${int(lines.length)} runs${runs.done ? "" : " · loading"}`}>
        <QueryView query={runs.query} isEmpty={(r) => r.length === 0} emptyTitle="No runs recorded for this tournament">
          {() => (
            <FilterTable
              rows={lines}
              columns={runColumns}
              getKey={(r) => r.row.run_id}
              haystack={(r) => `${r.row.run_id} ${r.row.spec.tone_variant} ${r.row.spec.core_tier} ${r.row.spec.segment} ${r.row.spec.audience_size}`}
              label={`Runs of ${id}`}
              viewKey={`simulator.tournaments.${id}.runs`}
              onActivate={(r) => router.navigate(pageRoute("simulator", "verdicts", r.row.run_id))}
            />
          )}
        </QueryView>
      </Section>
    </>
  );
}

function ResultsSummary({ r }: { r: SimResults }) {
  return (
    <>
      <StatBand
        stats={[
          { label: "runs", value: int(r.runs) },
          { label: "judged", value: int(r.judged) },
          { label: "void", value: int(r.void) },
          { label: "failed", value: int(r.failed), ...(r.failed > 0 ? { tone: "fail" as const } : {}) },
          { label: r.usage_total ? `calls · ${usd(r.usage_total.cost_usd)}` : "calls", value: r.usage_total ? int(r.usage_total.calls) : "–" },
        ]}
      />
      <dl className={styles.kv}>
        <dt>Held</dt>
        <dd>
          <Health value={resultsHealth(r)} />
        </dd>
        <dt>Instrument</dt>
        <dd>
          {r.instrument_hashes.map((h) => (
            <code key={h} className={styles.code}>
              {shortHash(h)}{" "}
            </code>
          ))}
          <span className={styles.dim}> results from different hashes are never merged</span>
        </dd>
      </dl>
      {r.instrument_hashes.length > 1 && <Note>This tournament mixes instrument hashes; compare cells within one hash only.</Note>}
      {r.assumptions !== undefined && r.assumptions.length > 0 && (
        <Section title="Assumptions the calibration carries">
          <ul className={styles.list}>
            {r.assumptions.map((a) => (
              <li key={a}>
                <code className={styles.code}>{a}</code>
              </li>
            ))}
          </ul>
        </Section>
      )}
    </>
  );
}
