// `reconcile/directives`: standing orders fed to every stream until deactivated, over
// `directive.list`, `directive.add` and `directive.deactivate` (stored by the daemon).
import type { Directive, DirectiveKind, SegmentId } from "@shadowrunr/contract";
import { useState, type FormEvent } from "react";
import { useMutation, useRpc, viewOfError } from "../../data";
import { QueryView } from "../../kit";
import { Chip, FilterTable, Head, type Column } from "../../ui";
import styles from "./Layer2.module.css";
import { dateTime } from "./model";

/** Segments a directive may be scoped to (SEG-02B is descoped). */
const SCOPES: readonly SegmentId[] = ["SEG-01", "SEG-02A"];

type Facet = "active" | "all" | "inactive";

export function DirectivesPage() {
  const list = useRpc("directive.list", {});
  return (
    <>
      <Head
        kicker="Reconcile & plan · layer 2"
        title="Directives"
        lede="Standing orders fed to every stream until deactivated. Prevent forbids a behaviour; try asks for an experiment."
      />
      <AddForm />
      <QueryView query={list} isEmpty={(d) => d.length === 0} emptyTitle="No directives yet" emptyHint="Add one above, or ask the Lord in the chat: an order there becomes a directive.">
        {(rows) => <DirectiveTable rows={rows} />}
      </QueryView>
    </>
  );
}

function AddForm() {
  const add = useMutation("directive.add");
  const [kind, setKind] = useState<DirectiveKind>("prevent");
  const [text, setText] = useState("");
  const [scope, setScope] = useState<SegmentId | "">("");
  const [error, setError] = useState<string | undefined>();

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    const t = text.trim();
    if (!t) return;
    setError(undefined);
    const r = await add.run({ kind, text: t, ...(scope === "" ? {} : { scope }) });
    if (r.ok) setText("");
    else setError(`${viewOfError(r.error).title}: ${r.error.message}`);
  }

  return (
    <>
      <form className={styles.form} onSubmit={(e) => void onSubmit(e)} data-role="reconcile-directive-form">
        <label className={styles.field}>
          <span>Kind</span>
          <select value={kind} onChange={(e) => setKind(e.target.value === "try" ? "try" : "prevent")}>
            <option value="prevent">Prevent</option>
            <option value="try">Try</option>
          </select>
        </label>
        <label className={`${styles.field} ${styles.grow}`}>
          <span>Directive</span>
          <input
            type="text"
            value={text}
            maxLength={160}
            onChange={(e) => setText(e.target.value)}
            placeholder="e.g. never repeat the same status line twice in a row"
          />
        </label>
        <label className={styles.field}>
          <span>Scope</span>
          <select value={scope} onChange={(e) => setScope(SCOPES.find((s) => s === e.target.value) ?? "")}>
            <option value="">All segments</option>
            {SCOPES.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        </label>
        <button type="submit" className={styles.primary} disabled={add.pending || text.trim().length === 0}>
          {add.pending ? "Adding…" : "Add directive"}
        </button>
      </form>
      {error && (
        <p className={styles.formError} role="alert">
          {error}
        </p>
      )}
    </>
  );
}

function Deactivate({ d }: { d: Directive }) {
  const off = useMutation("directive.deactivate");
  const [error, setError] = useState<string | undefined>();
  if (!d.active) return <span className={styles.dim}>Off</span>;
  return (
    <span className={styles.rowAction}>
      <button
        type="button"
        className={styles.secondary}
        disabled={off.pending}
        onClick={() =>
          void off.run({ id: d.id }).then((r) => setError(r.ok ? undefined : viewOfError(r.error).title))
        }
      >
        {off.pending ? "Turning off…" : "Deactivate"}
      </button>
      {error && <small role="alert">{error}</small>}
    </span>
  );
}

const COLUMNS: readonly Column<Directive>[] = [
  { id: "kind", header: "Kind", cell: (d) => <Chip tone={d.kind === "prevent" ? "fail" : "ok"}>{d.kind}</Chip>, sortValue: (d) => d.kind },
  { id: "text", header: "Directive", cell: (d) => <span className={d.active ? undefined : styles.struck}>{d.text}</span>, sortValue: (d) => d.text },
  { id: "scope", header: "Scope", cell: (d) => d.scope ?? "all segments", sortValue: (d) => d.scope ?? "" },
  { id: "created", header: "Added", cell: (d) => dateTime(d.created_ms), sortValue: (d) => d.created_ms },
  { id: "state", header: "State", cell: (d) => <Deactivate d={d} />, sortValue: (d) => (d.active ? 0 : 1), align: "end" },
];

function DirectiveTable({ rows }: { rows: readonly Directive[] }) {
  const active = rows.filter((d) => d.active).length;
  return (
    <div className={styles.tableBox} data-role="reconcile-directives">
      <FilterTable<Directive, Facet>
        rows={rows}
        columns={COLUMNS}
        getKey={(d) => d.id}
        haystack={(d) => `${d.kind} ${d.text} ${d.scope ?? ""}`}
        label="Directives"
        placeholder="Filter directives"
        initialSort={{ column: "created", dir: "desc" }}
        facets={{
          label: "Show",
          initial: "active",
          options: [
            { id: "active", label: "Active", count: active },
            { id: "all", label: "All", count: rows.length },
            { id: "inactive", label: "Off", count: rows.length - active },
          ],
          predicate: (d, f) => f === "all" || (f === "active") === d.active,
        }}
      />
    </div>
  );
}
