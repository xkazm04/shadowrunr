// The operator's rules: one table with a kind-or-action facet. Every change (add, edit, toggle,
// delete) sends the whole list through moderator.rules.set, which replaces it atomically; the
// daemon refuses the whole write on one invalid rule, and its reason prints here.
import type { ModRule } from "@shadowrunr/contract";
import { useMemo, useState } from "react";
import { useMutation, useRpc, viewOfError, type DataError } from "../../../data";
import { QueryView } from "../../../kit";
import { Button } from "../../../ui/Button";
import { Chip } from "../../../ui/Chip";
import { FilterTable, type Column, type Facets } from "../../../ui/FilterTable";
import { Switch } from "../../../ui/Switch";
import { actionText, KIND_NAME, matcherText, RULE_FACETS, ruleInFacet, strikesText, type RuleFacet } from "./model";
import s from "./Moderation.module.css";
import { Confirm, PanelHead } from "./parts";
import { RuleDrawer } from "./RuleDrawer";

export const errorText = (e: DataError): string => `${viewOfError(e).title}: ${e.message}`;

/** The drawer's subject: a new rule, or the id of the rule being edited. */
type Editing = { readonly mode: "add" } | { readonly mode: "edit"; readonly id: string };

export function RulesPanel() {
  const q = useRpc("moderator.rules.list", {});
  return (
    <section className={s.panel} data-role="moderation-rules" aria-labelledby="moderation-rules">
      <QueryView query={q}>{(r) => <Rules rules={r.rules} />}</QueryView>
    </section>
  );
}

function Rules({ rules }: { rules: readonly ModRule[] }) {
  const set = useMutation("moderator.rules.set");
  const [editing, setEditing] = useState<Editing | undefined>(undefined);
  const [deleting, setDeleting] = useState<string | undefined>(undefined);
  const [error, setError] = useState<string | undefined>(undefined);

  /** Sends the whole list; the reason a refused write gives, or undefined when it landed. */
  const save = async (next: ModRule[]): Promise<string | undefined> => {
    setError(undefined);
    const r = await set.run({ rules: next });
    return r.ok ? undefined : errorText(r.error);
  };
  const inline = (next: ModRule[]) =>
    void save(next).then((e) => {
      setDeleting(undefined);
      setError(e);
    });

  const columns = useMemo<Column<ModRule>[]>(
    () => [
      {
        id: "enabled",
        header: "On",
        cell: (r) => (
          <Switch checked={r.enabled} hideLabel label={`${r.name} enabled`} disabled={set.pending} onChange={(on) => inline(rules.map((x) => (x.id === r.id ? { ...x, enabled: on } : x)))} />
        ),
      },
      { id: "name", header: "Name", cell: (r) => <b data-role="moderation-rule-name">{r.name}</b>, sortValue: (r) => r.name },
      { id: "kind", header: "Kind", cell: (r) => <Chip tone="muted">{KIND_NAME[r.kind]}</Chip>, sortValue: (r) => r.kind },
      { id: "matcher", header: "Pattern or text", cell: (r) => <span className={r.kind === "pattern" ? s.mono : undefined}>{matcherText(r)}</span> },
      { id: "action", header: "Action", cell: (r) => actionText(r.action, r.timeout_s), sortValue: (r) => r.action },
      { id: "strikes", header: "Strikes", cell: (r) => <span className={s.dim}>{strikesText(r)}</span> },
      {
        id: "edit",
        header: "",
        align: "end",
        cell: (r) => (
          <span className={s.actions}>
            <Button size="compact" onClick={() => setEditing({ mode: "edit", id: r.id })} data-role="moderation-rule-edit">
              Edit
            </Button>
            <Button size="compact" disabled={set.pending} onClick={() => setDeleting(r.id)} data-role="moderation-rule-delete">
              Delete
            </Button>
          </span>
        ),
      },
    ],
    // `inline` closes over `rules` and the mutation; both are in the list.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [rules, set.pending],
  );
  const facets = useMemo<Facets<ModRule, RuleFacet>>(() => ({ label: "Kind or action", options: RULE_FACETS, initial: "all", predicate: ruleInFacet }), []);
  const doomed = rules.find((r) => r.id === deleting);
  const edited = editing?.mode === "edit" ? rules.find((r) => r.id === editing.id) : undefined;

  return (
    <>
      <PanelHead id="moderation-rules" title="Rules">
        <Button variant="ink" size="compact" onClick={() => setEditing({ mode: "add" })} data-role="moderation-rule-add">
          Add a rule
        </Button>
      </PanelHead>
      {rules.length === 0 ? (
        <p className={s.note} data-role="moderation-rules-empty">
          No rules: the moderator acts on nothing. Add one, or ask for them through `shadowrunr mcp`.
        </p>
      ) : (
        <FilterTable<ModRule, RuleFacet>
          rows={rules}
          columns={columns}
          getKey={(r) => r.id}
          haystack={(r) => `${r.name} ${r.id} ${matcherText(r)}`}
          label="Moderation rules"
          placeholder="Filter rules"
          facets={facets}
        />
      )}
      {doomed && (
        <Confirm
          role="moderation-rule-delete-confirm"
          text={`Delete "${doomed.name}"? The moderator stops applying it at the next message.`}
          yes="Delete the rule"
          danger
          pending={set.pending}
          onYes={() => inline(rules.filter((r) => r.id !== doomed.id))}
          onNo={() => setDeleting(undefined)}
        />
      )}
      {error !== undefined && (
        <p className={s.err} role="alert" data-role="moderation-rules-error">
          {error}
        </p>
      )}
      <RuleDrawer open={editing !== undefined} rule={edited} rules={rules} pending={set.pending} save={save} onClose={() => setEditing(undefined)} />
    </>
  );
}
