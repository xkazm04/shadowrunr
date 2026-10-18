// Optional rules: real switches over character.rules / character.rules.set. The daemon keeps the
// operator's override in <data>/characters/<id>/rules.json and never rewrites character.toml. A
// switch changes the instrument (the enabled set is hashed into it), so results run under
// different rule sets are never merged.
import { useState } from "react";
import type { CharacterDetail, RuleState } from "@shadowrunr/contract";
import { QueryView } from "../../kit";
import { useMutation, useRpc, viewOfError, type DataError } from "../../data";
import { Chip, Head, StatePanel, Switch } from "../../ui";
import { ruleBody } from "./doc";
import styles from "./Layer2.module.css";
import { Markdown } from "./Markdown";

/** The latest reply of a successful switch, valid only until the rules query answers anew. */
interface Shadow {
  readonly basis: readonly RuleState[] | undefined;
  readonly value: readonly RuleState[];
}

export function RulesTab({ d }: { d: CharacterDetail }) {
  const rules = useRpc("character.rules", { character: d.id });
  const list = useRpc("character.list", {});
  const set = useMutation("character.rules.set");
  const [shadow, setShadow] = useState<Shadow | undefined>(undefined);
  const [busyId, setBusyId] = useState<string | undefined>(undefined);
  const [failure, setFailure] = useState<{ id: string; error: DataError } | undefined>(undefined);

  const effective = shadow && shadow.basis === rules.data ? shadow.value : rules.data;
  const text = new Map(d.rules.map((r) => [r.id, r.text]));

  const toggle = async (id: string, enabled: boolean) => {
    setBusyId(id);
    setFailure(undefined);
    const r = await set.run({ character: d.id, id, enabled });
    setBusyId(undefined);
    if (!r.ok) {
      setFailure({ id, error: r.error });
      return;
    }
    setShadow({ basis: rules.data, value: r.value });
    // character.rules.set has no invalidation entry yet (reported): refetch what it changes.
    rules.reload();
    list.reload();
  };

  return (
    <>
      <Head
        kicker="Characters · the package"
        title="Optional rules"
        lede="Operator choices layered on top of the constitution. They are not red lines: they can be switched on and off, and each switch changes the instrument."
      />
      <QueryView query={rules} isEmpty={(r) => r.length === 0} emptyTitle="No optional rules" emptyHint="Add rules/<id>.md and list it under [rules] in character.toml.">
        {() => {
          const rows = effective ?? [];
          const changed = rows.filter((r) => r.enabled !== r.default).length;
          return (
            <div className={styles.cols2w}>
              <section>
                <h2 className={styles.secHead}>Rules</h2>
                <ul className={styles.rules} data-role="characters-rules">
                  {rows.map((r) => (
                    <li key={r.id} className={styles.rule} data-role="characters-rule" data-rule-id={r.id} data-enabled={r.enabled}>
                      <div>
                        <h3>{r.title}</h3>
                        {text.has(r.id) && <Markdown blocks={ruleBody(text.get(r.id) ?? "")} />}
                        <small>
                          <code>{r.id}</code> · package default {r.default ? "on" : "off"} {r.enabled !== r.default && <Chip tone="caution">changed by the operator</Chip>}
                        </small>
                        {failure?.id === r.id && (
                          <StatePanel state={viewOfError(failure.error).state} title={`The switch did not take · ${viewOfError(failure.error).title}`} detail={`${failure.error.code}: ${failure.error.message}`} />
                        )}
                      </div>
                      <div role="group" aria-label={r.title} className={styles.sw}>
                        <Switch checked={r.enabled} label="Enabled" disabled={busyId !== undefined} onChange={(next) => void toggle(r.id, next)} />
                      </div>
                    </li>
                  ))}
                </ul>
              </section>
              <aside className={styles.hashcard} data-role="characters-instrument">
                <h2>The instrument</h2>
                <p className={styles.hashState} data-changed={changed > 0}>
                  {changed === 0 ? "These match the package defaults." : `${changed} of ${rows.length} ${rows.length === 1 ? "rule differs" : "rules differ"} from the package defaults.`}
                </p>
                <p className={styles.dim}>
                  The enabled set is part of the instrument hash. Results from different hashes are never merged: runs judged under another rule set do not count toward runs with these rules.
                </p>
                {busyId !== undefined && <p className={styles.dim}>Saving…</p>}
              </aside>
            </div>
          );
        }}
      </QueryView>
    </>
  );
}
