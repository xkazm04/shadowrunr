// `reconcile/plan`: the reconciler proposes, the operator chooses. Options with their evidence
// and diffs; the composer seals the choice with `plan.choose`, and shows the daemon's live plan
// (`plan.current`: the Lord's dispatch, the running order, topics, the directives riding along).
// What the composer says and offers is derived in planState.ts.
import type { NextStreamPlan, PlanOption } from "@shadowrunr/contract";
import { useState } from "react";
import { LLM_TIMEOUT_MS, useMutation, useRpc, viewOfError } from "../../data";
import { QueryView } from "../../kit";
import { Chip } from "../../ui/Chip";
import { Head } from "../../ui/Section";
import { DiffView } from "./DiffView";
import { useReport } from "./hooks";
import styles from "./Layer2.module.css";
import { evidenceRefs, OPTION_KIND_LABEL, parseDiff } from "./model";
import { canSeal, composerState, draftOf, liveLine, liveRead, sealLabel, type ComposerState } from "./planState";
import { RefLinks } from "./Refs";
import { session, useSession } from "./session";
import { Button } from "../../ui/Button";

export function PlanPage({ reportId }: { reportId: string }) {
  const { proposals } = useReport(reportId);
  return (
    <>
      <Head
        kicker="Reconcile & plan · layer 2"
        title="Plan options"
        lede="The reconciler proposes; you choose. A diff may touch the bible, a tone or a segment table. It never touches the constitution."
      />
      <QueryView query={proposals} isEmpty={(p) => p.length === 0} emptyTitle="This report proposes nothing" emptyHint="Run reconcile again after the next tournament.">
        {(options) => <PlanGrid reportId={reportId} options={options} />}
      </QueryView>
    </>
  );
}

const blocked = (o: PlanOption) => o.diff !== undefined && parseDiff(o.diff).touchesConstitution;

function PlanGrid({ reportId, options }: { reportId: string; options: readonly PlanOption[] }) {
  const s = useSession();
  const current = useRpc("plan.current", {});
  const live = liveRead(current);
  const draft = draftOf(live, reportId, s.chosen[reportId]);
  const chosen = new Set(draft);
  const picked = options.filter((o) => chosen.has(o.option_id) && !blocked(o));
  const state = composerState(live, reportId, picked.map((o) => o.option_id));
  return (
    <div className={styles.plangrid} data-rise>
      <div className={styles.opts} data-rise>
        {options.map((o) => (
          <OptionCard key={o.option_id} option={o} on={chosen.has(o.option_id)} onToggle={() => session.toggleOption(reportId, o.option_id, draft)} />
        ))}
      </div>
      <Composer reportId={reportId} chosen={picked} state={state} />
    </div>
  );
}

const KIND_CLASS: Readonly<Record<PlanOption["kind"], string | undefined>> = {
  character_change: styles.kCharacter,
  segment_change: styles.kSegment,
  topic: styles.kTopic,
  experiment: styles.kExperiment,
};

function OptionCard({ option: o, on, onToggle }: { option: PlanOption; on: boolean; onToggle: () => void }) {
  const isBlocked = blocked(o);
  const refs = evidenceRefs(o.evidence);
  return (
    <article className={`${styles.opt} ${on && !isBlocked ? styles.optOn : ""}`} data-role="reconcile-option" data-option={o.option_id}>
      <header>
        <span className={`${styles.kind} ${KIND_CLASS[o.kind] ?? ""}`}>{OPTION_KIND_LABEL[o.kind]}</span>
        <code>{o.option_id}</code>
      </header>
      <h3>{o.title}</h3>
      <p>{o.rationale}</p>
      {refs.length > 0 && (
        <div className={styles.optRefs}>
          <b>Evidence</b> <RefLinks refs={refs} />
        </div>
      )}
      {o.diff !== undefined && o.diff.length > 0 && <DiffView diff={o.diff} />}
      <label className={styles.pick}>
        <input type="checkbox" checked={on && !isBlocked} disabled={isBlocked} onChange={onToggle} />
        <span>{isBlocked ? "Cannot be chosen: touches the constitution" : "Choose for the next stream"}</span>
      </label>
    </article>
  );
}

function Composer({ reportId, chosen, state }: { reportId: string; chosen: readonly PlanOption[]; state: ComposerState }) {
  const directives = useRpc("directive.list", {});
  const choose = useMutation("plan.choose", { timeoutMs: LLM_TIMEOUT_MS });
  const [error, setError] = useState<string | undefined>();
  // The layer owns the call, so a seal started before a tab switch still shows, and still blocks.
  const composing = choose.pending || choose.running.length > 0;
  // A failure that landed while this page was not mounted shows until the next seal.
  const missed = choose.settled?.error;
  const shownError = error ?? (missed && `${viewOfError(missed).title}: ${missed.message}`);
  const active = (directives.data ?? []).filter((d) => d.active).length;
  const line = liveLine(state);

  async function seal() {
    setError(undefined);
    if (!canSeal(state) || composing) return;
    choose.settled?.ack();
    const r = await choose.run({ report_id: reportId, option_ids: chosen.map((o) => o.option_id) });
    // On success plan.choose invalidates plan.current; the sealed block renders from the refetch.
    if (!r.ok) setError(`${viewOfError(r.error).title}: ${r.error.message}`);
  }

  return (
    <aside className={styles.composer} data-role="reconcile-composer">
      <h2>The next-stream plan</h2>
      <p className={styles.dim}>{chosen.length ? `${chosen.length} option${chosen.length === 1 ? "" : "s"} chosen` : "Nothing chosen yet: the plan rides on the directives alone"}</p>
      {chosen.length > 0 && (
        <ul className={styles.plain}>
          {chosen.map((o) => (
            <li key={o.option_id}>
              <code>{o.option_id}</code> {o.title}
            </li>
          ))}
        </ul>
      )}
      {line && (
        <p className={styles.dim} data-role="reconcile-live">
          {line}
        </p>
      )}
      {state.kind === "unknown" && (
        <p className={styles.formError} role="alert" data-role="reconcile-live-unknown">
          Could not read the live plan: {state.reason}. Sealing replaces whatever the daemon holds.
        </p>
      )}
      <h3>Directives riding along</h3>
      <p className={styles.dim}>{directives.data ? `${active} active` : directives.errorView ? directives.errorView.title : "Loading"}</p>
      <Button type="button" variant="primary" className={styles.seal} onClick={() => void seal()} disabled={composing || !canSeal(state)} data-role="reconcile-seal">
        {composing ? "Composing…" : sealLabel(state)}
      </Button>
      {composing && (
        <p className={styles.pending} role="status">
          The planner is writing the Lord’s dispatch and the running order.
        </p>
      )}
      {shownError && (
        <p className={styles.formError} role="alert">
          {shownError}
        </p>
      )}
      {(state.kind === "live-here" || state.kind === "live-elsewhere") && <SealedPlan plan={state.plan} />}
    </aside>
  );
}

function SealedPlan({ plan }: { plan: NextStreamPlan }) {
  return (
    <div className={styles.sealed} data-role="reconcile-sealed">
      <h3>
        Live plan <code>{plan.plan_id}</code>
        {plan.report_id && (
          <small>
            {" "}
            from <code>{plan.report_id}</code>
          </small>
        )}
      </h3>
      <h3>Lord’s dispatch</h3>
      <blockquote className={styles.voice}>{plan.lord_dispatch}</blockquote>
      <h3>Segments</h3>
      <ul className={styles.plain}>
        {plan.segments.map((s, i) => (
          <li key={`${s.segment}-${i}`}>
            <code>{s.segment}</code> {s.minutes} min <small>{s.notes}</small>
          </li>
        ))}
      </ul>
      {plan.topics.length > 0 && (
        <>
          <h3>Topics</h3>
          <ul className={styles.plain}>
            {plan.topics.map((t, i) => (
              <li key={i}>{t}</li>
            ))}
          </ul>
        </>
      )}
      <h3>Directives</h3>
      {plan.directives.length === 0 ? (
        <p className={styles.dim}>None active.</p>
      ) : (
        <ul className={styles.plain}>
          {plan.directives.map((d) => (
            <li key={d.id}>
              <Chip tone={d.kind === "prevent" ? "fail" : "ok"}>{d.kind}</Chip> {d.text}
              {d.scope && <small> {d.scope}</small>}
            </li>
          ))}
        </ul>
      )}
      <p className={styles.dim}>Malakor receives this dispatch at the next stream start.</p>
    </div>
  );
}
