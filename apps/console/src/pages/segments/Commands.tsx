// segments/commands: the chat commands (who may use them, where) and the op grid: every op tag
// of the action grammar against every runnable segment (l2b.js `commands`).
import type { RankTableInfo, SegmentTables } from "@shadowrunr/contract";
import { QueryView } from "../../kit";
import { pageRoute, RouteLink } from "../../router";
import { Chip, Head, Section } from "../../ui";
import { useRanks, useTables } from "./hooks";
import styles from "./Layer2.module.css";
import { commandRows, opGrid } from "./model";

export function CommandsPage() {
  const tables = useTables();
  const ranks = useRanks();
  return (
    <>
      <Head
        kicker="Segments & ranks · layer 2"
        title="Chat commands"
        lede="Chat commands let minions steer the night. Anything the model emits outside a segment’s allowed ops is rejected and logged as a moderation event."
      />
      <QueryView query={tables} isEmpty={(t) => t.command_names.length === 0 && t.op_tags.length === 0} emptyTitle="The daemon lists no commands or ops">
        {(t) => (
          <>
            <Commands tables={t} ranks={ranks.data} />
            <OpGrid tables={t} />
          </>
        )}
      </QueryView>
    </>
  );
}

function rankTitle(ranks: RankTableInfo | undefined, rank: string): string {
  return ranks?.ranks.find((r) => r.rank === rank)?.title ?? rank.replace(/_/g, " ");
}

function Commands({ tables, ranks }: { tables: SegmentTables; ranks: RankTableInfo | undefined }) {
  const rows = commandRows(tables, ranks?.propose_min_rank);
  return (
    <Section title="Commands" right={`${rows.length} commands`}>
      <div className={styles.tblwrap}>
        <table className={styles.tbl} data-role="segments-command-table">
          <thead>
            <tr>
              <th>Command</th>
              <th>What it does</th>
              <th>Segments</th>
              <th>Minimum rank</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((c) => (
              <tr key={c.name} data-role="segments-command-row">
                <td className={styles.nowrap}>
                  <code>!{c.name}</code> {c.grammar && <code className={styles.dim}>{c.grammar.args}</code>}
                </td>
                <td>{c.grammar?.what ?? <span className={styles.dim}>No description in this console yet.</span>}</td>
                <td>
                  {c.segments.length === 0 ? (
                    <span className={styles.dim}>no segment accepts it</span>
                  ) : (
                    c.segments.map((id) => (
                      <RouteLink key={id} to={pageRoute("segments", "tables", id)} className={styles.ref}>
                        {id}
                      </RouteLink>
                    ))
                  )}
                </td>
                <td>
                  {c.name === "propose" && ranks === undefined ? (
                    <span className={styles.dim}>rank-gated</span>
                  ) : c.minRank === undefined ? (
                    <Chip>any rank</Chip>
                  ) : (
                    <Chip tone="ok">{rankTitle(ranks, c.minRank)} and up</Chip>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className={styles.note}>
        <code>!propose</code> is gated by <code>propose_min_rank</code> in the rank table; every other command is open to every rank, and votes count with the voter’s weight.
      </p>
    </Section>
  );
}

function OpGrid({ tables }: { tables: SegmentTables }) {
  const { columns, rows } = opGrid(tables);
  return (
    <Section title="The action grammar" right={`${rows.length} ops · ${columns.length} runnable segments`}>
      <div className={styles.tblwrap}>
        <table className={`${styles.tbl} ${styles.grid}`} data-role="segments-op-grid">
          <thead>
            <tr>
              <th>Op</th>
              {columns.map((s) => (
                <th key={s.id} className={styles.c}>
                  <RouteLink to={pageRoute("segments", "tables", s.id)} className={styles.colLink}>
                    {s.id}
                  </RouteLink>
                </th>
              ))}
              <th className={styles.num}>Segments</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.op} data-role="segments-op-row">
                <td>
                  <code>{r.op}</code>
                </td>
                {r.allowed.map((ok, i) => (
                  <td key={columns[i]?.id ?? i} className={styles.c}>
                    {ok ? <i className={styles.tickOk} aria-label="allowed" role="img" /> : <span className={styles.dim} aria-label="not allowed">–</span>}
                  </td>
                ))}
                <td className={styles.num}>{r.count}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className={styles.note}>
        The model writes actions as <code>OP: {"{"}"op": "show_card", …{"}"}</code> lines; the runtime checks each against this grid. Descoped segments allow nothing and are left out.
      </p>
    </Section>
  );
}
