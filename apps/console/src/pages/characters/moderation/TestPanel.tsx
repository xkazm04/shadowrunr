// Try a sample message against the rules as they stand: moderator.rules.test has no side effects
// and logs nothing. A self-disclosure is never acted on, and the result says so.
import type { ModRulesTestResult } from "@shadowrunr/contract";
import { useState } from "react";
import { useMutation, useRpc } from "../../../data";
import { Button } from "../../../ui/Button";
import { Chip } from "../../../ui/Chip";
import { actionText, ruleName } from "./model";
import s from "./Moderation.module.css";
import { Field, PanelHead } from "./parts";
import { errorText } from "./RulesPanel";

export function TestPanel() {
  const test = useMutation("moderator.rules.test");
  const rules = useRpc("moderator.rules.list", {});
  const [text, setText] = useState("");
  const [viewer, setViewer] = useState("");
  const [result, setResult] = useState<ModRulesTestResult | undefined>(undefined);
  const [error, setError] = useState<string | undefined>(undefined);
  const run = async () => {
    setError(undefined);
    const v = viewer.trim();
    const r = await test.run({ text, ...(v === "" ? {} : { viewer_name: v }) });
    if (r.ok) setResult(r.value);
    else {
      setResult(undefined);
      setError(errorText(r.error));
    }
  };
  return (
    <section className={s.panel} data-role="moderation-test" aria-labelledby="moderation-test">
      <PanelHead id="moderation-test" title="Test" />
      <label className={s.field}>
        <span className={s.label}>A chat message</span>
        <textarea className={s.area} value={text} maxLength={500} onChange={(e) => setText(e.target.value)} data-role="moderation-test-text" />
      </label>
      <Field label="Viewer name" value={viewer} onChange={setViewer} role="moderation-test-viewer" placeholder="optional" />
      <div className={s.actions}>
        <Button variant="ink" disabled={test.pending || text.trim() === ""} onClick={() => void run()} data-role="moderation-test-run">
          {test.pending ? "Running…" : "Run"}
        </Button>
      </div>
      {error !== undefined && <p className={s.err}>{error}</p>}
      {result && (
        <div data-role="moderation-test-result">
          {result.disclosure && (
            <p className={s.note} data-role="moderation-test-disclosure">
              Read as a self-disclosure: no rule acts on it.
            </p>
          )}
          {result.decisions.length === 0 ? (
            !result.disclosure && (
              <p className={s.note} data-role="moderation-test-none">
                No rule matched
              </p>
            )
          ) : (
            <ul className={s.list}>
              {result.decisions.map((d) => (
                <li key={d.id} className={s.item} data-role="moderation-test-match">
                  <span className={s.meta}>
                    <b>{ruleName(rules.data?.rules, d.rule_id)}</b>
                    <Chip tone="caution">{actionText(d.action, d.timeout_s)}</Chip>
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </section>
  );
}
