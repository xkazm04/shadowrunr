import type { ChatMsg, Command } from "@shadowrunr/contract";
import { Sigil } from "../art/Sigil";
import { RANK_COLOR } from "../lore";
import type { SceneState } from "../sceneModel";

export function commandLabel(c: Command): string {
  switch (c.cmd) {
    case "summon":
      return `SUMMON ${c.foil}`;
    case "hint":
      return `HINT ${c.hex}`;
    case "grade":
      return `GRADE ${c.hex} ${c.grade}`;
    case "propose":
      return "PROPOSAL";
    case "vote":
      return `VOTE ${c.option}`;
  }
}

/** Command text without the `!cmd args` prefix the chip already shows. */
function bodyText(m: ChatMsg): string {
  const c = m.command;
  if (!c) return m.text;
  switch (c.cmd) {
    case "hint":
      return c.text;
    case "propose":
      return c.text;
    default:
      return "";
  }
}

export function ChatLane({ scene, now, title = "The Minions" }: { scene: SceneState; now: number; title?: string }) {
  // Newest at the bottom; column-reverse clips the oldest at the top without measuring.
  const ids = scene.lane.slice(-16).reverse();
  return (
    <div className="panel chat-lane">
      <div className="panel-title">
        <span>{title}</span>
        <span className="panel-sub">{scene.chats.size} lines in the ledger</span>
      </div>
      <div className="chat-rows" data-clip>
        {ids.map((id) => {
          const m = scene.chats.get(id);
          if (!m) return null;
          const purged = scene.purged[id];
          const heardAt = scene.heard.get(id);
          const isDecree = scene.decrees.some((d) => d.msgId === id);
          const fresh = now - m.ts_ms < 700;
          const text = bodyText(m);
          return (
            <div
              key={id}
              className={`chat-row${fresh ? " fresh" : ""}${heardAt !== undefined ? " heard" : ""}${isDecree ? " decreed" : ""}${purged ? " purged" : ""}`}
            >
              <Sigil rank={m.viewer.rank} size={28} />
              <div className="chat-body">
                <span className="chat-name" style={{ color: RANK_COLOR[m.viewer.rank] }}>
                  {m.viewer.name}
                </span>
                {purged ? (
                  <span className="chat-text purged-text">purged by the Warden</span>
                ) : (
                  <>
                    {m.command && <span className={`cmd-chip cmd-${m.command.cmd}`}>{commandLabel(m.command)}</span>}
                    {text && <span className="chat-text">{text}</span>}
                  </>
                )}
              </div>
              {heardAt !== undefined && !purged && <span className="heard-tag">HEARD</span>}
            </div>
          );
        })}
      </div>
    </div>
  );
}
