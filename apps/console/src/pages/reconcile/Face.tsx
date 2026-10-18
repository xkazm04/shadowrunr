// The poster face for Reconcile & Plan: the winner's motif (art.js M.reconcile, a sun setting
// behind ruled ground) and the glance chips on the hovered wall poster (app.js glance).
import { useReport, useReportId } from "./hooks";
import styles from "./Face.module.css";
import { triptych } from "./model";

export function Motif() {
  return (
    <svg className={styles.motif} viewBox="0 0 200 400" preserveAspectRatio="xMidYMid slice" aria-hidden="true" data-role="reconcile-motif">
      <circle className={`${styles.o} ${styles.breathe}`} cx="100" cy="270" r="92" />
      <circle className={styles.t} cx="100" cy="270" r="58" opacity=".9" />
      <rect className={styles.f} x="0" y="270" width="200" height="140" />
      <path className={styles.rules} strokeWidth="3" d="M0 282H200M0 300H200M0 322H200M0 350H200M0 384H200" opacity=".6" />
      <g className={styles.rays} strokeWidth="3" opacity=".7">
        <path d="M100 150V110M44 170L22 140M156 170l22-30M16 230H0M184 230h16" />
      </g>
    </svg>
  );
}

export function Glance() {
  const { reportId } = useReportId();
  const { report, proposals } = useReport(reportId);
  if (!report.data) return <b>{reportId === undefined ? "No report yet" : "Report"}</b>;
  const t = triptych(report.data, proposals.data ?? []);
  return (
    <>
      <b>{t.held} held</b>
      <b>{t.failed} failed</b>
      {proposals.data && <b>{t.proposed} proposed</b>}
    </>
  );
}
