// Layer 1: the winner's held / failed / proposed triptych over the rising halftone sun
// (l1.js P3.L1.reconcile, l1.css .rc-*), on the newest report. Each column is a lit link into
// its layer-2 page.
import type { ReactNode } from "react";
import { pageRoute, RouteLink, type Route } from "../../router";
import { useConnection } from "../../data";
import { PosterFrame } from "../../kit/PosterFrame";
import { CountUp } from "./CountUp";
import { useReport, useReportId } from "./hooks";
import styles from "./Layer1.module.css";
import { barCount, caution, triptych, type Triptych } from "./model";

export function Layer1({ active }: { readonly active: boolean }) {
  if (!active) return null;
  return <ReconcilePoster />;
}

type Slip = { readonly kind: "caution" | "state"; readonly text: string; readonly action?: ReactNode } | undefined;

function ReconcilePoster() {
  const { reportId, pending } = useReportId();
  const { report, proposals } = useReport(reportId);
  const conn = useConnection();

  const t: Triptych | undefined = report.data ? triptych(report.data, proposals.data ?? []) : undefined;
  // Proposed stays a dash until plan.proposals answers.
  const counts = t && { held: t.held, failed: t.failed, proposed: proposals.data ? t.proposed : undefined };

  let slip: Slip;
  if (reportId === undefined && pending) {
    slip = { kind: "state", text: conn.summary === "offline" ? "The daemon is not reachable. The report shows here once it answers." : "Finding the newest report…" };
  } else if (reportId === undefined) {
    slip =
      conn.summary === "offline"
        ? { kind: "state", text: "The daemon is not reachable. The report shows here once it answers." }
        : {
            kind: "state",
            text: "No report yet.",
            action: (
              <RouteLink to={pageRoute("reconcile", "report")} className={styles.slipAction} data-role="reconcile-run">
                Run reconcile
              </RouteLink>
            ),
          };
  } else if (report.data) {
    const note = caution(report.data);
    slip = report.errorView
      ? { kind: "state", text: `Showing the last good reply · ${report.errorView.title}` }
      : note === undefined
        ? undefined
        : { kind: "caution", text: note };
  } else if (report.errorView) {
    slip = {
      kind: "state",
      text: report.errorView.title,
      action: (
        <button type="button" className={styles.slipAction} onClick={report.reload}>
          Retry
        </button>
      ),
    };
  } else {
    slip = { kind: "state", text: "Reading the report…" };
  }

  return (
    <PosterFrame
      className={styles.root}
      role="reconcile-layer1"
      measure
      credit="Between streams · "
      cta={{ to: pageRoute("reconcile", "plan"), label: "Choose the next plan", role: "reconcile-cta" }}
    >
      <div className={styles.hero}>
        <div className={styles.sun} aria-hidden="true" data-role="reconcile-sun" />
        <div className={styles.tri} data-role="reconcile-triptych">
          <Column kind="held" label="Held" n={counts?.held} to={pageRoute("reconcile", "report")} title="What held: the report, worked and failed, with evidence" />
          <Column kind="failed" label="Failed" n={counts?.failed} to={pageRoute("reconcile", "report", "failed")} title="What failed: the failures, with line references" />
          <Column kind="proposed" label="Proposed" n={counts?.proposed} to={pageRoute("reconcile", "plan")} title="What is proposed: plan options, directives, the Lord’s chat" />
        </div>
        {slip && (
          <p className={slip.kind === "caution" ? styles.caution : styles.stateSlip} data-role="reconcile-caution" role={slip.kind === "state" ? "status" : undefined}>
            {slip.text}
            {slip.action}
          </p>
        )}
      </div>
    </PosterFrame>
  );
}

const KIND_CLASS = { held: styles.held, failed: styles.failed, proposed: styles.prop } as const;
const BAR_CLASS = { held: styles.barsG, failed: styles.barsE, proposed: styles.barsB } as const;

function Column(props: { kind: "held" | "failed" | "proposed"; label: string; n: number | undefined; to: Route; title: string }) {
  const { kind, n } = props;
  const bars = n === undefined ? 0 : barCount(n);
  return (
    <RouteLink
      to={props.to}
      className={`${styles.rc} ${KIND_CLASS[kind] ?? ""}`}
      title={props.title}
      aria-label={`${n === undefined ? "No" : n.toLocaleString("en-US")} ${props.label.toLowerCase()} · ${props.title}`}
      data-role={`reconcile-${kind}`}
    >
      {n === undefined ? <span className={styles.n}>–</span> : <CountUp value={n} run className={styles.n} />}
      <span className={styles.w}>{props.label}</span>
      <span className={`${styles.bars} ${BAR_CLASS[kind] ?? ""}`} aria-hidden="true" data-role={`reconcile-${kind}-bars`}>
        {Array.from({ length: bars }, (_, i) => (
          <i key={i} style={{ animationDelay: `${i * 55}ms` }} />
        ))}
      </span>
    </RouteLink>
  );
}
