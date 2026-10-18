// The segment cards (the winner's rundown `seg-card`): states, allowed ops, commands, handover
// and its clock, from `segments.tables`. Descoped segments are shown as descoped, not retired.
import { useRpc } from "../../data";
import { QueryView } from "../../kit";
import { RouteLink, pageRoute } from "../../router";
import { Chip } from "../../ui";
import styles from "./Layer2.module.css";
import { handoverText } from "./beats";

export function SegmentCards() {
  const q = useRpc("segments.tables", {});
  return (
    <QueryView query={q} isEmpty={(d) => d.segments.length === 0} emptyTitle="No segment tables" emptyHint="The daemon found no segment tables.">
      {(d) => (
        <div className={styles.cards} data-role="onair-segment-cards">
          {d.segments.map((s) => {
            const off = s.status === "descoped" || s.status === "retired";
            return (
              <article key={s.id} className={styles.card} data-off={off || undefined}>
                <header>
                  <span className={styles.sid}>{s.id}</span>
                  <h3>{s.title}</h3>
                  <Chip tone={off ? "fail" : "ok"}>{s.status}</Chip>
                </header>
                {off ? (
                  <p className={styles.dim}>The runtime refuses to start it. Its design notes are kept so it can be revisited.</p>
                ) : (
                  <>
                    <div className={styles.chain}>
                      {s.states.map((x, i) => (
                        <span key={x} data-init={x === s.initial || undefined}>
                          {i > 0 && <em aria-hidden="true" />}
                          {x.replace(/_/g, " ")}
                        </span>
                      ))}
                    </div>
                    <dl className={styles.kv}>
                      <dt>Allowed ops</dt>
                      <dd>{s.allowed_ops.length}</dd>
                      <dt>Chat commands</dt>
                      <dd>{s.commands.length === 0 ? "none" : s.commands.map((c) => <code key={c}>!{c} </code>)}</dd>
                      <dt>Hands over to</dt>
                      <dd>{s.switch_to.length === 0 ? "nothing" : s.switch_to.join(", ")}</dd>
                      {s.clock.handover && (
                        <>
                          <dt>Handover clock</dt>
                          <dd>
                            {handoverText(s.clock.handover)} <em className={styles.asm}>assumption</em>
                          </dd>
                        </>
                      )}
                    </dl>
                  </>
                )}
                <RouteLink to={pageRoute("segments", "tables", s.id)} className={styles.more}>
                  Open the segment table
                </RouteLink>
              </article>
            );
          })}
        </div>
      )}
    </QueryView>
  );
}
