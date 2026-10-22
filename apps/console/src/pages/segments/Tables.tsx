// segments/tables[/<SEG>[/<state>]]: the lifecycle flow, the portfolio (virtualised) and each segment's
// table: states, transitions, allowed ops, commands and the handover clock (l2b.js `tables`).
import type { SegmentStatus, SegmentTableInfo, SegmentTables } from "@shadowrunr/contract";
import { QueryView } from "../../kit";
import { pageRoute, RouteLink, useRouter } from "../../router";
import { Chip } from "../../ui/Chip";
import { FilterTable, type Column } from "../../ui/FilterTable";
import { Head, Section } from "../../ui/Section";
import { StatePanel } from "../../ui/States";
import { useTables } from "./hooks";
import styles from "./Layer2.module.css";
import { LIFECYCLE, STATUS_TONE, statusCounts } from "./model";
import { SegmentCard } from "./SegmentCard";

/** Up to this many segments, the tables page prints every card; beyond it, one per route. */
export const CARD_LIMIT = 8;

type Facet = "all" | "runnable" | "descoped";

export function TablesPage({ focus, state }: { focus: string | undefined; state?: string | undefined }) {
  const tables = useTables();
  return (
    <>
      <Head
        kicker="Segments & ranks · layer 2"
        title={focus ?? "Segment tables"}
        lede="Every segment is a table, not code: states, allowed ops, commands, clocks and transitions, interpreted by one runtime. A segment is born as a pilot and earns its way up."
      />
      <QueryView query={tables} isEmpty={(t) => t.segments.length === 0} emptyTitle="The daemon has no segment tables" emptyHint="Tables load from the segments directory in shadowrunr.toml.">
        {(t) => <Body tables={t} focus={focus} state={state} />}
      </QueryView>
    </>
  );
}

function Body({ tables, focus, state }: { tables: SegmentTables; focus: string | undefined; state: string | undefined }) {
  const segs = tables.segments;
  if (focus !== undefined) {
    const seg = segs.find((s) => s.id === focus);
    const badState = seg !== undefined && state !== undefined && !seg.states.includes(state);
    return (
      <>
        {badState && <StatePanel state="invalid" title={`No state ${state} in ${focus}`} hint={`Its states: ${seg.states.join(", ")}. The card opens on ${seg.initial}.`} />}
        {seg ? (
          <SegmentCard seg={seg} focused state={badState ? undefined : state} />
        ) : (
          <StatePanel state="invalid" title={`No segment ${focus} in the tables`} hint="Pick one from the portfolio below." />
        )}
        <Portfolio segments={segs} selected={focus} />
      </>
    );
  }
  return (
    <>
      <Lifecycle segments={segs} />
      <Portfolio segments={segs} selected={undefined} />
      {segs.length <= CARD_LIMIT ? (
        segs.map((s) => <SegmentCard key={s.id} seg={s} focused={false} />)
      ) : (
        <p className={styles.note}>Open a segment from the portfolio to read its table.</p>
      )}
    </>
  );
}

function lifecycleNote(c: Readonly<Record<SegmentStatus, number>>): string {
  const parts: string[] = [];
  const word = (n: number) => (n === 1 ? "segment" : "segments");
  if (c.pilot > 0) parts.push(`${c.pilot} ${word(c.pilot)} ${c.pilot === 1 ? "runs" : "run"} as ${c.pilot === 1 ? "a pilot" : "pilots"}.`);
  if (c.descoped > 0) parts.push(`${c.descoped} ${c.descoped === 1 ? "was" : "were"} descoped: the runtime refuses to start ${c.descoped === 1 ? "it" : "them"}, and descoped is not retired.`);
  const empty = (["trial", "regular", "flagship"] as const).filter((s) => c[s] === 0);
  if (empty.length > 0) {
    const list = empty.join(", ");
    parts.push(`${list.charAt(0).toUpperCase()}${list.slice(1)} ${empty.length === 1 ? "is" : "are"} empty: nothing has earned ${empty.length === 1 ? "it" : "them"} yet.`);
  }
  return parts.join(" ");
}

function Lifecycle({ segments }: { segments: readonly SegmentTableInfo[] }) {
  const c = statusCounts(segments);
  return (
    <Section title="Lifecycle">
      <div className={styles.lcflow} data-role="segments-lcflow">
        {LIFECYCLE.map((s, i) => (
          <span key={s} className={styles.lcStep}>
            {i > 0 && <i className={styles.arr} aria-hidden="true" />}
            <span className={styles.lcs} data-status={s}>
              <b>{c[s]}</b>
              <span>{s}</span>
            </span>
          </span>
        ))}
      </div>
      {c.descoped > 0 && (
        <div className={styles.branch} data-role="segments-lcflow-descoped">
          <i className={styles.branchLine} aria-hidden="true" />
          <span className={`${styles.lcs} ${styles.lcsOff}`} data-status="descoped">
            <b>{c.descoped}</b>
            <span>descoped</span>
          </span>
        </div>
      )}
      <p className={styles.note}>{lifecycleNote(c)}</p>
    </Section>
  );
}

const COLUMNS: readonly Column<SegmentTableInfo>[] = [
  { id: "id", header: "ID", cell: (s) => <code>{s.id}</code>, sortValue: (s) => s.id },
  {
    id: "title",
    header: "Title",
    cell: (s) => (
      <RouteLink to={pageRoute("segments", "tables", s.id)} className={styles.link}>
        {s.title}
      </RouteLink>
    ),
    sortValue: (s) => s.title,
  },
  { id: "status", header: "Lifecycle", cell: (s) => <Chip tone={STATUS_TONE[s.status]}>{s.status}</Chip>, sortValue: (s) => s.status },
  { id: "states", header: "States", cell: (s) => s.states.length, sortValue: (s) => s.states.length, align: "end" },
  { id: "ops", header: "Ops", cell: (s) => s.allowed_ops.length, sortValue: (s) => s.allowed_ops.length, align: "end" },
  { id: "commands", header: "Commands", cell: (s) => s.commands.map((c) => `!${c}`).join(" "), align: "start" },
];

function Portfolio({ segments, selected }: { segments: readonly SegmentTableInfo[]; selected: string | undefined }) {
  const router = useRouter();
  return (
    <Section title="Portfolio" right={`${segments.length.toLocaleString("en-US")} tables`}>
      <div className={styles.tableBox} data-role="segments-portfolio">
        <FilterTable<SegmentTableInfo, Facet>
          rows={segments}
          columns={COLUMNS}
          getKey={(s) => s.id}
          haystack={(s) => `${s.id} ${s.title} ${s.status}`}
          label="Segment portfolio"
          viewKey="segments.tables"
          placeholder="Filter by id, title or lifecycle"
          facets={{
            label: "Lifecycle",
            initial: "all",
            options: [
              { id: "all", label: "All" },
              { id: "runnable", label: "Runnable" },
              { id: "descoped", label: "Descoped" },
            ],
            predicate: (s, f) => f === "all" || (f === "descoped") === (s.status === "descoped"),
          }}
          onActivate={(s) => router.navigate(pageRoute("segments", "tables", s.id))}
          {...(selected === undefined ? {} : { selectedKey: selected })}
        />
      </div>
    </Section>
  );
}
