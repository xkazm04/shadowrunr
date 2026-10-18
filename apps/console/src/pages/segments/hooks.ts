// Data hooks for Segments & ranks: the compiled-in segment tables and the rank ladder.
import { useRpc } from "../../data";

const NONE = {} as const;

/** `segments.tables`: every table the runtime interprets, plus the op and command vocabularies. */
export const useTables = () => useRpc("segments.tables", NONE);

/** `ranks.table` for the configured character (titles come from its package). */
export const useRanks = () => useRpc("ranks.table", NONE);

/** `reconcile.report` for one report id; skipped without one. */
export const useReport = (reportId: string | undefined) => useRpc("reconcile.report", reportId === undefined ? null : { report_id: reportId });
