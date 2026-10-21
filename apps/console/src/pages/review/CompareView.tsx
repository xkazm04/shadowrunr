// Compare two sandbox sessions (`review/<A>/compare/<B>`), the view over compare.ts (a separate name: the
// two would collide on a case-insensitive disk). A before the change, B after. What got
// fixed, what broke and what moved: the judge's rules, the gate checks that flipped, reply wait and
// silence against the page's limits, the line mix and the quality the judge sampled. A rule's line
// opens that session's timeline.
import type { ReviewSession } from "@shadowrunr/contract";
import { QueryView } from "../../kit/QueryView";
import { pageRoute, RouteLink, useRouter } from "../../router";
import { buttonClass } from "../../ui/Button";
import { Chip, ChipRow } from "../../ui/Chip";
import { Head, Section } from "../../ui/Section";
import { EmptyState } from "../../ui/States";
import { checkDiff, dimDiff, figureDiff, mixDiff, ruleDiff, type RuleRow } from "./compare";
import styles from "./Compare.module.css";
import type { Derived } from "./derive";
import { useDerived, useSession } from "./hooks";
import l2 from "./Layer2.module.css";
import { sessionTitle } from "./names";
import { pct, sayNear } from "./tabkit";
import kit from "./tabkit.module.css";

const share = (x: number) => `${Math.round(x * 100)}%`;
const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? "" : "s"}`;

export function Compare({ a, b }: { a: string; b: string }) {
  const qa = useSession(a);
  const qb = useSession(b);
  return <QueryView query={qa}>{(ra) => <QueryView query={qb}>{(rb) => <Pair a={ra} b={rb} />}</QueryView>}</QueryView>;
}

function Side({ rs, side }: { rs: ReviewSession; side: "A" | "B" }) {
  const id = rs.row.session_id;
  return (
    <div className={styles.side} data-role="review-compare-side">
      <b className={kit.sub}>{side === "A" ? "A · before" : "B · after"}</b>
      <b className={styles.title}>{sessionTitle(rs.row.minutes, rs.row.chaos)}</b>
      <span className={rs.row.pass ? l2.pass : l2.fail}>{rs.row.pass ? "Passed" : "Failed"}</span>
      <RouteLink to={pageRoute("review", id)} className={l2.id}>
        {id}
      </RouteLink>
    </div>
  );
}

function Pair({ a, b }: { a: ReviewSession; b: ReviewSession }) {
  const router = useRouter();
  const da = useDerived(a);
  const db = useDerived(b);
  const rules = ruleDiff(a, b);
  const checks = checkDiff(a.row.checks, b.row.checks);
  const figures = figureDiff({ ...optP95(da), silence: a.row.max_say_gap_s }, { ...optP95(db), silence: b.row.max_say_gap_s });
  const mix = mixDiff({ mix: da.mix, lines: da.says.length }, { mix: db.mix, lines: db.says.length });
  const dims = dimDiff(a, b);
  const derived = new Map<string, Derived>([
    [a.row.session_id, da],
    [b.row.session_id, db],
  ]);
  const open = (session: string, line: number) => {
    const d = derived.get(session);
    const l = d ? sayNear(d, line) : undefined;
    router.navigate(pageRoute("review", session, "timeline", String(l ?? line)));
  };
  const group = (key: string, title: string, rows: readonly RuleRow[]) =>
    rows.length > 0 && (
      <div data-role={`review-compare-${key}`}>
        <h3 className={kit.sub}>{title}</h3>
        <div className={kit.stack}>
          {rows.map((r) => (
            <div key={r.rule} className={styles.rule}>
              <b>{r.name}</b>
              {r.refs.map((x) => (
                <button key={`${x.session}#${x.line}`} type="button" className={buttonClass("secondary", "compact")} data-line={x.line} onClick={() => open(x.session, x.line)}>
                  {x.session === a.row.session_id ? "A" : "B"} · line {x.line}
                </button>
              ))}
            </div>
          ))}
        </div>
      </div>
    );
  const unjudged = [!rules.aJudged && "A", !rules.bJudged && "B"].filter((s): s is string => Boolean(s));
  return (
    <>
      <Head kicker="Session review · Compare" title="What got fixed, what broke, what moved" lede="Session A before the change, session B after it. A line opens that session's timeline." />
      <div className={styles.sides}>
        <Side rs={a} side="A" />
        <Side rs={b} side="B" />
      </div>
      <Section title="The judge’s rules" right={unjudged.length ? "Not comparable" : `${rules.newly.length} newly failing · ${rules.fixed.length} fixed`}>
        {unjudged.length > 0 ? (
          <EmptyState title={`${unjudged.join(" and ")} ${unjudged.length > 1 ? "were" : "was"} not judged`} hint="A session without a verdict is unmeasured, not passed: nothing counts as fixed or newly failing against it." />
        ) : rules.newly.length + rules.fixed.length + rules.still.length + rules.unchecked.length === 0 ? (
          <p className={kit.note}>Neither session failed a rule.</p>
        ) : (
          <div className={kit.stack}>
            {group("newly", "Newly failing", rules.newly)}
            {group("fixed", "Fixed", rules.fixed)}
            {group("still", "Still failing", rules.still)}
            {group("unchecked", "Not checked in B", rules.unchecked)}
          </div>
        )}
      </Section>
      <Section title="Gate checks" right={`${plural(checks.unchanged, "check")} unchanged`}>
        {checks.changed.length === 0 ? (
          <p className={kit.note}>No gate check changed.</p>
        ) : (
          <ChipRow>
            {checks.changed.map((c) => (
              <Chip key={c.id} tone={c.tone}>
                {c.label}
              </Chip>
            ))}
          </ChipRow>
        )}
      </Section>
      <Section title="Figures">
        <table className={kit.table}>
          <thead>
            <tr>
              <th>Figure</th>
              <th className={kit.num}>A</th>
              <th className={kit.num}>B</th>
              <th className={kit.num}>Change</th>
            </tr>
          </thead>
          <tbody>
            {figures.map((f) => (
              <tr key={f.id} data-role="review-compare-figure" data-tone={f.tone}>
                <td title={`Fails over ${f.limit} s`}>{f.label}</td>
                <td className={kit.num}>{f.a}</td>
                <td className={kit.num}>{f.b}</td>
                <td className={f.tone === "fail" ? `${kit.num} ${kit.bad}` : kit.num}>{f.delta}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Section>
      <Section title="Line mix" right={mix.unchanged ? `${mix.unchanged} ${mix.unchanged === 1 ? "kind held its" : "kinds held their"} share` : undefined}>
        {mix.rows.length === 0 ? (
          <p className={kit.note}>Every kind held its share of his lines.</p>
        ) : (
          <table className={kit.table}>
            <thead>
              <tr>
                <th>Kind</th>
                <th className={kit.num}>A</th>
                <th className={kit.num}>B</th>
              </tr>
            </thead>
            <tbody>
              {mix.rows.map((m) => (
                <tr key={m.kind} data-role="review-compare-mix">
                  <td>{m.label}</td>
                  <td className={kit.num}>{share(m.a)}</td>
                  <td className={kit.num}>{share(m.b)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Section>
      <Section title="Quality on sampled lines">
        {dims.length === 0 ? (
          <p className={kit.note}>Neither judge recorded quality facts.</p>
        ) : (
          <table className={kit.table}>
            <thead>
              <tr>
                <th>Dimension, most moved first</th>
                <th className={kit.num}>A</th>
                <th className={kit.num}>B</th>
              </tr>
            </thead>
            <tbody>
              {dims.map((r) => (
                <tr key={r.dim} data-role="review-compare-dim">
                  <td>{r.name}</td>
                  <td className={kit.num}>{r.a ? `${r.a.yes}/${r.a.total}` : "–"}</td>
                  <td className={kit.num}>{r.b ? `${r.b.yes}/${r.b.total}` : "–"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Section>
      <RouteLink to={pageRoute("review", b.row.session_id)} className={buttonClass("secondary")}>
        Back to session B
      </RouteLink>
    </>
  );
}

function optP95(d: Derived): { replyP95?: number } {
  const p = pct(d.replyWaitS, 0.95);
  return p === undefined ? {} : { replyP95: p };
}
