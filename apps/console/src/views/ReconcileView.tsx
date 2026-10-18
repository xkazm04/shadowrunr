import { useEffect, useMemo, useState } from "react";
import type { Evidence, NextStreamPlan, PlanOption, ReconcileReport, RuleIncident } from "../contract";
import { useApi, useRpc } from "../api/context";
import { boundStr, dateTime, msStr, parseRef, pct } from "../lib/format";
import { useDesk } from "../state";
import { DisclosureRow } from "../ui/Disclosure";
import { DiffView, PlanLetter, diffFiles } from "../ui/DiffView";
import { Button, Chip, Empty, ErrorLine, ErrorState, Loading, Popover, SectionHeader, Sheet, Toolbar, cx } from "../ui/primitives";
import { TranscriptInspector } from "../ui/TranscriptViewer";

type OpenRef = (ref: string) => void;

function RefChips({ refs, onOpen }: { refs: string[]; onOpen: OpenRef }) {
  return (
    <span className="chips">
      {refs.map((r) => {
        const p = parseRef(r);
        return (
          <Chip key={r} tone="accent" onClick={() => onOpen(r)} title={`Open ${r}`}>
            {p ? `L${p.line} · ${p.runId.length > 28 ? `…${p.runId.slice(-26)}` : p.runId}` : r}
          </Chip>
        );
      })}
    </span>
  );
}

function EvidenceList({ items, tone, onOpen }: { items: Evidence[]; tone: "ok" | "bad"; onOpen: OpenRef }) {
  return (
    <ul className="evidence">
      {items.map((e, i) => (
        <li key={i}>
          <span className={cx("ev-mark", tone)}>{tone === "ok" ? "✓" : "✗"}</span>
          <span className="ev-claim">{e.claim}</span>
          <RefChips refs={e.refs} onOpen={onOpen} />
        </li>
      ))}
    </ul>
  );
}

const KIND_LABEL: Record<PlanOption["kind"], string> = {
  character_change: "character",
  segment_change: "segment",
  topic: "topic",
  experiment: "experiment",
};

function incidentTone(i: RuleIncident) {
  return i.state === "fail" ? "bad" : i.state === "not_evaluable" ? "warn" : "neutral";
}

function ReportBody({ report, onOpen }: { report: ReconcileReport; onOpen: OpenRef }) {
  const maxMsgs = Math.max(1, ...report.ranks.by_rank.map((r) => r.messages));
  return (
    <div className="report" data-xrows="">
      <SectionHeader right={<span className="muted">{report.segments.length}</span>}>Segment findings</SectionHeader>
      {report.segments.map((s) => (
        <DisclosureRow
          key={s.segment}
          summary={
            <>
              <span className="c-id mono">{s.segment}</span>
              <span className="c-name muted">{s.runs} runs</span>
              <span className="c-name">SES {typeof s.ses_mean === "number" ? s.ses_mean.toFixed(2) : "–"}</span>
              <span className="c-grow chips">
                <Chip tone="ok">✓ {s.what_worked.length} worked</Chip>
                <Chip tone={s.what_failed.length ? "bad" : "neutral"}>✗ {s.what_failed.length} failed</Chip>
              </span>
            </>
          }
        >
          {s.what_worked.length > 0 && <EvidenceList items={s.what_worked} tone="ok" onOpen={onOpen} />}
          {s.what_failed.length > 0 && <EvidenceList items={s.what_failed} tone="bad" onOpen={onOpen} />}
        </DisclosureRow>
      ))}

      <SectionHeader right={<span className="muted">{report.tone.length}</span>}>Tone findings</SectionHeader>
      {report.tone.map((t) => {
        const dims = Object.entries(t.quality_rates).sort((a, b) => a[1].rate - b[1].rate);
        return (
          <DisclosureRow
            key={t.tone_variant}
            summary={
              <>
                <span className="c-tone">{t.tone_variant}</span>
                <span className="c-name">
                  {t.rule_failures > 0 ? <Chip tone="bad">✗ {t.rule_failures} rule fails</Chip> : <Chip tone="ok">rules held</Chip>}
                </span>
                <span className="c-grow mini-rates">
                  {dims.map(([d, q]) => (
                    <span key={d} className={cx("mini", q.rate >= 0.8 ? "heat-good" : q.rate >= 0.6 ? "heat-warn" : "heat-bad")} title={`${d}: ${pct(q.rate, 1)} (n ${q.n}) · 95% ${boundStr(q.bound)}`}>
                      {pct(q.rate)}
                    </span>
                  ))}
                </span>
              </>
            }
          >
            <div className="table">
              {dims.map(([d, q]) => (
                <div className="tr" key={d}>
                  <span className="c-grow">{d.replace(/_/g, " ")}</span>
                  <span className="c-name mono">{pct(q.rate, 1)}</span>
                  <span className="c-name mono muted">n {q.n}</span>
                  <span className="c-wide mono muted">{boundStr(q.bound)}</span>
                </div>
              ))}
            </div>
          </DisclosureRow>
        );
      })}

      <SectionHeader right={<span className="muted">picked → answered</span>}>Rank engagement</SectionHeader>
      {report.ranks.by_rank.map((r) => (
        <DisclosureRow
          key={r.rank}
          summary={
            <>
              <span className="c-tone">{r.rank.replace("_", " ")}</span>
              <span className="c-name mono muted">{r.chatters} chatters</span>
              <span className="c-grow">
                <span className="bar wide" title={`${r.messages} messages`}>
                  <span style={{ width: `${(100 * r.messages) / maxMsgs}%` }} />
                </span>
              </span>
              <span className="c-name mono">
                {r.answered}/{r.picked}
              </span>
              <span className="c-name mono muted">{r.picked ? pct(r.answered / r.picked) : "–"}</span>
            </>
          }
        />
      ))}

      <SectionHeader right={<span className="muted">{report.red_lines.length}</span>}>Red-line incidents</SectionHeader>
      {report.red_lines.length === 0 ? (
        <div className="xrow muted">No incidents</div>
      ) : (
        report.red_lines.map((i, k) => (
          <DisclosureRow
            key={k}
            summary={
              <>
                <span className="c-name">
                  <Chip tone={incidentTone(i)}>{i.state.replace("_", " ")}</Chip>
                </span>
                <span className="c-rule mono">{i.rule}</span>
                <span className="c-grow ellipsis muted mono">{i.run_id}</span>
              </>
            }
          >
            {i.note && <p className="prose">{i.note}</p>}
            <RefChips refs={i.refs} onOpen={onOpen} />
          </DisclosureRow>
        ))
      )}

      <SectionHeader>Latency</SectionHeader>
      <DisclosureRow
        summary={
          <>
            <span className="c-tone">core first text</span>
            <span className="c-name mono">p50 {msStr(report.latency.core_first_text_ms.p50_ms)}</span>
            <span className="c-name mono">p95 {msStr(report.latency.core_first_text_ms.p95_ms)}</span>
            <span className="c-grow muted mono">n {report.latency.core_first_text_ms.n}</span>
          </>
        }
      >
        {report.latency.by_role && (
          <div className="table">
            {Object.entries(report.latency.by_role).map(([role, l]) => (
              <div className="tr" key={role}>
                <span className="c-grow mono">{role}</span>
                <span className="c-name mono">p50 {msStr(l.p50_ms)}</span>
                <span className="c-name mono">p95 {msStr(l.p95_ms)}</span>
                <span className="c-name mono muted">n {l.n}</span>
              </div>
            ))}
          </div>
        )}
      </DisclosureRow>
    </div>
  );
}

function OptionCard(props: { option: PlanOption; selected: boolean; onToggle: () => void; onDiff: () => void; onOpen: OpenRef }) {
  const o = props.option;
  const refs = o.evidence.flatMap((e) => e.refs);
  return (
    <div
      className={cx("opt", props.selected && "on")}
      role="checkbox"
      aria-checked={props.selected}
      tabIndex={0}
      onClick={props.onToggle}
      onKeyDown={(e) => {
        if (e.key === " " || e.key === "Enter") {
          e.preventDefault();
          props.onToggle();
        }
      }}
    >
      <div className="opt-h">
        <span className="opt-check" aria-hidden="true">
          {props.selected ? "✓" : ""}
        </span>
        <Chip tone={o.kind === "character_change" ? "accent" : "neutral"}>{KIND_LABEL[o.kind]}</Chip>
        <span className="opt-title" title={o.title}>
          {o.title}
        </span>
      </div>
      <div className="opt-body" title={o.rationale}>
        {o.rationale}
      </div>
      <div className="opt-f" onClick={(e) => e.stopPropagation()}>
        <Popover
          trigger={(open) => (
            <Chip onClick={open} title="Evidence">
              {o.evidence.length} evidence · {refs.length} refs
            </Chip>
          )}
        >
          {() => <EvidenceList items={o.evidence} tone="ok" onOpen={props.onOpen} />}
        </Popover>
        {o.diff && (
          <Button variant="ghost" onClick={props.onDiff} title={diffFiles(o.diff).join(", ")}>
            View diff
          </Button>
        )}
      </div>
    </div>
  );
}

export function ReconcileView() {
  const { client } = useApi();
  const { reconcileTournament, setReconcileTournament, reportId, setReportId } = useDesk();
  const tournaments = useRpc("tournament.list", {});
  const [source, setSource] = useState<string>(reconcileTournament ?? "");
  const [running, setRunning] = useState(false);
  const [runError, setRunError] = useState<unknown>(null);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [diffOf, setDiffOf] = useState<PlanOption | null>(null);
  const [plan, setPlan] = useState<NextStreamPlan | null>(null);
  const [choosing, setChoosing] = useState(false);
  const [chooseError, setChooseError] = useState<unknown>(null);
  const [openRef, setOpenRef] = useState<{ runId: string; line: number } | null>(null);

  useEffect(() => {
    if (reconcileTournament) setSource(reconcileTournament);
  }, [reconcileTournament]);
  useEffect(() => {
    const first = tournaments.data?.[tournaments.data.length - 1];
    if (!source && first) setSource(first.tournament_id);
  }, [tournaments.data, source]);

  const report = useRpc("reconcile.report", reportId ? { report_id: reportId } : null, reportId ?? "");
  const proposals = useRpc("plan.proposals", reportId ? { report_id: reportId } : null, reportId ?? "");

  useEffect(() => {
    setChosen(new Set());
    setPlan(null);
  }, [reportId]);

  const run = async () => {
    if (!source) return;
    setRunning(true);
    setRunError(null);
    try {
      const r = await client.call("reconcile.run", { source: { tournament_id: source } });
      setReconcileTournament(source);
      setReportId(r.report_id);
    } catch (e) {
      setRunError(e);
    } finally {
      setRunning(false);
    }
  };

  const choose = async () => {
    if (!reportId || !chosen.size) return;
    setChoosing(true);
    setChooseError(null);
    try {
      setPlan(await client.call("plan.choose", { report_id: reportId, option_ids: [...chosen] }));
    } catch (e) {
      setChooseError(e);
    } finally {
      setChoosing(false);
    }
  };

  const onOpen: OpenRef = (ref) => {
    const p = parseRef(ref);
    if (p) setOpenRef(p);
  };

  const options = proposals.data ?? [];
  const toggle = (id: string) =>
    setChosen((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });

  const sourceLabel = useMemo(() => {
    const s = report.data?.source;
    if (!s) return "";
    return "tournament_id" in s ? s.tournament_id : `${s.run_ids.length} runs`;
  }, [report.data]);

  return (
    <div className="view">
      <div className="main">
        <Toolbar
          title="Reconcile & Plan"
          subtitle={report.data ? <span className="mono">{report.data.report_id} · {sourceLabel} · {dateTime(report.data.created_ms)}</span> : "Between streams: what worked, what failed, what next"}
        >
          <label className="field-inline">
            <span className="muted">Source</span>
            <select value={source} onChange={(e) => setSource(e.target.value)} aria-label="Source tournament">
              {!tournaments.data?.length && <option value="">No tournaments</option>}
              {tournaments.data?.map((t) => (
                <option key={t.tournament_id} value={t.tournament_id}>
                  {t.tournament_id} ({t.completed}/{t.runs})
                </option>
              ))}
            </select>
          </label>
          <Button variant="primary" onClick={() => void run()} disabled={running || !source}>
            {running ? "Reconciling…" : "Run reconcile"}
          </Button>
        </Toolbar>
        <div className="content stack">
          {runError ? <ErrorState error={runError} onRetry={() => void run()} /> : null}
          {!reportId && !runError ? (
            tournaments.error ? (
              <ErrorState error={tournaments.error} onRetry={tournaments.reload} />
            ) : (
              <Empty title="No report yet">Pick a tournament and run reconcile. The reconciler reads its transcripts, verdicts and ledgers.</Empty>
            )
          ) : null}
          {reportId && report.error ? <ErrorState error={report.error} onRetry={report.reload} /> : null}
          {reportId && report.loading && !report.data ? <Loading label="Loading report" /> : null}
          {report.data && (
            <>
              <div className="summary-line">
                <span className="ellipsis">{report.data.summary}</span>
                <Popover trigger={(open) => <Button variant="ghost" onClick={open}>More</Button>}>
                  {() => <p className="prose pop-prose">{report.data!.summary}</p>}
                </Popover>
              </div>
              <ReportBody report={report.data} onOpen={onOpen} />

              <SectionHeader
                right={
                  <span className="hm-summary">
                    <span className="muted">{chosen.size} selected</span>
                    <Button variant="primary" onClick={() => void choose()} disabled={!chosen.size || choosing}>
                      {choosing ? "Composing…" : "Choose"}
                    </Button>
                  </span>
                }
              >
                Plan options
              </SectionHeader>
              <ErrorLine error={chooseError} />
              {proposals.error ? (
                <ErrorState error={proposals.error} onRetry={proposals.reload} />
              ) : (
                <div className="opts">
                  {options.map((o) => (
                    <OptionCard key={o.option_id} option={o} selected={chosen.has(o.option_id)} onToggle={() => toggle(o.option_id)} onDiff={() => setDiffOf(o)} onOpen={onOpen} />
                  ))}
                </div>
              )}
            </>
          )}
        </div>
      </div>
      {openRef && <TranscriptInspector runId={openRef.runId} highlightLine={openRef.line} onClose={() => setOpenRef(null)} />}
      {diffOf?.diff && (
        <Sheet title={`${diffOf.title} · ${diffFiles(diffOf.diff).join(", ")}`} onClose={() => setDiffOf(null)} wide
          footer={
            <>
              <Button onClick={() => setDiffOf(null)}>Close</Button>
              <Button
                variant="primary"
                onClick={() => {
                  if (!chosen.has(diffOf.option_id)) toggle(diffOf.option_id);
                  setDiffOf(null);
                }}
              >
                {chosen.has(diffOf.option_id) ? "Selected" : "Select option"}
              </Button>
            </>
          }
        >
          <DiffView diff={diffOf.diff} />
        </Sheet>
      )}
      {plan && (
        <Sheet title="Next stream plan" onClose={() => setPlan(null)} wide footer={<Button variant="primary" onClick={() => setPlan(null)}>Done</Button>}>
          <PlanLetter plan={plan} />
        </Sheet>
      )}
    </div>
  );
}
