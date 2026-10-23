// The decision log, newest first: the last 100 entries of decisions.jsonl, polled every 5 s while
// the tab is open and the window visible. Executed bans and timeouts can be lifted from here.
import type { ModDecision, ModLogEntry, ModRule, ModUnban } from "@shadowrunr/contract";
import { useState } from "react";
import { useMutation, useRpc } from "../../../data";
import { QueryView } from "../../../kit";
import { timeOf } from "../../../kit/freshness";
import { Button } from "../../../ui/Button";
import { Chip } from "../../../ui/Chip";
import { actionText, canUnban, rowOf, ruleName } from "./model";
import s from "./Moderation.module.css";
import { Confirm, PanelHead, usePoll } from "./parts";
import { errorText } from "./RulesPanel";

export const LOG_POLL_MS = 5000;
export const LOG_LIMIT = 100;

export function LogPanel() {
  const log = useRpc("moderator.log", { limit: LOG_LIMIT });
  const rules = useRpc("moderator.rules.list", {});
  usePoll(log.reload, LOG_POLL_MS);
  return (
    <section className={s.panel} data-role="moderation-log" aria-labelledby="moderation-log">
      <PanelHead id="moderation-log" title="Log" />
      <QueryView query={log} isEmpty={(l) => l.entries.length === 0} emptyTitle="Nothing logged yet" emptyHint="Decisions land here as the moderator screens the chat.">
        {(l) => <Entries entries={l.entries} rules={rules.data?.rules} />}
      </QueryView>
    </section>
  );
}

function Entries({ entries, rules }: { entries: readonly ModLogEntry[]; rules: readonly ModRule[] | undefined }) {
  const unban = useMutation("moderator.unban");
  const [asking, setAsking] = useState<ModDecision | undefined>(undefined);
  const [error, setError] = useState<string | undefined>(undefined);
  const lift = async (d: ModDecision) => {
    setError(undefined);
    const r = await unban.run({ platform: d.platform, viewer_id: d.viewer_id });
    setAsking(undefined);
    if (!r.ok) setError(errorText(r.error));
  };
  // An unban names only the viewer id; the decisions in view name the viewer.
  const names = new Map(entries.map(rowOf).flatMap((r) => (r.kind === "decision" ? [[r.d.viewer_id, r.d.viewer_name] as const] : [])));
  return (
    <>
      {asking && (
        <Confirm
          role="moderation-unban-confirm"
          text={`Lift the ${asking.action} on ${asking.viewer_name}?`}
          yes="Unban"
          pending={unban.pending}
          onYes={() => void lift(asking)}
          onNo={() => setAsking(undefined)}
        />
      )}
      {error !== undefined && <p className={s.err}>{error}</p>}
      <ul className={s.list}>
        {entries.map((e, i) => {
          const row = rowOf(e);
          return row.kind === "decision" ? (
            <Decision key={`d-${row.d.id}`} d={row.d} rule={ruleName(rules, row.d.rule_id)} busy={unban.pending} onUnban={() => setAsking(row.d)} />
          ) : (
            <Unban key={`u-${row.u.ts_ms}-${row.u.viewer_id}-${i}`} u={row.u} name={names.get(row.u.viewer_id) ?? row.u.viewer_id} />
          );
        })}
      </ul>
    </>
  );
}

function Decision({ d, rule, busy, onUnban }: { d: ModDecision; rule: string; busy: boolean; onUnban: () => void }) {
  return (
    <li className={s.item} data-role="moderation-log-decision" data-executed={d.executed}>
      <span className={s.meta}>
        <span className={s.dim}>{timeOf(d.ts_ms)}</span>
        <b>{d.viewer_name}</b>
        <span>{rule}</span>
        <Chip tone="caution">{actionText(d.action, d.timeout_s)}</Chip>
        <Chip tone={d.mode === "armed" ? "signal" : "muted"}>{d.mode === "armed" ? "Armed" : "Shadow"}</Chip>
        {d.mode === "armed" && <Chip tone={d.executed ? "ok" : "fail"}>{d.executed ? "Executed" : "Failed"}</Chip>}
        {canUnban(d) && (
          <Button size="compact" disabled={busy} onClick={onUnban} data-role="moderation-unban">
            Unban
          </Button>
        )}
      </span>
      {d.error !== undefined && <p className={s.err}>{d.error}</p>}
      {d.line !== undefined && <p className={s.line}>“{d.line}”</p>}
    </li>
  );
}

function Unban({ u, name }: { u: ModUnban; name: string }) {
  return (
    <li className={s.item} data-role="moderation-log-unban" data-executed={u.executed}>
      <span className={s.meta}>
        <span className={s.dim}>{timeOf(u.ts_ms)}</span>
        <b>{name}</b>
        <span>Unban</span>
        <Chip tone={u.executed ? "ok" : "fail"}>{u.executed ? "Executed" : "Failed"}</Chip>
      </span>
      {u.error !== undefined && <p className={s.err}>{u.error}</p>}
    </li>
  );
}
