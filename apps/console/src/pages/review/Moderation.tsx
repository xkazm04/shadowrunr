// Moderation: every moderation record, grouped by what happened (safety & care, persona,
// accuracy, engine cover, chat filters, show mechanics), each kind with how often and when. A
// kind opens its occurrences in the side panel, 20 at a time, and each opens its moment.
import { useState } from "react";
import type { ReviewSession } from "@shadowrunr/contract";
import { tsOf } from "./derive";
import styles from "./Moderation.module.css";
import { fmtClock, MOD_GROUP_NOTE, MOD_GROUPS, modName, type ModGroup, type ModName } from "./names";
import type { TabProps } from "./tab";
import { PAGE, Pager, sayNear, TickStrip } from "./tabkit";
import kit from "./tabkit.module.css";

export interface ModKind extends ModName {
  /** Each occurrence: its transcript line, stream seconds and detail. */
  readonly items: readonly { readonly line: number; readonly t: number; readonly detail: string }[];
}

/** Moderation records by group (in MOD_GROUPS order), each group's kinds most frequent first. */
export function modGroups(rs: ReviewSession): { group: ModGroup; kinds: ModKind[] }[] {
  const kinds = new Map<string, { m: ModName; items: { line: number; t: number; detail: string }[] }>();
  rs.events.forEach((e, i) => {
    if (e.type !== "moderation") return;
    const m = modName(e.stage, e.rule, e.action);
    const k = kinds.get(m.key) ?? { m, items: [] };
    k.items.push({ line: i + 1, t: tsOf(e), detail: m.detail });
    kinds.set(m.key, k);
  });
  const all: ModKind[] = [...kinds.values()].map(({ m, items }) => ({ ...m, items }));
  return MOD_GROUPS.map((group) => ({ group, kinds: all.filter((k) => k.group === group).sort((a, b) => b.items.length - a.items.length) })).filter((g) => g.kinds.length > 0);
}

export function Moderation(props: TabProps) {
  const { rs, d } = props;
  const groups = modGroups(rs);
  const [sel, setSel] = useState<{ key: string; page: number } | null>(null);
  const picked = sel ? groups.flatMap((g) => g.kinds).find((k) => k.key === sel.key) : undefined;
  const open = (line: number) => {
    const l = sayNear(d, line);
    if (l !== undefined) props.openLine(l);
  };
  return (
    <div className={kit.split} data-role="review-moderation">
      <section className={`${kit.panel} ${kit.scroll}`}>
        {groups.length === 0 && <p className={kit.empty}>No moderation records in this session.</p>}
        {groups.map(({ group, kinds }) => (
          <section key={group} className={styles.group} data-role="review-mod-group" data-group={group}>
            <div className={kit.kicker}>{group}</div>
            {MOD_GROUP_NOTE[group] && <p className={kit.lede}>{MOD_GROUP_NOTE[group]}</p>}
            <table className={kit.table}>
              <thead>
                <tr>
                  <th>What happened</th>
                  <th className={kit.num}>Times</th>
                  <th className={styles.when}>When in the session</th>
                </tr>
              </thead>
              <tbody>
                {kinds.map((k) => (
                  <tr key={k.key} className={sel?.key === k.key ? styles.on : undefined}>
                    <td>
                      <button type="button" className={styles.pick} aria-pressed={sel?.key === k.key} onClick={() => setSel({ key: k.key, page: 0 })}>
                        {k.name}
                      </button>
                    </td>
                    <td className={kit.num}>{k.items.length}</td>
                    <td className={styles.when}>
                      <TickStrip ticks={k.items} lengthS={d.lengthS} label={`${k.items.length} times across the session`} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        ))}
      </section>
      <aside className={`${kit.panel} ${styles.side}`} data-role="review-mod-occurrences">
        {picked && sel ? (
          <>
            <div className={kit.kicker}>{picked.group}</div>
            <h2 className={kit.title}>{picked.name}</h2>
            <ol className={styles.list}>
              {picked.items.slice(sel.page * PAGE, (sel.page + 1) * PAGE).map((o) => (
                <li key={o.line}>
                  <button type="button" className={styles.occ} data-line={o.line} onClick={() => open(o.line)}>
                    <span className={kit.mono}>{fmtClock(o.t)}</span>
                    <span>{o.detail}</span>
                    <span className={styles.open}>Open</span>
                  </button>
                </li>
              ))}
            </ol>
            <Pager page={sel.page} total={picked.items.length} onPage={(page) => setSel({ key: picked.key, page })} label="Occurrences" />
          </>
        ) : (
          <>
            <div className={kit.kicker}>Occurrences</div>
            <p className={kit.empty}>Pick a row to list when it happened. Each one opens the moment on the timeline.</p>
          </>
        )}
      </aside>
    </div>
  );
}
