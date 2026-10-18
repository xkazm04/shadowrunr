// Data hooks for Reconcile & Plan, over the shared data layer.
import { useRpc } from "../../data";
import { useNewestReport, type ReportOnShow } from "../../kit/useNewestReport";
import { useSession } from "./session";

export type { ReportOnShow };

/**
 * The report on show: the one this session ran or opened, else the newest the daemon stores
 * (`reconcile.list`, newest first).
 */
export function useReportId(): ReportOnShow {
  return useNewestReport(useSession().reportId);
}

/** The report and its plan options, both keyed by the report on show (skipped without one). */
export function useReport(reportId: string | undefined) {
  const params = reportId === undefined ? null : { report_id: reportId };
  return { report: useRpc("reconcile.report", params), proposals: useRpc("plan.proposals", params) };
}
