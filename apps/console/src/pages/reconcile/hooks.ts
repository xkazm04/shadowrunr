// Data hooks for Reconcile & Plan, over the shared data layer.
import { useCallback, useState } from "react";
import type { RpcMethod, RpcParams, RpcResult } from "@shadowrunr/contract";
import { useDataLayer, useRpc, type DataError, type Result } from "../../data";
import { useSession } from "./session";

/**
 * The report on show. The contract has no way yet to find the latest report (`reconcile.report`
 * needs an id, and ids are `rec-<ms>`), so today this is the report this session ran or opened.
 * Proposed: `reconcile.list {}` → reports newest first; then this becomes
 * `session.reportId ?? useRpc("reconcile.list", {}).data?.[0]?.report_id`, and nothing else
 * in the page changes.
 */
export function useReportId(): string | undefined {
  return useSession().reportId;
}

/** The report and its plan options, both keyed by the report on show (skipped without one). */
export function useReport(reportId: string | undefined) {
  const params = reportId === undefined ? null : { report_id: reportId };
  return { report: useRpc("reconcile.report", params), proposals: useRpc("plan.proposals", params) };
}

/**
 * `reconcile.run`, `plan.choose` and `lord.chat` run the reconciler (opus, 600 s in the routing
 * table), so the data layer's 15 s default would cut them off. Proposed: `useMutation` takes
 * `CallOptions`; until then this is the same typed wrapper with a timeout.
 */
export const LLM_TIMEOUT_MS = 11 * 60_000;

export function useLongMutation<M extends RpcMethod>(method: M, timeoutMs = LLM_TIMEOUT_MS) {
  const layer = useDataLayer();
  const [pending, setPending] = useState(false);
  const run = useCallback(
    async (params: RpcParams<M>): Promise<Result<RpcResult<M>, DataError>> => {
      setPending(true);
      try {
        const r = await layer.mutate(method, params, { timeoutMs });
        // The door validated the reply against this method's schema (as in useMutation).
        return r.ok ? { ok: true, value: r.value as RpcResult<M> } : r;
      } finally {
        setPending(false);
      }
    },
    [layer, method, timeoutMs],
  );
  return { run, pending };
}
