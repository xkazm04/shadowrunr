import { useEffect, useMemo, useState } from "react";
import type { CellResult, RunRow } from "../contract";
import { useRpc } from "../api/context";
import { boundStr, pct, shortHash } from "../lib/format";
import { cellId, cellLabel } from "../lib/heatmap";
import { useDesk } from "../legacy/state";
import { Button, Chip, Empty, ErrorState, Loading, Popover, SectionHeader, Toolbar, cx, useListNav } from "../legacy/ui/primitives";
import { TranscriptInspector } from "../legacy/ui/TranscriptViewer";
import { Heatmap, type HeatSelection } from "./Heatmap";

function runsOfCell(runs: RunRow[], cell: CellResult): RunRow[] {
  const k = cell.cell;
  return runs.filter(
    (r) =>
      r.spec.tone_variant === k.tone &&
      r.spec.core_tier === k.core_tier &&
      r.spec.segment === k.segment &&
      r.spec.audience_size === k.audience_size,
  );
}

export function SimulationsView() {
  const { go, setReconcileTournament } = useDesk();
  const tournaments = useRpc("tournament.list", {});
  const [tid, setTid] = useState<string | null>(null);
  const [sel, setSel] = useState<{ cell: CellResult; column: string } | null>(null);
  const [runId, setRunId] = useState<string | null>(null);

  const list = tournaments.data ?? [];
  useEffect(() => {
    if (!tid && list[0]) setTid(list[list.length - 1]!.tournament_id);
  }, [list, tid]);

  const results = useRpc("tournament.results", tid ? { tournament_id: tid } : null, tid ?? "");
  const runs = useRpc("runs.list", tid ? { tournament_id: tid } : null, tid ?? "");

  useEffect(() => {
    setSel(null);
    setRunId(null);
  }, [tid]);

  const tNav = useListNav(list.length, {
    onMove: (i) => setTid(list[i]?.tournament_id ?? null),
  });

  const cellRuns = useMemo(() => (sel && runs.data ? runsOfCell(runs.data, sel.cell) : []), [sel, runs.data]);
  const rNav = useListNav(cellRuns.length, {
    onMove: (i) => setRunId(cellRuns[i]?.run_id ?? null),
    onActivate: (i) => setRunId(cellRuns[i]?.run_id ?? null),
  });

  const selection: HeatSelection | null = sel ? { cellId: cellId(sel.cell.cell), column: sel.column } : null;
  const res = results.data;
  const multiHashWarning = res && res.instrument_hashes.length > 1;
  const selQ = sel && sel.column !== "rules" ? sel.cell.quality[sel.column] : undefined;

  if (tournaments.error) {
    return (
      <div className="view">
        <div className="main">
          <Toolbar title="Simulations" />
          <div className="content">
            <ErrorState error={tournaments.error} onRetry={tournaments.reload} />
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="view">
      <div className="main">
        <Toolbar
          title="Simulations"
          subtitle={tid ? <span className="mono">{tid}</span> : undefined}
        >
          {tid && (
            <Button
              onClick={() => {
                setReconcileTournament(tid);
                go("reconcile");
              }}
              title="Open Reconcile & Plan with this tournament as the source"
            >
              Reconcile…
            </Button>
          )}
          <Button
            variant="ghost"
            onClick={() => {
              tournaments.reload();
              results.reload();
              runs.reload();
            }}
          >
            Reload
          </Button>
        </Toolbar>
        <div className="content stack">
          <SectionHeader right={<span className="muted">{list.length} tournaments</span>}>Tournaments</SectionHeader>
          {tournaments.loading && !list.length ? (
            <Loading />
          ) : !list.length ? (
            <Empty title="No tournaments yet">
              Run <code>shadowrunr sim tournament --matrix sim/matrices/pilot.toml</code>
            </Empty>
          ) : (
            <div className="table table-short" {...tNav.containerProps} aria-label="Tournaments">
              <div className="tr th">
                <span className="c-grow">Tournament</span>
                <span className="c-progress">Runs</span>
                <span className="c-hash">Instrument</span>
              </div>
              {list.map((t, i) => (
                <div
                  key={t.tournament_id}
                  {...tNav.rowProps(i)}
                  className={cx("tr", i === tNav.active && "active", t.tournament_id === tid && "selected")}
                  onClick={() => setTid(t.tournament_id)}
                >
                  <span className="c-grow mono">{t.tournament_id}</span>
                  <span className="c-progress">
                    <span className="bar">
                      <span style={{ width: `${t.runs ? (100 * t.completed) / t.runs : 0}%` }} />
                    </span>
                    <span className="mono small">
                      {t.completed}/{t.runs}
                    </span>
                  </span>
                  <span className="c-hash mono muted" title={t.instrument_hash}>
                    {shortHash(t.instrument_hash)}
                  </span>
                </div>
              ))}
            </div>
          )}

          {tid && (
            <>
              <SectionHeader
                right={
                  res && (
                    <span className="hm-summary">
                      <span className="muted">
                        {res.judged} judged · {res.void} void · {res.failed} failed of {res.runs}
                      </span>
                      {multiHashWarning && <Chip tone="warn">{res.instrument_hashes.length} instruments: never merged</Chip>}
                      {!!res.assumptions?.length && (
                        <Popover
                          trigger={(open) => (
                            <Chip tone="warn" onClick={open} title="Calibration figures without a source">
                              {res.assumptions!.length} assumptions
                            </Chip>
                          )}
                        >
                          {() => (
                            <ul className="pop-list">
                              {res.assumptions!.map((a) => (
                                <li key={a}>{a}</li>
                              ))}
                            </ul>
                          )}
                        </Popover>
                      )}
                    </span>
                  )
                }
              >
                Results
              </SectionHeader>
              {results.error ? (
                <ErrorState error={results.error} onRetry={results.reload} />
              ) : !res ? (
                <Loading label="Loading results" />
              ) : !res.cells.length ? (
                <Empty title="No judged cells yet" />
              ) : (
                <Heatmap results={res} selection={selection} onSelect={(cell, column) => setSel({ cell, column })} />
              )}
            </>
          )}

          {sel && (
            <>
              <SectionHeader right={<span className="muted">{cellRuns.length} runs</span>}>
                <span>{cellLabel(sel.cell.cell)}</span>
                <span className="muted">
                  {" "}
                  ·{" "}
                  {sel.column === "rules"
                    ? "rules"
                    : `${sel.column.replace(/_/g, " ")} ${pct(selQ?.rate, 1)} (n ${selQ?.n ?? 0}) · 95% ${boundStr(selQ?.bound)}`}
                </span>
              </SectionHeader>
              {runs.error ? (
                <ErrorState error={runs.error} onRetry={runs.reload} />
              ) : (
                <div className="table table-short" {...rNav.containerProps} aria-label="Runs in cell">
                  {cellRuns.map((r, i) => (
                    <div
                      key={r.run_id}
                      {...rNav.rowProps(i)}
                      className={cx("tr", i === rNav.active && "active", r.run_id === runId && "selected")}
                      onClick={() => setRunId(r.run_id)}
                    >
                      <span className="c-grow mono ellipsis">{r.run_id}</span>
                      <span className="c-name muted">seed {r.spec.seed}</span>
                      <span className="c-name muted">{r.spec.population}</span>
                      <span className="c-name">{r.has_verdict ? <Chip tone="ok">judged</Chip> : <Chip>no verdict</Chip>}</span>
                    </div>
                  ))}
                  {!cellRuns.length && <div className="tr muted">No runs listed for this cell</div>}
                </div>
              )}
            </>
          )}
        </div>
      </div>
      {runId && <TranscriptInspector runId={runId} onClose={() => setRunId(null)} />}
    </div>
  );
}
