// Add or edit one rule in the side drawer. Save sends the whole list (the edited rule in place,
// or appended); a refusal from the daemon prints under the form and the drawer stays open.
import type { ModActionKind, ModRule } from "@shadowrunr/contract";
import { useState } from "react";
import { Button } from "../../../ui/Button";
import { Drawer } from "../../../ui/Drawer";
import { PillGroup } from "../../../ui/PillGroup";
import { Switch } from "../../../ui/Switch";
import { ACTION_NAME, ACTION_ORDER, blankForm, formOf, KIND_NAME, KIND_ORDER, ruleOf, upsert, withName, type RuleForm } from "./model";
import s from "./Moderation.module.css";
import { Field } from "./parts";

const ACTIONS = ACTION_ORDER.map((id) => ({ id, label: ACTION_NAME[id] }));
const KINDS = KIND_ORDER.map((id) => ({ id, label: KIND_NAME[id] }));

export function RuleDrawer(props: {
  open: boolean;
  /** The rule being edited; undefined adds one. */
  rule: ModRule | undefined;
  rules: readonly ModRule[];
  pending: boolean;
  save: (next: ModRule[]) => Promise<string | undefined>;
  onClose: () => void;
}) {
  const { rule } = props;
  return (
    <Drawer open={props.open} onClose={props.onClose} kicker="Moderation · rule" title={rule ? `Edit ${rule.name}` : "Add a rule"} page="characters" data-role="moderation-rule-drawer">
      <Form key={rule?.id ?? "add"} {...props} />
    </Drawer>
  );
}

function Form({ rule, rules, pending, save, onClose }: Parameters<typeof RuleDrawer>[0]) {
  const [f, setF] = useState<RuleForm>(() => (rule ? formOf(rule) : blankForm()));
  const [error, setError] = useState<string | undefined>(undefined);
  const put = (patch: Partial<RuleForm>) => {
    setF((x) => ({ ...x, ...patch }));
    setError(undefined);
  };
  const taken = rules.filter((r) => r.id !== rule?.id).map((r) => r.id);
  const built = ruleOf(f, taken);
  const submit = async () => {
    if (!built.ok) return;
    const e = await save(upsert(rules, built.rule, rule?.id));
    if (e === undefined) onClose();
    else setError(e);
  };
  return (
    <form
      className={s.form}
      data-role="moderation-rule-form"
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
    >
      <div className={s.fields}>
        <Field label="Name" value={f.name} onChange={(name) => put(withName(f, name))} role="moderation-rule-name-input" placeholder="No links" />
        <Field label="Id" value={f.id} onChange={(id) => put({ id, idTouched: true })} role="moderation-rule-id" mono />
      </div>
      <div className={s.field}>
        <span className={s.label}>Kind</span>
        <PillGroup label="Kind" options={KINDS} value={f.kind} onChange={(kind) => put({ kind })} />
      </div>
      {f.kind === "pattern" ? (
        <Field label="Pattern" value={f.matcher} onChange={(matcher) => put({ matcher })} role="moderation-rule-matcher" placeholder="https?://" mono />
      ) : (
        <label className={s.field}>
          <span className={s.label}>What the rule catches, in plain words</span>
          <textarea className={s.area} value={f.matcher} maxLength={500} onChange={(e) => put({ matcher: e.target.value })} data-role="moderation-rule-matcher" />
        </label>
      )}
      <div className={s.field}>
        <span className={s.label}>Action</span>
        <PillGroup label="Action" options={ACTIONS} value={f.action} onChange={(action) => put({ action })} />
      </div>
      {f.action === "timeout" && <Field label="Timeout (minutes)" value={f.timeoutMin} onChange={(timeoutMin) => put({ timeoutMin })} role="moderation-rule-timeout" numeric />}
      <Switch checked={f.strikes} label="Escalate on repeat strikes" onChange={(strikes) => put({ strikes })} />
      {f.strikes && <Strikes f={f} put={put} />}
      <Switch checked={f.enabled} label="Enabled" onChange={(enabled) => put({ enabled })} />
      <div className={s.actions}>
        <Button type="submit" variant="ink" disabled={pending || !built.ok} data-role="moderation-rule-save">
          {pending ? "Saving…" : "Save rule"}
        </Button>
        <Button onClick={onClose}>Cancel</Button>
      </div>
      {!built.ok && <p className={s.note}>{built.error}</p>}
      {error !== undefined && (
        <p className={s.err} role="alert" data-role="moderation-rule-error">
          {error}
        </p>
      )}
    </form>
  );
}

function Strikes({ f, put }: { f: RuleForm; put: (p: Partial<RuleForm>) => void }) {
  return (
    <div className={s.fields} data-role="moderation-rule-strikes">
      <Field label="Strikes" value={f.count} onChange={(count) => put({ count })} role="moderation-rule-strike-count" numeric />
      <Field label="Within (minutes)" value={f.windowMin} onChange={(windowMin) => put({ windowMin })} role="moderation-rule-strike-window" numeric />
      <div className={s.field}>
        <span className={s.label}>Then</span>
        <PillGroup<ModActionKind> label="Then" options={ACTIONS} value={f.then} onChange={(then) => put({ then })} />
      </div>
      {f.then === "timeout" && <Field label="Then timeout (minutes)" value={f.thenTimeoutMin} onChange={(thenTimeoutMin) => put({ thenTimeoutMin })} role="moderation-rule-strike-timeout" numeric />}
    </div>
  );
}
