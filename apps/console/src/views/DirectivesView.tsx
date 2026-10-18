import { useMemo, useState } from "react";
import { ACTIVE_SEGMENTS, type Directive, type DirectiveKind, type SegmentId } from "../contract";
import { useApi, useRpc } from "../api/context";
import { dateTime } from "../lib/format";
import { Button, Chip, Empty, ErrorLine, ErrorState, Inspector, KV, Loading, Segmented, Sheet, Toolbar, cx, useListNav } from "../ui/primitives";

type KindFilter = "all" | DirectiveKind;
type StatusFilter = "active" | "inactive" | "all";
type ScopeFilter = "any" | "global" | SegmentId;

export function filterDirectives(ds: Directive[], f: { kind: KindFilter; status: StatusFilter; scope: ScopeFilter; text: string }): Directive[] {
  const q = f.text.trim().toLowerCase();
  return ds
    .filter((d) => f.kind === "all" || d.kind === f.kind)
    .filter((d) => f.status === "all" || (f.status === "active" ? d.active : !d.active))
    .filter((d) => f.scope === "any" || (f.scope === "global" ? !d.scope : d.scope === f.scope))
    .filter((d) => !q || d.text.toLowerCase().includes(q) || d.id.toLowerCase().includes(q))
    .sort((a, b) => Number(b.active) - Number(a.active) || b.created_ms - a.created_ms);
}

function NewDirectiveSheet({ onClose, onCreated }: { onClose: () => void; onCreated: (d: Directive) => void }) {
  const { client } = useApi();
  const [kind, setKind] = useState<DirectiveKind>("prevent");
  const [scope, setScope] = useState<"" | SegmentId>("");
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const submit = async () => {
    if (!text.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const params = scope ? { kind, text: text.trim(), scope } : { kind, text: text.trim() };
      onCreated(await client.call("directive.add", params));
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Sheet
      title="New directive"
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={() => void submit()} disabled={busy || !text.trim()}>
            Add
          </Button>
        </>
      }
    >
      <form
        className="form"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <label>
          <span>Kind</span>
          <Segmented label="Kind" value={kind} onChange={setKind} options={[{ value: "prevent", label: "Prevent" }, { value: "try", label: "Try" }]} />
        </label>
        <label>
          <span>Scope</span>
          <select value={scope} onChange={(e) => setScope(e.target.value as "" | SegmentId)}>
            <option value="">All segments</option>
            {ACTIVE_SEGMENTS.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span>Text</span>
          <textarea rows={3} value={text} onChange={(e) => setText(e.target.value)} autoFocus />
        </label>
        <ErrorLine error={error} />
      </form>
    </Sheet>
  );
}

export function DirectivesView() {
  const { client } = useApi();
  const directives = useRpc("directive.list", {});
  const [kind, setKind] = useState<KindFilter>("all");
  const [status, setStatus] = useState<StatusFilter>("all");
  const [scope, setScope] = useState<ScopeFilter>("any");
  const [text, setText] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<unknown>(null);

  const all = directives.data ?? [];
  const rows = useMemo(() => filterDirectives(all, { kind, status, scope, text }), [all, kind, status, scope, text]);
  const nav = useListNav(rows.length, { onMove: (i) => setSelected(rows[i]?.id ?? null), onActivate: (i) => setSelected(rows[i]?.id ?? null) });
  const sel = all.find((d) => d.id === selected);

  const deactivate = async (d: Directive) => {
    setError(null);
    try {
      await client.call("directive.deactivate", { id: d.id });
      directives.reload();
    } catch (e) {
      setError(e);
    }
  };

  return (
    <div className="view">
      <div className="main">
        <Toolbar title="Directives" subtitle={`${all.filter((d) => d.active).length} active · ${all.length} total`}>
          <Button variant="primary" onClick={() => setAdding(true)}>
            New directive
          </Button>
        </Toolbar>
        <div className="filters">
          <Segmented label="Kind" value={kind} onChange={setKind} options={[{ value: "all", label: "All" }, { value: "prevent", label: "Prevent" }, { value: "try", label: "Try" }]} />
          <Segmented label="Status" value={status} onChange={setStatus} options={[{ value: "all", label: "All" }, { value: "active", label: "Active" }, { value: "inactive", label: "Inactive" }]} />
          <select value={scope} onChange={(e) => setScope(e.target.value as ScopeFilter)} aria-label="Scope">
            <option value="any">Any scope</option>
            <option value="global">All segments</option>
            {ACTIVE_SEGMENTS.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
          <input type="search" placeholder="Filter text" value={text} onChange={(e) => setText(e.target.value)} aria-label="Filter text" />
          <span className="muted">{rows.length} shown</span>
        </div>
        <div className="content">
          <ErrorLine error={error} />
          {directives.error ? (
            <ErrorState error={directives.error} onRetry={directives.reload} />
          ) : directives.loading && !all.length ? (
            <Loading />
          ) : !rows.length ? (
            <Empty title={all.length ? "No directives match the filters" : "No directives yet"} />
          ) : (
            <div className="table" {...nav.containerProps} aria-label="Directives">
              <div className="tr th">
                <span className="c-name">Kind</span>
                <span className="c-grow">Directive</span>
                <span className="c-name">Scope</span>
                <span className="c-date">Created</span>
                <span className="c-name">Status</span>
              </div>
              {rows.map((d, i) => (
                <div
                  key={d.id}
                  {...nav.rowProps(i)}
                  className={cx("tr", i === nav.active && "active", selected === d.id && "selected", !d.active && "inactive")}
                  onClick={() => setSelected(d.id)}
                >
                  <span className="c-name">
                    <Chip tone={d.active ? d.kind : "neutral"}>{d.kind}</Chip>
                  </span>
                  <span className="c-grow ellipsis">{d.text}</span>
                  <span className="c-name mono muted">{d.scope ?? "all"}</span>
                  <span className="c-date muted">{dateTime(d.created_ms)}</span>
                  <span className="c-name">{d.active ? <Chip tone="ok">active</Chip> : <span className="muted">off</span>}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
      {sel && (
        <Inspector
          title={<span className="mono">{sel.id}</span>}
          onClose={() => setSelected(null)}
          actions={
            sel.active ? (
              <Button variant="danger" onClick={() => void deactivate(sel)}>
                Deactivate
              </Button>
            ) : undefined
          }
        >
          <KV
            rows={[
              ["Kind", <Chip tone={sel.kind}>{sel.kind}</Chip>],
              ["Scope", sel.scope ?? "All segments"],
              ["Status", sel.active ? "Active: fed to every stream" : "Inactive"],
              ["Created", dateTime(sel.created_ms)],
            ]}
          />
          <p className="prose">{sel.text}</p>
        </Inspector>
      )}
      {adding && (
        <NewDirectiveSheet
          onClose={() => setAdding(false)}
          onCreated={(d) => {
            setAdding(false);
            directives.reload();
            setSelected(d.id);
          }}
        />
      )}
    </div>
  );
}
