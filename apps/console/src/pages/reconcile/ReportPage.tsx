// `reconcile/report[/held|failed]`: the report in its own words, what held and failed per
// segment with evidence linked into Brain transcripts, red lines (Incidents), ranks, tone
// quality and what the synthesis prompt carried (Synthesis).
import type { Evidence, RankRow, ReconcileReport, SegmentFinding, ToneFinding } from "@shadowrunr/contract";
import { useState } from "react";
import { pageRoute, RouteLink } from "../../router";
import { QueryView } from "../../kit";
import { RankEngagement } from "../../kit/RankEngagement";
import { RateBar } from "../../ui/RateBar";
import { Head, Section } from "../../ui/Section";
import { StatBand } from "../../ui/StatBand";
import { useReport } from "./hooks";
import styles from "./Layer2.module.css";
import { incidentTotal, Incidents } from "./Incidents";
import { dateTime, ruleLabel, sourceLabel, triptych } from "./model";
import { RefLinks } from "./Refs";
import { Synthesis } from "./Synthesis";
import { Button } from "../../ui/Button";

export type ReportFilter = "all" | "held" | "failed";

export function ReportPage({ reportId, filter }: { reportId: string; filter: ReportFilter }) {
  const { report, proposals } = useReport(reportId);
  return (
    <>
      <Head kicker="Reconcile & plan · layer 2" title="The report" lede="What the character did between streams: what held, what failed, and the evidence, line by line." />
      <QueryView query={report} emptyTitle="No report with this id">
        {(r) => <ReportBody report={r} filter={filter} proposed={proposals.data?.length} />}
      </QueryView>
    </>
  );
}

function ReportBody({ report, filter, proposed }: { report: ReconcileReport; filter: ReportFilter; proposed: number | undefined }) {
  const t = triptych(report, []);
  return (
    <div data-role="reconcile-report">
      <p className={styles.meta}>
        <code>{report.report_id}</code> · {sourceLabel(report.source)} · {dateTime(report.created_ms)}
      </p>
      <StatBand
        stats={[
          { label: "held", value: t.held },
          { label: "failed", value: t.failed, tone: "fail" },
          { label: "proposed", value: proposed ?? "–" },
          { label: "red-line incidents", value: incidentTotal(report).toLocaleString("en-US") },
        ]}
      />
      <nav className={styles.segFilter} aria-label="Show">
        <FilterLink to="all" current={filter} label={`All ${t.held + t.failed}`} />
        <FilterLink to="held" current={filter} label={`Held ${t.held}`} />
        <FilterLink to="failed" current={filter} label={`Failed ${t.failed}`} />
      </nav>
      <div className={styles.summary}>
        <h2>In the report’s words</h2>
        <p className={styles.voice}>{report.summary}</p>
      </div>
      {report.segments.map((s) => (
        <SegmentSection key={s.segment} s={s} filter={filter} />
      ))}
      <Incidents report={report} />
      <Ranks rows={report.ranks.by_rank ?? []} />
      {report.tone.map((tone, i) => (
        <ToneSection key={`${tone.tone_variant}-${tone.instrument_hash ?? i}`} tone={tone} />
      ))}
      <Synthesis input={report.synthesis_input} />
      {report.dropped && report.dropped.length > 0 && (
        <details className={styles.dropped}>
          <summary>The verifier dropped {report.dropped.length} item(s) it could not stand behind</summary>
          <ul>
            {report.dropped.map((d, i) => (
              <li key={i}>
                <b>{d.what}</b> {d.label}: {d.reason}
              </li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}

function FilterLink({ to, current, label }: { to: ReportFilter; current: ReportFilter; label: string }) {
  const route = to === "all" ? pageRoute("reconcile", "report") : pageRoute("reconcile", "report", to);
  return (
    <RouteLink to={route} aria-current={to === current ? "true" : undefined}>
      {label}
    </RouteLink>
  );
}

/** Evidence items shown per column before "show more" (keeps the DOM bounded at any size). */
export const EVIDENCE_PAGE = 20;

function EvidenceList({ items, tone }: { items: readonly Evidence[]; tone: "g" | "e" }) {
  const [limit, setLimit] = useState(EVIDENCE_PAGE);
  const shown = items.slice(0, limit);
  return (
    <>
      <ul className={`${styles.evid} ${tone === "e" ? styles.evidE : ""}`}>
        {shown.map((e, i) => (
          <li key={i}>
            <p>{e.claim}</p>
            <RefLinks refs={e.refs} />
          </li>
        ))}
      </ul>
      {items.length > shown.length && (
        <Button type="button" variant="secondary" onClick={() => setLimit((n) => n + EVIDENCE_PAGE)}>
          Show {Math.min(EVIDENCE_PAGE, items.length - shown.length)} more of {items.length - shown.length}
        </Button>
      )}
    </>
  );
}

function SegmentSection({ s, filter }: { s: SegmentFinding; filter: ReportFilter }) {
  const worked = s.what_worked ?? [];
  const failed = s.what_failed ?? [];
  return (
    <Section
      title={
        <>
          {s.segment} <small className={styles.small}>{s.runs} runs</small>
        </>
      }
      right={s.ses_mean === undefined ? undefined : `SES ${s.ses_mean.toFixed(2)}`}
    >
      <div className={styles.cols2}>
        {filter !== "failed" && (
          <div>
            <h3 className={styles.hG}>
              What held <small>{worked.length}</small>
            </h3>
            {worked.length === 0 ? <p className={styles.dim}>Nothing recorded.</p> : <EvidenceList items={worked} tone="g" />}
          </div>
        )}
        {filter !== "held" && (
          <div>
            <h3 className={styles.hE}>
              What failed <small>{failed.length}</small>
            </h3>
            {failed.length === 0 ? <p className={styles.dim}>Nothing recorded.</p> : <EvidenceList items={failed} tone="e" />}
          </div>
        )}
      </div>
    </Section>
  );
}

function Ranks({ rows }: { rows: readonly RankRow[] }) {
  return (
    <Section title="Which ranks got answered">
      {rows.length === 0 ? <p className={styles.dim}>No ranked chat in this report.</p> : <RankEngagement rows={rows} />}
    </Section>
  );
}

function ToneSection({ tone }: { tone: ToneFinding }) {
  const rows = Object.entries(tone.quality_rates)
    .flatMap(([d, q]) => (q ? [{ d, q }] : []))
    .sort((a, b) => a.q.rate - b.q.rate);
  const fails = tone.rule_failures ?? [];
  return (
    <Section
      title={
        <>
          Quality, {tone.tone_variant} tone <small className={styles.small}>n and 95% bound shown, worst first</small>
        </>
      }
      right={[tone.runs === undefined ? "" : `${tone.runs} judged runs`, tone.instrument_hash ? `instrument ${tone.instrument_hash.slice(0, 8)}` : ""].filter(Boolean).join(" · ") || undefined}
    >
      <div className={styles.qrows}>
        {rows.map(({ d, q }) => (
          <div className={styles.qrow} key={d}>
            <span>{d.replace(/_/g, " ")}</span>
            <RateBar label={d.replace(/_/g, " ")} rate={q.rate} n={q.n} bound={q.bound} />
          </div>
        ))}
      </div>
      {fails.length > 0 && (
        <>
          <h3 className={styles.hE}>Rule failures</h3>
          <ul className={`${styles.evid} ${styles.evidE}`}>
            {fails.map((f) => (
              <li key={f.rule}>
                <p>
                  {ruleLabel(f.rule)}: failed {f.fails} of {f.n}
                </p>
                <RefLinks refs={f.refs} />
              </li>
            ))}
          </ul>
        </>
      )}
    </Section>
  );
}
