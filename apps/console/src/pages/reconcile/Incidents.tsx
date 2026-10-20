// The report's red-line and moderation incidents. The daemon sends every incident counted by
// rule (`red_line_counts`) and, for a large report, only the first page of the list
// (`red_lines_total` says how many there are): the rest pages in through `reconcile.incidents`,
// one rule at a time when a bar is picked. A small report carries its whole list inline.
import type { ReconcileReport, RuleIncident } from "@shadowrunr/contract";
import { useMemo, useState } from "react";
import { usePaged } from "../../data";
import { int } from "../../kit/events";
import { QueryView } from "../../kit";
import { Chip } from "../../ui/Chip";
import { FilterTable, type Column } from "../../ui/FilterTable";
import { Section } from "../../ui/Section";
import styles from "./Layer2.module.css";
import { incidentGroups, ruleLabel } from "./model";
import { RefLinks } from "./Refs";
import { Button } from "../../ui/Button";

type SourceFacet = "all" | "verdict" | "moderation";
/** An incident with its position, so identical incidents still get distinct row keys. */
type Row = RuleIncident & { readonly k: number };

/** Pages of `reconcile.incidents` read per "load more". */
export const INCIDENT_PAGE = 500;
const STEP = 4;

const COLUMNS: readonly Column<Row>[] = [
  { id: "source", header: "Source", cell: (i) => <Chip tone={i.source === "verdict" ? "fail" : "muted"}>{i.source}</Chip>, sortValue: (i) => i.source },
  { id: "rule", header: "Rule", cell: (i) => ruleLabel(i.rule), sortValue: (i) => i.rule },
  { id: "action", header: "Action", cell: (i) => [i.stage, i.action].filter(Boolean).join(" · ") || "–" },
  { id: "run", header: "Run", cell: (i) => <code>{i.run_id}</code>, sortValue: (i) => i.run_id },
  { id: "refs", header: "Lines", cell: (i) => <RefLinks refs={i.refs} /> },
];

/** The incident total: the daemon's count when it cut the list, else the list itself. */
export const incidentTotal = (r: ReconcileReport): number => r.red_lines_total ?? r.red_lines.length;

function Table({ rows, judged, total }: { rows: readonly RuleIncident[]; judged: number | undefined; total: number }) {
  const keyed = useMemo(() => rows.map((x, k): Row => ({ ...x, k })), [rows]);
  return (
    <div className={styles.tableBox}>
      <FilterTable<Row, SourceFacet>
        rows={keyed}
        columns={COLUMNS}
        getKey={(i) => String(i.k)}
        haystack={(i) => `${i.rule} ${i.run_id} ${i.action ?? ""} ${i.stage ?? ""}`}
        label="Incidents"
        placeholder="Filter by rule or run"
        facets={{
          label: "Source",
          initial: "all",
          options: [
            { id: "all", label: "All" },
            { id: "verdict", label: "Judge", ...(judged === undefined ? {} : { count: judged }) },
            { id: "moderation", label: "Moderation", ...(judged === undefined ? {} : { count: total - judged }) },
          ],
          predicate: (i, f) => f === "all" || i.source === f,
        }}
      />
    </div>
  );
}

/** A cut report: the incidents page in from the daemon, all rules or the one picked. */
function Paged({ report, rule }: { report: ReconcileReport; rule: string | undefined }) {
  const [pages, setPages] = useState(1);
  const incidents = usePaged("reconcile.incidents", { report_id: report.report_id, limit: INCIDENT_PAGE, ...(rule === undefined ? {} : { rule }) }, pages);
  const loaded = incidents.query.data?.length ?? 0;
  return (
    <>
      <p className={styles.dim} data-role="reconcile-incidents-paged">
        Showing {int(loaded)} of {int(rule === undefined ? incidentTotal(report) : (incidentGroups(report).find((g) => g.rule === rule)?.n ?? loaded))}
        {rule !== undefined && ` for ${ruleLabel(rule)}`}.{" "}
        {incidents.more && (
          <Button type="button" variant="secondary" onClick={() => setPages((p) => p + STEP)}>
            Load {int(STEP * INCIDENT_PAGE)} more
          </Button>
        )}
      </p>
      <QueryView query={incidents.query} isEmpty={(d) => d.length === 0} emptyTitle="No incidents for this rule">
        {(rows) => <Table rows={rows} judged={undefined} total={loaded} />}
      </QueryView>
    </>
  );
}

export function Incidents({ report }: { report: ReconcileReport }) {
  const groups = useMemo(() => incidentGroups(report), [report]);
  const [rule, setRule] = useState<string | undefined>(undefined);
  const total = incidentTotal(report);
  const cut = report.red_lines_total !== undefined;
  const judged = useMemo(
    () => (report.red_line_counts?.length ? report.red_line_counts.filter((c) => c.source === "verdict").reduce((n, c) => n + c.count, 0) : report.red_lines.filter((i) => i.source === "verdict").length),
    [report],
  );
  const top = groups.slice(0, 12);
  const max = Math.max(1, top[0]?.n ?? 1);
  return (
    <Section title={<>Red-line and moderation incidents <small className={styles.small}>{int(total)} logged</small></>}>
      {total === 0 ? (
        <p className={styles.dim}>No incidents in this report.</p>
      ) : (
        <>
          <div className={styles.incs} data-role="reconcile-incidents">
            {top.map((g) =>
              cut && g.rule !== "alleged" ? (
                <button type="button" className={styles.inc} key={g.rule} aria-pressed={rule === g.rule} onClick={() => setRule((r) => (r === g.rule ? undefined : g.rule))} title={`Show only ${ruleLabel(g.rule)}`}>
                  <span>{ruleLabel(g.rule)}</span>
                  <i style={{ width: `${((g.n / max) * 100).toFixed(1)}%` }} />
                  <b>{int(g.n)}</b>
                </button>
              ) : (
                <div className={styles.inc} key={g.rule}>
                  <span>{ruleLabel(g.rule)}</span>
                  <i style={{ width: `${((g.n / max) * 100).toFixed(1)}%` }} />
                  <b>{int(g.n)}</b>
                </div>
              ),
            )}
            {groups.length > top.length && <p className={styles.dim}>and {groups.length - top.length} rarer rules, in the table below</p>}
          </div>
          {cut ? <Paged report={report} rule={rule} /> : <Table rows={report.red_lines} judged={judged} total={total} />}
        </>
      )}
    </Section>
  );
}
