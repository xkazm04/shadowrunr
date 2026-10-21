// `reconcile/directives`: standing orders fed to every stream until deactivated, over
// `directive.list`, `directive.add` and `directive.deactivate` (stored by the daemon).
// Directives the Reconciler proposed in the Lord's chat (`active: false, proposed: true`,
// WP14) show as Proposed with Keep and Drop, over the same RPCs as the chat's chips: Keep is
// `directive.add` with the proposal's kind, text and scope; Drop is `directive.deactivate`.
import type { Directive, DirectiveKind, SegmentId } from "@shadowrunr/contract";
import { useState, type FormEvent } from "react";
import { useMutation, useRpc, viewOfError } from "../../data";
import { QueryView } from "../../kit";
import { Chip } from "../../ui/Chip";
import { FilterTable, type Column } from "../../ui/FilterTable";
import { Head } from "../../ui/Section";
import styles from "./Layer2.module.css";
import { dateTime } from "./model";
import { Button } from "../../ui/Button";

/** Segments a directive may be scoped to (SEG-02B is descoped). */
const SCOPES: readonly SegmentId[] = ["SEG-01", "SEG-02A"];

type Facet = "active" | "proposed" | "all" | "inactive";

const isProposed = (d: Directive) => d.proposed === true;
/** Retired or dropped: neither live nor awaiting the owner. */
const isOff = (d: Directive) => !d.active && !isProposed(d);

export function DirectivesPage() {
  const list = useRpc("directive.list", {});
  return (
    <>
      <Head
        kicker="Reconcile & plan · layer 2"
        title="Directives"
        lede="Standing orders fed to every stream until deactivated. Prevent forbids a behaviour; try asks for an experiment. Proposals from the Lord’s chat stay inactive until you keep them."
      />
      <AddForm />
      <QueryView query={list} isEmpty={(d) => d.length === 0} emptyTitle="No directives yet" emptyHint="Add one above, or ask the Lord in the chat: an order there comes back as a proposal to keep or drop.">
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
        <Button type="submit" variant="primary" disabled={add.pending || text.trim().length === 0}>
          {add.pending ? "Adding…" : "Add directive"}
        </Button>
      </form>
      {error && (
        <p className={styles.formError} role="alert">
          {error}
        </p>
      )}
    </>
  );
}

/** A proposal awaiting the owner: Keep makes it active under its own id, Drop retires it. */
function Proposal({ d }: { d: Directive }) {
  const keep = useMutation("directive.add");
  const drop = useMutation("directive.deactivate");
  const [error, setError] = useState<string | undefined>();
  const label = `${d.kind}: ${d.text}`;
  const busy = keep.pending || drop.pending;
  return (
    <span className={styles.rowAction} data-role="reconcile-directive-proposal">
      <Chip tone="caution" title="Proposed by the Reconciler in the Lord’s chat: inactive until kept">
        Proposed
      </Chip>
      <Button
        type="button"
        variant="secondary"
        disabled={busy}
        aria-label={`Keep ${label}`}
        onClick={() => void keep.run({ kind: d.kind, text: d.text, ...(d.scope === undefined ? {} : { scope: d.scope }) }).then((r) => setError(r.ok ? undefined : viewOfError(r.error).title))}
      >
        Keep
      </Button>
      <Button type="button" variant="secondary" disabled={busy} aria-label={`Drop ${label}`} onClick={() => void drop.run({ id: d.id }).then((r) => setError(r.ok ? undefined : viewOfError(r.error).title))}>
        Drop
      </Button>
      {error && <small role="alert">{error}</small>}
    </span>
  );
}

function Deactivate({ d }: { d: Directive }) {
  const off = useMutation("directive.deactivate");
  const [error, setError] = useState<string | undefined>();
  if (!d.active) return <span className={styles.dim}>Off</span>;
  return (
    <span className={styles.rowAction}>
      <Button
        type="button"
        variant="secondary"
        disabled={off.pending}
        onClick={() =>
          void off.run({ id: d.id }).then((r) => setError(r.ok ? undefined : viewOfError(r.error).title))
        }
      >
        {off.pending ? "Turning off…" : "Deactivate"}
      </Button>
      {error && <small role="alert">{error}</small>}
    </span>
  );
}

const COLUMNS: readonly Column<Directive>[] = [
  { id: "kind", header: "Kind", cell: (d) => <Chip tone={d.kind === "prevent" ? "fail" : "ok"}>{d.kind}</Chip>, sortValue: (d) => d.kind },
  { id: "text", header: "Directive", cell: (d) => <span className={isOff(d) ? styles.struck : undefined}>{d.text}</span>, sortValue: (d) => d.text },
  { id: "scope", header: "Scope", cell: (d) => d.scope ?? "all segments", sortValue: (d) => d.scope ?? "" },
  { id: "created", header: "Added", cell: (d) => dateTime(d.created_ms), sortValue: (d) => d.created_ms },
  { id: "state", header: "State", cell: (d) => (isProposed(d) ? <Proposal d={d} /> : <Deactivate d={d} />), sortValue: (d) => (isProposed(d) ? 0 : d.active ? 1 : 2), align: "end" },
];

function DirectiveTable({ rows }: { rows: readonly Directive[] }) {
  // Open on the proposals while any await the owner; fixed at mount so keeping the last one
  // does not move the view.
  const [initial] = useState<Facet>(() => (rows.some(isProposed) ? "proposed" : "active"));
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
          initial,
          options: [
            { id: "active", label: "Active" },
            { id: "proposed", label: "Proposed" },
            { id: "all", label: "All" },
            { id: "inactive", label: "Off" },
          ],
          predicate: (d, f) => (f === "all" ? true : f === "active" ? d.active : f === "proposed" ? isProposed(d) : isOff(d)),
        }}
      />
    </div>
  );
}
