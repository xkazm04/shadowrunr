// What each of the Lord's directives came to in a report (`ReconcileReport.directives`, computed
// by the reconciler from the judged runs, never by the synthesis): a verdict chip, `k of n` with
// the 95% bound, the small-sample mark, and the evidence refs into Brain transcripts. Used beside
// each row of the Directives tab and as a section of the Report tab.
import type { Directive, DirectiveOutcome, ReconcileReport } from "@shadowrunr/contract";
import { useRpc } from "../../data";
import { Chip } from "../../ui/Chip";
import { RateBar } from "../../ui/RateBar";
import { Section } from "../../ui/Section";
import styles from "./Layer2.module.css";
import { outcomeCount, outcomeOrder, SMALL_SAMPLE_N, smallSample, VERDICT_LABEL, VERDICT_TONE } from "./model";
import { RefLinks } from "./Refs";

/** Refs shown in a table cell; the rest are in the report section. */
const CELL_REFS = 2;

const pct = (x: number) => `${Math.round(x * 100)}%`;
const what = (o: DirectiveOutcome) => (o.kind === "prevent" ? "held" : "attempted");

function SmallSample({ n }: { n: number }) {
  return (
    <span data-role="reconcile-small-sample">
      <Chip tone="muted" title={`Rests on ${n} judged ${n === 1 ? "run" : "runs"}, fewer than ${SMALL_SAMPLE_N}: not settled`}>
        small sample
      </Chip>
    </span>
  );
}

/** `held 2 of 4 · 95% 15–85%`, or why there is no rate. */
function Count({ o }: { o: DirectiveOutcome }) {
  const c = outcomeCount(o);
  if (!c || !o.rate) return <span className={styles.dim}>no judged run of {o.runs}</span>;
  return (
    <span>
      {what(o)} {c.k} of {c.n} <span className={styles.dim}>· 95% {pct(o.rate.bound.lo)}–{pct(o.rate.bound.hi)}</span>
    </span>
  );
}

/**
 * The Directives tab's outcome cell. `reportId` undefined: no report to judge by, a dash.
 * No outcome in the report: "Not judged", never "Held".
 */
export function OutcomeCell({ outcome, reportId }: { outcome: DirectiveOutcome | undefined; reportId: string | undefined }) {
  if (reportId === undefined) {
    return (
      <span className={styles.dim} data-role="reconcile-directive-outcome">
        –
      </span>
    );
  }
  if (!outcome) {
    return (
      <span className={styles.dim} data-role="reconcile-directive-outcome" data-verdict="none">
        Not judged in {reportId}
      </span>
    );
  }
  const refs = outcome.refs ?? [];
  return (
    <span className={styles.outcome} data-role="reconcile-directive-outcome" data-verdict={outcome.verdict}>
      <Chip tone={VERDICT_TONE[outcome.verdict]}>{VERDICT_LABEL[outcome.verdict]}</Chip>
      <Count o={outcome} />
      {smallSample(outcome) && outcome.rate && <SmallSample n={outcome.rate.n} />}
      <RefLinks refs={refs.slice(0, CELL_REFS)} />
      {refs.length > CELL_REFS && <span className={styles.refMore}>+{refs.length - CELL_REFS} more</span>}
    </span>
  );
}

/** The Report tab's section: every judged directive, worst first, named by its standing text. */
export function ReportDirectives({ report }: { report: ReconcileReport }) {
  const list = useRpc("directive.list", report.directives === undefined ? null : {});
  if (report.directives === undefined) return null;
  const byId = new Map<string, Directive>((list.data ?? []).map((d) => [d.id, d]));
  const name = (id: string) => byId.get(id)?.text ?? (list.data === undefined ? `directive ${id}` : `retired directive ${id}`);
  const rows = outcomeOrder(report.directives);
  return (
    <div data-role="reconcile-report-directives">
      <Section
        title={
          <>
            The Lord’s directives <small className={styles.small}>worst first, n and 95% bound shown</small>
          </>
        }
      >
        {rows.length === 0 ? (
          <p className={styles.dim}>No directive was standing for these runs.</p>
        ) : (
          <ul className={styles.plain}>
            {rows.map((o) => (
              <li key={o.check} className={styles.outcome} data-directive={o.id} data-verdict={o.verdict}>
                <Chip tone={VERDICT_TONE[o.verdict]}>{VERDICT_LABEL[o.verdict]}</Chip>
                <span>
                  <b data-role="reconcile-directive-name">{name(o.id)}</b> <small>{o.kind}</small>
                </span>
                <span className={styles.outcome}>
                  <Count o={o} />
                  {o.rate && <RateBar label={what(o)} rate={o.rate.rate} n={o.rate.n} bound={o.rate.bound} />}
                  {smallSample(o) && o.rate && <SmallSample n={o.rate.n} />}
                </span>
                <RefLinks refs={o.refs} />
              </li>
            ))}
          </ul>
        )}
      </Section>
    </div>
  );
}
