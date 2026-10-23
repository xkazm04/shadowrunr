// characters/<id>/package/moderation: the console's backup for the chat moderator (the operator's
// own Claude Code configures it through `shadowrunr mcp`). Shown only on the moderator
// character's package. Its own lazy chunk (characters.moderation in bundle-measure.mjs), so the
// host characters' package page carries none of it.
import type { CharacterDetail, ModeratorStatus } from "@shadowrunr/contract";
import { useState } from "react";
import { useMutation, useRpc, viewOfError } from "../../../data";
import { QueryView } from "../../../kit";
import { Head } from "../../../ui/Section";
import { Switch } from "../../../ui/Switch";
import { ConnectionPanel } from "./Connection";
import { LogPanel } from "./LogPanel";
import { armingText } from "./model";
import s from "./Moderation.module.css";
import { Confirm, PanelHead } from "./parts";
import { RulesPanel } from "./RulesPanel";
import { TestPanel } from "./TestPanel";

export function ModerationTab({ d }: { d: CharacterDetail }) {
  const status = useRpc("moderator.status", {});
  return (
    <div data-role="characters-moderation-tab">
      <Head
        kicker="Characters · the moderator"
        title="Moderation"
        lede={`${d.name} guards the Twitch chat from its own bot account. In shadow it only logs what it would do; armed, it deletes, times out and bans.`}
      />
      <QueryView query={status}>
        {(st) => (
          <div className={s.grid}>
            <ConnectionPanel st={st} reload={status.reload} />
            <ModePanel st={st} />
            <div className={s.wide}>
              <RulesPanel />
            </div>
            <TestPanel />
            <LogPanel />
          </div>
        )}
      </QueryView>
    </div>
  );
}

/** Shadow or armed. Arming asks first, with what shadow has seen; disarming never asks. */
function ModePanel({ st }: { st: ModeratorStatus }) {
  const set = useMutation("moderator.mode.set");
  const log = useRpc("moderator.log", { limit: 100 });
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState<string | undefined>(undefined);
  const armed = st.mode === "armed";
  const write = async (mode: ModeratorStatus["mode"]) => {
    setError(undefined);
    const r = await set.run({ mode });
    setAsking(false);
    if (!r.ok) setError(`${viewOfError(r.error).title}: ${r.error.message}`);
  };
  return (
    <section className={s.panel} data-role="moderation-mode" data-mode={st.mode} aria-labelledby="moderation-mode">
      <PanelHead id="moderation-mode" title="Mode" />
      <div className={s.row}>
        <Switch
          checked={armed}
          label="Armed"
          disabled={set.pending}
          onChange={(next) => {
            if (next) setAsking(true);
            else void write("shadow");
          }}
        />
        <span className={s.note} data-role="moderation-mode-text">
          {armed ? "Armed: decisions are carried out and the lines are posted." : "Shadow: decisions are logged as would-be actions; nothing reaches the chat."}
        </span>
      </div>
      <p className={s.note}>
        {st.counts.decisions.toLocaleString("en-US")} decisions logged, {st.counts.executed.toLocaleString("en-US")} carried out.
      </p>
      {asking && !armed && (
        <Confirm role="moderation-arm" text={armingText(st.counts.decisions, log.data?.entries)} yes="Arm the moderator" danger pending={set.pending} onYes={() => void write("armed")} onNo={() => setAsking(false)} />
      )}
      {error !== undefined && <p className={s.err}>{error}</p>}
    </section>
  );
}
