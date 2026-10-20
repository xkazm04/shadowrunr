// Layer 1 of Session review: the poster face. Two display numerals over a hotspot light (how
// many sessions, with the line mix across all of them; how many lines carry your marks, by
// mark), an order control, then the sessions as a lit list: one row per sandbox session with its
// verdict, gate squares, mix bar and facts, the hovered row lit and the rest knocked back. Each
// row opens the session's timeline.
import { useMemo, useState } from "react";
import type { LineFeedback, ReviewSessionRow, SandboxCheckState } from "@shadowrunr/contract";
import { useRpc } from "../../data";
import { PosterFrame } from "../../kit/PosterFrame";
import { QueryView } from "../../kit/QueryView";
import { pageRoute, RouteLink } from "../../router";
import { State } from "../../ui/Chip";
import { PillGroup } from "../../ui/PillGroup";
import { VirtualList } from "../../ui/VirtualList";
import { useSessions } from "./hooks";
import styles from "./Layer1.module.css";
import { MixBar, shownMix, type Mix } from "./MixBar";
import { CHECK_NAME, CHECK_STATE_NAME, FEEDBACK, human, sessionTitle, SILENCE_LIMIT_S, WALL_ORDER, type ShownKind, type WallOrder } from "./names";

const NONE = {} as const;
const CHECK_CLASS: Readonly<Record<SandboxCheckState, string | undefined>> = { pass: styles.ckPass, fail: styles.ckFail, unmeasured: styles.ckUnmeasured };
const ORDER_OPTIONS = WALL_ORDER.map(([id, label]) => ({ id, label }));

export function Layer1({ active }: { readonly active: boolean }) {
  if (!active) return null;
  return <Wall />;
}

function Wall() {
  const sessions = useSessions();
  const feedback = useRpc("review.feedback.list", NONE);
  const newest = sessions.data?.[0];
  return (
    <PosterFrame
      className={styles.root}
      role="review-layer1"
      heading="Session review: every sandbox session"
      measure
      credit="After the stream · "
      cta={{
        to: newest ? pageRoute("review", newest.session_id) : pageRoute("review"),
        label: newest ? "Review the newest session" : "Waiting for a session",
        role: "review-cta",
      }}
    >
      <div className={styles.hero}>
        <div className={styles.light} aria-hidden="true" data-role="review-light" />
        <QueryView
          query={sessions}
          isEmpty={(rows) => rows.length === 0}
          emptyTitle="No sandbox sessions yet"
          emptyHint={
            <>
              Run <code>shadowrunr stream sandbox --minutes 5</code>, then come back: every session shows here once it ends.
            </>
          }
        >
          {(rows) => <WallBody rows={rows} feedback={feedback.data} />}
        </QueryView>
      </div>
    </PosterFrame>
  );
}

const roastShare = (r: ReviewSessionRow): number => (r.lines > 0 ? (r.mix["roast"] ?? 0) / r.lines : 0);

export function sortRows(rows: readonly ReviewSessionRow[], order: WallOrder): ReviewSessionRow[] {
  const byTime = (a: ReviewSessionRow, b: ReviewSessionRow) => a.started_ms - b.started_ms;
  return [...rows].sort((a, b) =>
    order === "fails" ? (b.rule_fails ?? -1) - (a.rule_fails ?? -1) || byTime(a, b) : order === "roast" ? roastShare(b) - roastShare(a) || byTime(a, b) : byTime(a, b),
  );
}

function WallBody({ rows, feedback }: { rows: readonly ReviewSessionRow[]; feedback: readonly LineFeedback[] | undefined }) {
  const [order, setOrder] = useState<WallOrder>("oldest");
  const sorted = useMemo(() => sortRows(rows, order), [rows, order]);
  const all = useMemo(() => {
    const m: Partial<Record<ShownKind, number>> = {};
    for (const r of rows) for (const [k, n] of Object.entries(shownMix(r.mix, r.lines))) m[k as ShownKind] = (m[k as ShownKind] ?? 0) + (n ?? 0);
    return m as Mix;
  }, [rows]);
  const lines = rows.reduce((n, r) => n + r.lines, 0);
  return (
    <>
      <div className={styles.figs}>
        <section className={styles.fig} data-role="review-wall-all">
          <b className={styles.n}>{rows.length.toLocaleString("en-US")}</b>
          <span className={styles.w}>{rows.length === 1 ? "Session" : "Sessions"}</span>
          <p className={styles.cap}>{lines.toLocaleString("en-US")} lines from Malakor</p>
          <MixBar mix={all} big />
        </section>
        <YourFeedback rows={rows} feedback={feedback} />
      </div>
      <div className={styles.sortbar}>
        <span className={styles.kicker}>Order</span>
        <PillGroup options={ORDER_OPTIONS} value={order} onChange={setOrder} label="Order" />
      </div>
      <div className={styles.listBox}>
        <VirtualList
          count={sorted.length}
          estimateRem={7}
          overscan={2}
          label="Sandbox sessions"
          role="review-wall"
          getKey={(i) => sorted[i]?.session_id ?? i}
          renderRow={(i) => {
            const r = sorted[i];
            return r ? <SessionRow r={r} /> : null;
          }}
        />
      </div>
    </>
  );
}

function YourFeedback({ rows, feedback }: { rows: readonly ReviewSessionRow[]; feedback: readonly LineFeedback[] | undefined }) {
  const marked = feedback ? feedback.length : rows.reduce((n, r) => n + r.marked, 0);
  const totals = FEEDBACK.map(([id, name]) => [name, feedback?.filter((f) => f.tags.includes(id)).length ?? 0] as const)
    .filter(([, n]) => n > 0)
    .sort((a, b) => b[1] - a[1]);
  return (
    <section className={styles.fig} data-role="review-wall-feedback">
      <b className={styles.n}>{marked.toLocaleString("en-US")}</b>
      <span className={styles.w}>{marked === 1 ? "Line marked" : "Lines marked"}</span>
      {totals.length > 0 ? (
        <ul className={styles.totals}>
          {totals.map(([name, n]) => (
            <li key={name}>
              {name} <b>{n}</b>
            </li>
          ))}
        </ul>
      ) : (
        marked === 0 && <p className={styles.cap}>Nothing marked yet. Open a session and review a type, or pick single moments on its timeline.</p>
      )}
    </section>
  );
}

function SessionRow({ r }: { r: ReviewSessionRow }) {
  const silenceBad = r.max_say_gap_s > SILENCE_LIMIT_S;
  return (
    <RouteLink to={pageRoute("review", r.session_id)} className={styles.row} data-role="review-wall-row">
      <div className={styles.who}>
        <h3 className={styles.h3}>{sessionTitle(r.minutes, r.chaos)}</h3>
        <div className={styles.headline}>
          <State value={r.pass ? "pass" : "fail"} label={r.pass ? "passed" : "failed"} />
          <span className={styles.id}>{r.session_id}</span>
        </div>
        <ul className={styles.checks} aria-label="Gate checks">
          {r.checks.map((c) => (
            <li key={c.id} className={CHECK_CLASS[c.state]} data-state={c.state} title={`${CHECK_NAME[c.id] ?? human(c.id)}: ${CHECK_STATE_NAME[c.state]}. ${c.detail}`} />
          ))}
        </ul>
      </div>
      <div className={styles.mixCell}>
        <MixBar mix={shownMix(r.mix, r.lines)} top={6} />
        {!r.tagged && <p className={styles.hint}>Not tagged yet: line types show once the tagger has run.</p>}
      </div>
      <dl className={styles.facts}>
        <dt>Lines</dt>
        <dd>{r.lines}</dd>
        <dt>Rule fails</dt>
        <dd className={r.rule_fails ? styles.bad : undefined}>{r.rule_fails ?? "–"}</dd>
        <dt>Longest silence</dt>
        <dd className={silenceBad ? styles.bad : undefined} data-role="review-wall-silence">
          {r.max_say_gap_s.toFixed(1)} s
        </dd>
        <dt>You marked</dt>
        <dd>
          {r.marked} of {r.lines}
        </dd>
      </dl>
    </RouteLink>
  );
}
