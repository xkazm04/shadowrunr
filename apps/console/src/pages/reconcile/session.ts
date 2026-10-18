// What this page remembers for the length of a session, in memory only (never localStorage):
// the report on show, the options chosen per report, the plan the daemon sealed, and the
// conversation with the Lord. The daemon owns persistence; this store only spares the operator
// from losing a half-made choice when switching tabs. Layer 1 and layer 2 are separate lazy
// chunks rendered by the shell, so they share this module rather than a React provider.
import { useSyncExternalStore } from "react";
import type { Directive, NextStreamPlan } from "@shadowrunr/contract";

export type ChatTurn =
  | { readonly id: number; readonly who: "me"; readonly text: string }
  | { readonly id: number; readonly who: "lord"; readonly text: string; readonly directives: readonly Directive[] }
  | { readonly id: number; readonly who: "error"; readonly text: string };

export interface SessionState {
  /** The report on show; see `useReportId` for how it is resolved. */
  readonly reportId: string | undefined;
  /** Option ids chosen, per report id. */
  readonly chosen: Readonly<Record<string, readonly string[]>>;
  /** The plan `plan.choose` returned, per report id. */
  readonly sealed: Readonly<Record<string, NextStreamPlan>>;
  readonly chat: readonly ChatTurn[];
}

const INITIAL: SessionState = { reportId: undefined, chosen: {}, sealed: {}, chat: [] };

let state: SessionState = INITIAL;
let nextTurn = 1;
const listeners = new Set<() => void>();

function set(next: SessionState) {
  state = next;
  for (const l of listeners) l();
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
}

const get = () => state;

export function useSession(): SessionState {
  return useSyncExternalStore(subscribe, get, get);
}

export const session = {
  get,
  showReport(reportId: string) {
    if (state.reportId !== reportId) set({ ...state, reportId });
  },
  toggleOption(reportId: string, optionId: string) {
    const cur = state.chosen[reportId] ?? [];
    const next = cur.includes(optionId) ? cur.filter((x) => x !== optionId) : [...cur, optionId];
    set({ ...state, chosen: { ...state.chosen, [reportId]: next } });
  },
  seal(reportId: string, plan: NextStreamPlan) {
    set({ ...state, sealed: { ...state.sealed, [reportId]: plan } });
  },
  say(turn: { who: "me"; text: string } | { who: "lord"; text: string; directives: readonly Directive[] } | { who: "error"; text: string }) {
    set({ ...state, chat: [...state.chat, { ...turn, id: nextTurn++ }] });
  },
  /** Tests only: back to a fresh session. */
  reset() {
    nextTurn = 1;
    set(INITIAL);
  },
};
