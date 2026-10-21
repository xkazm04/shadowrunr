// The tone's line bank (tone.lines.list): every line with its status and situations, filtered by
// status and situation. Approve a seed, retire a line, edit one as a new approved line (the old
// one stays as it is), or send it to the variants panel. Each write is one text edit of
// lines/<tone>.toml (tone.lines.set_status, tone.lines.add); the list reloads after it.
import { useState } from "react";
import type { LineSituation, LineStatus, ToneLine } from "@shadowrunr/contract";
import { QueryView } from "../../kit";
import { useMutation, useRpc, viewOfError } from "../../data";
import { Button } from "../../ui/Button";
import { Chip, type Tone } from "../../ui/Chip";
import { PillGroup } from "../../ui/PillGroup";
import { StatePanel } from "../../ui/States";
import styles from "./Layer2.module.css";
import lab from "./ToneLab.module.css";
import { nextKey, type LabProps, type Seed } from "./toneTypes";
import { SITUATION_NAME, SITUATION_ORDER, STATUS_HINT, STATUS_NAME, STATUS_ORDER } from "./toneNames";

type StatusFilter = LineStatus | "all";
const STATUS_TONE: Readonly<Record<LineStatus, Tone>> = { seed: "neutral", approved: "ok", retired: "muted" };

export function ToneBank(props: LabProps & { onImprove: (s: Seed) => void }) {
  const bank = useRpc("tone.lines.list", { character: props.character, tone: props.tone });
  const [status, setStatus] = useState<StatusFilter>("all");
  const [tag, setTag] = useState<LineSituation | "all">("all");
  return (
    <section className={lab.panel} data-role="tone-lab-bank" aria-labelledby="tone-lab-bank">
      <h2 className={styles.secHead} id="tone-lab-bank">
        The line bank
      </h2>
      <p className={styles.note}>
        Lines the model is shown as examples, three a beat, matched by situation; never aired as written. {STATUS_ORDER.map((s) => `${STATUS_NAME[s]}: ${STATUS_HINT[s]}`).join(". ")}.
      </p>
      <QueryView query={bank} isEmpty={(b) => b.lines.length === 0} emptyTitle="No lines in this tone's bank yet" emptyHint="Draft some in Variants above, or approve a line from the inbox: the first one starts the file.">
        {(b) => {
          const count = (s: LineStatus) => b.lines.filter((l) => l.status === s).length;
          const tags = SITUATION_ORDER.filter((t) => b.lines.some((l) => l.tags.includes(t)));
          const shown = b.lines.filter((l) => (status === "all" || l.status === status) && (tag === "all" || l.tags.includes(tag)));
          return (
            <>
              <div className={styles.toolsRow}>
                <PillGroup<StatusFilter>
                  label="Status"
                  options={[{ id: "all", label: "All", count: b.lines.length }, ...STATUS_ORDER.map((s) => ({ id: s, label: STATUS_NAME[s], count: count(s) }))]}
                  value={status}
                  onChange={setStatus}
                />
                <label className={styles.fld}>
                  Situation
                  <select value={tag} onChange={(e) => setTag(e.target.value as LineSituation | "all")} data-role="tone-lab-bank-tag">
                    <option value="all">Every situation</option>
                    {tags.map((t) => (
                      <option key={t} value={t}>
                        {SITUATION_NAME[t]}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              {shown.length === 0 ? (
                <StatePanel state="empty" title="No line matches these filters" />
              ) : (
                <ul className={lab.list} data-role="tone-lab-bank-lines">
                  {shown.map((l) => (
                    <BankRow key={l.id} l={l} {...props} reload={bank.reload} />
                  ))}
                </ul>
              )}
            </>
          );
        }}
      </QueryView>
    </section>
  );
}

function BankRow({ l, reload, ...props }: LabProps & { l: ToneLine; reload: () => void; onImprove: (s: Seed) => void }) {
  const setStatus = useMutation("tone.lines.set_status");
  const add = useMutation("tone.lines.add");
  const [editing, setEditing] = useState<string | undefined>(undefined);
  const [error, setError] = useState<string | undefined>(undefined);
  const [added, setAdded] = useState<string | undefined>(undefined);
  const busy = setStatus.pending || add.pending;

  const move = async (to: LineStatus) => {
    setError(undefined);
    const r = await setStatus.run({ character: props.character, tone: props.tone, id: l.id, status: to });
    if (!r.ok) return setError(`${viewOfError(r.error).title}: ${r.error.message}`);
    props.onWrote(r.value.file);
    reload();
  };

  const saveNew = async () => {
    if (editing === undefined) return;
    setError(undefined);
    const r = await add.run({ character: props.character, tone: props.tone, text: editing, tags: l.tags, status: "approved" });
    if (!r.ok) return setError(`${viewOfError(r.error).title}: ${r.error.message}`);
    setEditing(undefined);
    setAdded(r.value.line.id);
    props.onWrote(r.value.file);
    reload();
  };

  return (
    <li className={lab.item} data-role="tone-lab-bank-line" data-status={l.status} data-line-id={l.id}>
      <p className={lab.text}>{l.text}</p>
      <div className={lab.meta}>
        <Chip tone={STATUS_TONE[l.status]}>{STATUS_NAME[l.status]}</Chip>
        {l.tags.map((t) => (
          <Chip key={t}>{SITUATION_NAME[t]}</Chip>
        ))}
        <code className={styles.code}>{l.id}</code>
        {added !== undefined && <Chip tone="ok">Saved as {added}</Chip>}
      </div>
      {editing !== undefined && (
        <textarea className={lab.field} aria-label={`Edit ${l.id} as a new line`} value={editing} maxLength={400} onChange={(e) => setEditing(e.target.value)} />
      )}
      <div className={lab.actions}>
        {editing !== undefined ? (
          <>
            <Button size="compact" variant="ink" disabled={busy || editing.trim() === "" || editing.trim() === l.text} onClick={() => void saveNew()}>
              Save as a new line
            </Button>
            <Button size="compact" onClick={() => setEditing(undefined)}>
              Cancel
            </Button>
          </>
        ) : (
          <>
            {l.status !== "approved" && (
              <Button size="compact" disabled={busy} onClick={() => void move("approved")}>
                {l.status === "retired" ? "Bring back" : "Approve"}
              </Button>
            )}
            {l.status !== "retired" && (
              <Button size="compact" disabled={busy} onClick={() => void move("retired")}>
                Retire
              </Button>
            )}
            <Button size="compact" disabled={busy} onClick={() => setEditing(l.text)}>
              Edit as new
            </Button>
            <Button size="compact" onClick={() => props.onImprove({ key: nextKey(), line: l.text, ...(l.tags[0] ? { situation: l.tags[0] } : {}), source: `bank line ${l.id}` })}>
              Variants of it
            </Button>
          </>
        )}
        {error !== undefined && <span className={lab.err}>{error}</span>}
      </div>
    </li>
  );
}
