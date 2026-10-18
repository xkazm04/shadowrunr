import { createContext, useContext, useState, type ReactNode } from "react";

export type ViewId = "preflight" | "simulations" | "reconcile" | "lord" | "directives";

export const VIEWS: { id: ViewId; label: string; key: string }[] = [
  { id: "preflight", label: "Pre-flight", key: "1" },
  { id: "simulations", label: "Simulations", key: "2" },
  { id: "reconcile", label: "Reconcile & Plan", key: "3" },
  { id: "lord", label: "Lord's chat", key: "4" },
  { id: "directives", label: "Directives", key: "5" },
];

interface DeskState {
  view: ViewId;
  go: (v: ViewId) => void;
  /** Tournament preselected for Reconcile & Plan. */
  reconcileTournament: string | null;
  setReconcileTournament: (t: string | null) => void;
  /** The report the Lord's chat talks about (`lord.chat` report_id). */
  reportId: string | null;
  setReportId: (r: string | null) => void;
}

const Ctx = createContext<DeskState | null>(null);

/** Controlled: the shell's router owns the view (`#/legacy/<view>`). */
export function DeskProvider({ children, view, go }: { children: ReactNode; view: ViewId; go: (v: ViewId) => void }) {
  const [reconcileTournament, setReconcileTournament] = useState<string | null>(null);
  const [reportId, setReportId] = useState<string | null>(null);
  return (
    <Ctx.Provider value={{ view, go, reconcileTournament, setReconcileTournament, reportId, setReportId }}>
      {children}
    </Ctx.Provider>
  );
}

export function useDesk(): DeskState {
  const v = useContext(Ctx);
  if (!v) throw new Error("useDesk outside DeskProvider");
  return v;
}
