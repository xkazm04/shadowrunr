// The designed empty state when there is no report on show: run the reconciler over a
// tournament (`reconcile.run`), or open a report the daemon already holds by its id.
import { useState, type FormEvent } from "react";
import { LLM_TIMEOUT_MS, useMutation, useRpc, viewOfError } from "../../data";
import { ConnectionState } from "../../kit";
import styles from "./Layer2.module.css";
import { session } from "./session";
import { Button } from "../../ui/Button";

export function NoReport() {
  const tournaments = useRpc("tournament.list", {});
  const run = useMutation("reconcile.run", { timeoutMs: LLM_TIMEOUT_MS });
  const [picked, setPicked] = useState<string>("");
  const [openId, setOpenId] = useState("");
  const [error, setError] = useState<string | undefined>();

  const list = tournaments.data ?? [];
  // Newest last in the daemon's listing; default to it until the operator picks.
  const source = picked || list[list.length - 1]?.tournament_id || "";
  // The layer owns the call, so a run started before a tab switch still shows (and still blocks).
  const inflight = run.running[0];
  const busy = run.pending || inflight !== undefined;
  const reading = inflight ? (inflight.params as { source: { tournament_id: string } }).source.tournament_id : source;
  // A failure that landed while this page was not mounted shows until the next run.
  const missed = run.settled?.error;
  const shownError = error ?? (missed && `${viewOfError(missed).title}: ${missed.message}`);

  async function onRun(e: FormEvent) {
    e.preventDefault();
    if (!source || busy) return;
    setError(undefined);
    run.settled?.ack();
    const r = await run.run({ source: { tournament_id: source } });
    if (r.ok) session.showReport(r.value.report_id);
    else setError(`${viewOfError(r.error).title}: ${r.error.message}`);
  }

  function onOpen(e: FormEvent) {
    e.preventDefault();
    const id = openId.trim();
    if (id) session.showReport(id);
  }

  return (
    <ConnectionState>
      <section className={styles.empty} data-role="reconcile-empty">
        <span className={styles.emptyKicker}>No report yet</span>
        <h2 className={styles.emptyTitle}>The morning after has not happened</h2>
        <p className={styles.emptyHint}>
          Run the reconciler over a tournament. It reads the transcripts, verdicts and ledgers, then writes what held, what failed and what to try next. It
          can take up to ten minutes.
        </p>
        <form className={styles.form} onSubmit={(e) => void onRun(e)}>
          <label className={styles.field}>
            <span>Source tournament</span>
            <select value={source} onChange={(e) => setPicked(e.target.value)} disabled={busy || list.length === 0}>
              {list.length === 0 && <option value="">{tournaments.errorView ? tournaments.errorView.title : "No tournaments yet"}</option>}
              {list.map((t) => (
                <option key={t.tournament_id} value={t.tournament_id}>
                  {t.tournament_id} ({t.completed}/{t.runs})
                </option>
              ))}
            </select>
          </label>
          <Button type="submit" variant="primary" disabled={busy || !source} data-role="reconcile-run">
            {busy ? "Reconciling…" : "Run reconcile"}
          </Button>
        </form>
        {busy && (
          <p className={styles.pending} role="status">
            The reconciler is reading {reading}
            {inflight && `, started ${new Date(inflight.startedAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`}. The report opens here when it is written.
          </p>
        )}
        {shownError && (
          <p className={styles.formError} role="alert">
            {shownError}
          </p>
        )}
        <form className={styles.form} onSubmit={onOpen}>
          <label className={`${styles.field} ${styles.grow}`}>
            <span>Or open a report by id</span>
            <input type="text" value={openId} onChange={(e) => setOpenId(e.target.value)} placeholder="rec-1792284840627" spellCheck={false} />
          </label>
          <Button type="submit" variant="secondary" disabled={openId.trim().length === 0}>
            Open
          </Button>
        </form>
      </section>
    </ConnectionState>
  );
}
