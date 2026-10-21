// brain/preflight: the PF-01..PF-09 board over `preflight.run`, the health check that gates
// every stream start. Two layers: a row per check (id, title, state), its detail opened in
// place. Re-run is the button or the `rerun` key command (the keymap resolves the chord).
import { useState } from "react";
import type { HealthReport, PreflightRunResult } from "@shadowrunr/contract";
import { QueryView } from "../../kit";
import { timeOf } from "../../kit/freshness";
import { useKeyCommand } from "../../kit/keyCommands";
import { useRpc } from "../../data";
import { Button } from "../../ui/Button";
import { Chip, type Tone } from "../../ui/Chip";
import { PREFLIGHT_PARAMS, rowState, tally, titleOf, verdictOf, worstState, type RowState } from "./preflightModel";
import styles from "./Preflight.module.css";

const TONE: Readonly<Record<RowState, Tone>> = { green: "ok", amber: "caution", red: "fail", expected: "muted" };

function Row({ report, open, onToggle }: { report: HealthReport; open: boolean; onToggle: () => void }) {
  const state = rowState(report);
  const id = `pf-detail-${report.check}`;
  return (
    <li className={styles.row} data-role="brain-preflight-row" data-check={report.check} data-state={state}>
      <button type="button" className={styles.head} aria-expanded={open} aria-controls={id} onClick={onToggle}>
        <span className={styles.lamp} data-state={state} aria-hidden="true" />
        <span className={styles.id}>{report.check}</span>
        <span className={styles.title}>{titleOf(report.check)}</span>
        <Chip tone={TONE[state]}>{state === "expected" ? "amber · expected" : state}</Chip>
      </button>
      {open && (
        <p id={id} className={styles.detail} data-role="brain-preflight-detail">
          {report.detail || "No detail reported."}
          {state === "expected" && <span className={styles.note}> Expected in the media-less phase: not a fault.</span>}
        </p>
      )}
    </li>
  );
}

function Board({ reports }: { reports: PreflightRunResult }) {
  const [open, setOpen] = useState<ReadonlySet<string>>(() => new Set());
  const toggle = (check: string) =>
    setOpen((s) => {
      const next = new Set(s);
      if (!next.delete(check)) next.add(check);
      return next;
    });
  const t = tally(reports);
  return (
    <div data-role="brain-preflight" data-worst={worstState(reports)}>
      <p className={styles.verdict} data-role="brain-preflight-verdict">
        {verdictOf(reports)}
      </p>
      <p className={styles.tally}>
        {t.green} green · {t.amber} amber · {t.red} red · {t.expected} expected
      </p>
      <ol className={styles.board} data-role="brain-preflight-board">
        {reports.map((r) => (
          <Row key={r.check} report={r} open={open.has(r.check)} onToggle={() => toggle(r.check)} />
        ))}
      </ol>
    </div>
  );
}

export function Preflight() {
  const query = useRpc("preflight.run", PREFLIGHT_PARAMS);
  // The time of the last good run, as the cache stamped it.
  const at = query.updatedAt;
  const rerun = query.reload;
  useKeyCommand("rerun", rerun);
  return (
    <>
      <div className={styles.bar}>
        <span className={styles.last} data-role="brain-preflight-last">
          {at === undefined ? "Not run yet" : `Last run ${timeOf(at)}`}
        </span>
        <Button variant="ink" size="compact" onClick={rerun} disabled={query.refreshing} data-role="brain-preflight-rerun">
          {query.refreshing ? "Running…" : "Re-run"}
        </Button>
      </div>
      <QueryView<PreflightRunResult>
        query={query}
        isEmpty={(d) => d.length === 0}
        emptyTitle="The daemon reported no pre-flight checks"
        emptyHint="A daemon of this version reports PF-01 to PF-08; re-run once it has loaded its config."
      >
        {(reports) => <Board reports={reports} />}
      </QueryView>
    </>
  );
}
