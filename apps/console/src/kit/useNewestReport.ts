// The report a page shows: the one it was given (route or session), else the newest the
// daemon stores (`reconcile.list`, newest first). Reconcile and Segments held copies.
import { useRpc } from "../data";

export interface ReportOnShow {
  /** The report on show; undefined while the list loads or when the daemon holds none. */
  readonly reportId: string | undefined;
  /** The newest report is still being looked up. */
  readonly pending: boolean;
  /** True when `reportId` is the newest stored report rather than the one given. */
  readonly newest: boolean;
}

/** `chosen` when given (the list is then not fetched), else the newest stored report. */
export function useNewestReport(chosen: string | undefined): ReportOnShow {
  const list = useRpc("reconcile.list", chosen === undefined ? {} : null);
  if (chosen !== undefined) return { reportId: chosen, pending: false, newest: false };
  return {
    reportId: list.data?.[0]?.report_id,
    pending: list.data === undefined && list.errorView === undefined,
    newest: true,
  };
}
