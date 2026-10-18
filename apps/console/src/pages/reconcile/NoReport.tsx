// The designed empty state when there is no report on show: run the reconciler over a
// tournament (`reconcile.run`), or open a report the daemon already holds by its id.
import { useState, type FormEvent } from "react";
import { LLM_TIMEOUT_MS, useMutation, useRpc, viewOfError } from "../../data";
import { ConnectionState } from "../../kit";
import styles from "./Layer2.module.css";
import { session } from "./session";

export function NoReport() {
  const tournaments = useRpc("tournament.list", {});
  const run = useMutation("reconcile.run", { timeoutMs: LLM_TIMEOUT_MS });
  const [picked, setPicked] = useState<string>("");
  const [openId, setOpenId] = useState("");
  const [error, setError] = useState<string | undefined>();

  const list = tournaments.data ?? [];
  // Newest last in the daemon's listing; default to it until the operator picks.
  const source = picked || list[list.length - 1]?.tournament_id || "";

  async function onRun(e: FormEvent) {
    e.preventDefault();
    if (!source) return;
    setError(undefined);
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
            <select value={source} onChange={(e) => setPicked(e.target.value)} disabled={run.pending || list.length === 0}>
              {list.length === 0 && <option value="">{tournaments.errorView ? tournaments.errorView.title : "No tournaments yet"}</option>}
              {list.map((t) => (
                <option key={t.tournament_id} value={t.tournament_id}>
                  {t.tournament_id} ({t.completed}/{t.runs})
                </option>
              ))}
            </select>
          </label>
          <button type="submit" className={styles.primary} disabled={run.pending || !source} data-role="reconcile-run">
            {run.pending ? "Reconciling…" : "Run reconcile"}
          </button>
        </form>
        {run.pending && (
          <p className={styles.pending} role="status">
            The reconciler is reading {source}. Keep this page open; the report opens here when it is written.
          </p>
        )}
        {error && (
          <p className={styles.formError} role="alert">
            {error}
          </p>
        )}
        <form className={styles.form} onSubmit={onOpen}>
          <label className={`${styles.field} ${styles.grow}`}>
            <span>Or open a report by id</span>
            <input type="text" value={openId} onChange={(e) => setOpenId(e.target.value)} placeholder="rec-1792284840627" spellCheck={false} />
          </label>
          <button type="submit" className={styles.secondary} disabled={openId.trim().length === 0}>
            Open
          </button>
        </form>
      </section>
    </ConnectionState>
  );
}
