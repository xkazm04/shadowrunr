// Data hooks for Reconcile & Plan, over the shared data layer.
import { useRpc } from "../../data";
import { useSession } from "./session";

export interface ReportOnShow {
  /** The report on show; undefined while the list loads or when the daemon holds none. */
  readonly reportId: string | undefined;
  /** The newest report is still being looked up. */
  readonly pending: boolean;
}

/**
 * The report on show: the one this session ran or opened, else the newest the daemon stores
 * (`reconcile.list`, newest first).
 */
export function useReportId(): ReportOnShow {
  const chosen = useSession().reportId;
  const list = useRpc("reconcile.list", chosen === undefined ? {} : null);
  if (chosen !== undefined) return { reportId: chosen, pending: false };
  return { reportId: list.data?.[0]?.report_id, pending: list.data === undefined && list.errorView === undefined };
}

/** The report and its plan options, both keyed by the report on show (skipped without one). */
export function useReport(reportId: string | undefined) {
  const params = reportId === undefined ? null : { report_id: reportId };
  return { report: useRpc("reconcile.report", params), proposals: useRpc("plan.proposals", params) };
}
