// The Lord's dispatch: what the next stream receives. A plan is composed in Reconcile › Plan from
// a report's options plus every active directive (verbatim). This page shows the report it
// answers (the one in the route, else the newest from reconcile.list), its summary and options;
// the standing directives always show.
import type { Directive } from "@shadowrunr/contract";
import { useRpc } from "../../data";
import { QueryView } from "../../kit";
import { RouteLink, pageRoute } from "../../router";
import { Chip, EmptyState, Section } from "../../ui";
import styles from "./Layer2.module.css";

function Directives() {
  const q = useRpc("directive.list", {});
  return (
    <Section title="Standing directives" right={<RouteLink to={pageRoute("reconcile", "directives")}>Manage directives</RouteLink>}>
      <QueryView query={q} isEmpty={(d) => !d.some((x: Directive) => x.active)} emptyTitle="None active" emptyHint="Directives you add in Reconcile go into every plan, verbatim.">
        {(d) => (
          <ul className={styles.plain} data-role="onair-directives">
            {d
              .filter((x) => x.active)
              .map((x) => (
                <li key={x.id}>
                  <Chip tone={x.kind === "prevent" ? "fail" : "ok"}>{x.kind}</Chip> {x.text}
                  {x.scope !== undefined && <small> {x.scope}</small>}
                </li>
              ))}
          </ul>
        )}
      </QueryView>
    </Section>
  );
}

function FromReport({ reportId }: { reportId: string }) {
  const report = useRpc("reconcile.report", { report_id: reportId });
  const options = useRpc("plan.proposals", { report_id: reportId });
  return (
    <>
      <Section title="The report it answers" right={<RouteLink to={pageRoute("reconcile", "report")}>Open the report</RouteLink>}>
        <QueryView query={report}>{(r) => <p className={styles.lede}>{r.summary}</p>}</QueryView>
      </Section>
      <Section title="What the dispatch is composed from" right={<RouteLink to={pageRoute("reconcile", "plan")}>Choose options</RouteLink>}>
        <QueryView query={options} isEmpty={(d) => d.length === 0} emptyTitle="No options in this report" emptyHint="The reconciler proposed nothing for this report.">
          {(d) => (
            <div className={styles.cards} data-role="onair-dispatch-options">
              {d.map((o) => (
                <article key={o.option_id} className={styles.card}>
                  <header>
                    <span className={styles.sid}>{o.kind.replace(/_/g, " ")}</span>
                    <h3>{o.title}</h3>
                  </header>
                  <p className={styles.dim}>{o.rationale}</p>
                  {o.diff !== undefined && <Chip tone="caution">changes the character package</Chip>}
                </article>
              ))}
            </div>
          )}
        </QueryView>
      </Section>
    </>
  );
}

export function Dispatch({ reportId }: { reportId: string | null }) {
  const list = useRpc("reconcile.list", reportId === null ? {} : null);
  const id = reportId ?? list.data?.[0]?.report_id ?? null;
  if (reportId === null && list.data === undefined) {
    return (
      <>
        <Section title="Tonight’s dispatch">
          <QueryView query={list}>{() => null}</QueryView>
        </Section>
        <Directives />
      </>
    );
  }
  return (
    <>
      {id === null ? (
        <Section title="Tonight’s dispatch">
          <EmptyState
            title="No reconcile report yet"
            hint={
              <>
                A dispatch is composed from a reconcile report. Run one in <RouteLink to={pageRoute("reconcile", "report")}>Reconcile</RouteLink>, then choose its options in <RouteLink to={pageRoute("reconcile", "plan")}>Reconcile › Plan</RouteLink>.
              </>
            }
          />
        </Section>
      ) : (
        <FromReport reportId={id} />
      )}
      <Directives />
      <p className={styles.note}>v0.1 has no live broadcast, so nothing is sent: the dispatch is what the next stream would receive.</p>
    </>
  );
}
