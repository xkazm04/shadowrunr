// `reconcile/report[/held|failed]`: the report in its own words, what held and failed per
// segment with evidence linked into Brain transcripts, red lines, ranks and tone quality.
import type { Evidence, RankRow, ReconcileReport, RuleIncident, SegmentFinding, ToneFinding } from "@shadowrunr/contract";
import { useMemo, useState } from "react";
import { pageRoute, RouteLink } from "../../router";
import { QueryView } from "../../kit";
import { Chip, FilterTable, Head, RateBar, Section, StatBand, type Column } from "../../ui";
import { useReport } from "./hooks";
import styles from "./Layer2.module.css";
import { dateTime, incidentGroups, ruleLabel, sourceLabel, triptych } from "./model";
import { RefLinks } from "./Refs";

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
          { label: "red-line incidents", value: report.red_lines.length.toLocaleString("en-US") },
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
      <Incidents incidents={report.red_lines} />
      <Ranks rows={report.ranks.by_rank ?? []} />
      {report.tone.map((tone, i) => (
        <ToneSection key={`${tone.tone_variant}-${tone.instrument_hash ?? i}`} tone={tone} />
      ))}
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
        <button type="button" className={styles.secondary} onClick={() => setLimit((n) => n + EVIDENCE_PAGE)}>
          Show {Math.min(EVIDENCE_PAGE, items.length - shown.length)} more of {items.length - shown.length}
        </button>
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

type SourceFacet = "all" | "verdict" | "moderation";
/** An incident with its position, so identical incidents still get distinct row keys. */
type Row = RuleIncident & { readonly k: number };

const INCIDENT_COLUMNS: readonly Column<Row>[] = [
  { id: "source", header: "Source", cell: (i) => <Chip tone={i.source === "verdict" ? "fail" : "muted"}>{i.source}</Chip>, sortValue: (i) => i.source },
  { id: "rule", header: "Rule", cell: (i) => ruleLabel(i.rule), sortValue: (i) => i.rule },
  { id: "action", header: "Action", cell: (i) => [i.stage, i.action].filter(Boolean).join(" · ") || "–" },
  { id: "run", header: "Run", cell: (i) => <code>{i.run_id}</code>, sortValue: (i) => i.run_id },
  { id: "refs", header: "Lines", cell: (i) => <RefLinks refs={i.refs} /> },
];

function Incidents({ incidents }: { incidents: readonly RuleIncident[] }) {
  const groups = useMemo(() => incidentGroups(incidents), [incidents]);
  const rows = useMemo(() => incidents.map((x, k): Row => ({ ...x, k })), [incidents]);
  const judged = useMemo(() => incidents.filter((i) => i.source === "verdict").length, [incidents]);
  const top = groups.slice(0, 12);
  const max = Math.max(1, top[0]?.n ?? 1);
  return (
    <Section title={<>Red-line and moderation incidents <small className={styles.small}>{incidents.length.toLocaleString("en-US")} logged</small></>}>
      {incidents.length === 0 ? (
        <p className={styles.dim}>No incidents in this report.</p>
      ) : (
        <>
          <div className={styles.incs} data-role="reconcile-incidents">
            {top.map((g) => (
              <div className={styles.inc} key={g.rule}>
                <span>{ruleLabel(g.rule)}</span>
                <i style={{ width: `${((g.n / max) * 100).toFixed(1)}%` }} />
                <b>{g.n.toLocaleString("en-US")}</b>
              </div>
            ))}
            {groups.length > top.length && <p className={styles.dim}>and {groups.length - top.length} rarer rules, in the table below</p>}
          </div>
          <div className={styles.tableBox}>
            <FilterTable<Row, SourceFacet>
              rows={rows}
              columns={INCIDENT_COLUMNS}
              getKey={(i) => String(i.k)}
              haystack={(i) => `${i.rule} ${i.run_id} ${i.action ?? ""} ${i.stage ?? ""}`}
              label="Incidents"
              placeholder="Filter by rule or run"
              facets={{
                label: "Source",
                initial: "all",
                options: [
                  { id: "all", label: "All" },
                  { id: "verdict", label: "Judge", count: judged },
                  { id: "moderation", label: "Moderation", count: incidents.length - judged },
                ],
                predicate: (i, f) => f === "all" || i.source === f,
              }}
            />
          </div>
        </>
      )}
    </Section>
  );
}

const rankLabel = (r: string) => r.replace(/_/g, " ");

function Ranks({ rows }: { rows: readonly RankRow[] }) {
  return (
    <Section title="Which ranks got answered">
      {rows.length === 0 ? (
        <p className={styles.dim}>No ranked chat in this report.</p>
      ) : (
        <div className={styles.tblwrap}>
          <table className={styles.tbl}>
            <thead>
              <tr>
                <th>Rank</th>
                <th className={styles.num}>Viewers</th>
                <th className={styles.num}>Messages</th>
                <th>Picked</th>
                <th>Answered</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((x) => (
                <tr key={x.rank}>
                  <td>
                    <span className={styles.rkDot} data-rank={x.rank} />
                    {rankLabel(x.rank)}
                  </td>
                  <td className={styles.num}>{x.viewers.toLocaleString("en-US")}</td>
                  <td className={styles.num}>{x.messages.toLocaleString("en-US")}</td>
                  <td>{x.pick_rate ? <RateBar label="Picked" rate={x.pick_rate.rate} n={x.pick_rate.n} bound={x.pick_rate.bound} /> : `${x.picked}`}</td>
                  <td>{x.answer_rate ? <RateBar label="Answered" rate={x.answer_rate.rate} n={x.answer_rate.n} bound={x.answer_rate.bound} /> : `${x.answered}`}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
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
